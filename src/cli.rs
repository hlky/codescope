use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Args, Parser, Subcommand, error::ErrorKind};

use crate::context::add_import_context;
use crate::context_pack::{ContextPack, ContextPackItem};
use crate::diagnostics::{DiagnosticOptions, DiagnosticRecord, DiagnosticTool};
use crate::lsp::{ClangdOptions, NavigationRequest};
use crate::model::{Backend, Language, NavigationRecord, Symbol, SymbolKind, SymbolKindFilter};
use crate::replace::{Pattern, ReplaceOptions, Replacement};
use crate::workspace::{language_for_path, line_slice, read_text, source_files};

const EXIT_FOUND: u8 = 0;
const EXIT_NO_MATCH: u8 = 1;
const EXIT_CONFIG: u8 = 2;
const EXIT_BACKEND: u8 = 3;

#[derive(Parser, Debug)]
#[command(
    name = "codescope",
    version,
    about = "Extract code symbols and context from source trees."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    ListFunctions(QueryArgs),
    ListHeadings(QueryArgs),
    ExtractFunction(NamedArgs),
    ExtractSection(NamedArgs),
    ExtractBlock(BlockArgs),
    ExtractSymbol(SymbolArgs),
    ExtractVariable(VariableArgs),
    References(NamedArgs),
    Callers(NamedArgs),
    Definition(NavigationArgs),
    TypeOf(NavigationArgs),
    Hover(NavigationArgs),
    Context(ContextArgs),
    ContextPack(ContextPackArgs),
    ReplaceText(ReplaceTextArgs),
    ReplaceRegex(ReplaceRegexArgs),
    Replace(ReplaceSymbolArgs),
    RenameSymbol(RenameSymbolArgs),
    RewriteImport(RewriteImportArgs),
    RewriteMarkdown(RewriteMarkdownArgs),
    Diagnostics(DiagnosticsArgs),
}

#[derive(Args, Clone, Debug)]
struct CommonArgs {
    #[arg(long, default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long, value_enum)]
    lang: Option<crate::model::LanguageFilter>,
    #[arg(long, value_enum, default_value_t = Backend::Auto)]
    backend: Backend,
    #[arg(long)]
    compile_commands_dir: Option<PathBuf>,
    #[arg(long)]
    json: bool,
    #[arg(long, default_value_t = 20)]
    max_matches: usize,
}

#[derive(Args, Clone, Debug)]
struct QueryArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    query: Option<String>,
}

#[derive(Args, Clone, Debug)]
struct NamedArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    name: String,
}

#[derive(Args, Clone, Debug)]
#[group(multiple = false)]
struct BlockSelectionArgs {
    #[arg(long)]
    largest: bool,
    #[arg(long)]
    smallest: bool,
}

#[derive(Args, Clone, Debug)]
struct BlockArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    name: String,
    #[arg(long)]
    around_line: Option<usize>,
    #[arg(long)]
    contains: Option<String>,
    #[command(flatten)]
    selection: BlockSelectionArgs,
}

#[derive(Args, Clone, Debug)]
struct SymbolArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    name: String,
    #[arg(long, value_enum, default_value_t = SymbolKindFilter::All)]
    kind: SymbolKindFilter,
}

#[derive(Args, Clone, Debug)]
struct VariableArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    name: String,
    #[arg(long)]
    scope: Option<String>,
}

#[derive(Args, Clone, Debug)]
struct ContextArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    name: String,
    #[arg(long, value_enum, default_value_t = SymbolKindFilter::All)]
    kind: SymbolKindFilter,
}

#[derive(Args, Clone, Debug)]
struct ContextPackArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    around_line: Option<usize>,
    #[arg(long, default_value_t = 8000)]
    budget: usize,
    #[arg(long)]
    intent: Option<String>,
}

#[derive(Args, Clone, Debug)]
struct EditCommonArgs {
    #[arg(long, default_value = ".")]
    path: PathBuf,
    #[arg(long, value_enum)]
    lang: Option<crate::model::LanguageFilter>,
    #[arg(long)]
    preview: bool,
    #[arg(long, conflicts_with = "preview")]
    apply: bool,
    #[arg(long)]
    include: Vec<String>,
    #[arg(long)]
    exclude: Vec<String>,
    #[arg(long, default_value_t = 50)]
    max_files: usize,
    #[arg(long)]
    confirm: bool,
}

#[derive(Args, Clone, Debug)]
struct ReplaceTextArgs {
    #[command(flatten)]
    common: EditCommonArgs,
    #[arg(long)]
    find: String,
    #[arg(long)]
    replace: String,
}

#[derive(Args, Clone, Debug)]
struct ReplaceRegexArgs {
    #[command(flatten)]
    common: EditCommonArgs,
    #[arg(long)]
    find: String,
    #[arg(long)]
    replace: String,
}

#[derive(Args, Clone, Debug)]
struct ReplaceSymbolArgs {
    #[command(flatten)]
    common: EditCommonArgs,
    #[arg(long)]
    name: String,
    #[arg(long = "with")]
    replacement: String,
    #[arg(long, value_enum)]
    kind: Option<SymbolKindFilter>,
}

#[derive(Args, Clone, Debug)]
struct RenameSymbolArgs {
    #[command(flatten)]
    common: EditCommonArgs,
    #[arg(long = "from")]
    from: String,
    #[arg(long = "to")]
    to: String,
    #[arg(long, value_enum)]
    kind: Option<SymbolKindFilter>,
}

#[derive(Args, Clone, Debug)]
struct RewriteImportArgs {
    #[command(flatten)]
    common: EditCommonArgs,
    #[arg(long = "from")]
    from: String,
    #[arg(long = "to")]
    to: String,
}

#[derive(Args, Clone, Debug)]
struct RewriteMarkdownArgs {
    #[command(flatten)]
    common: EditCommonArgs,
    #[arg(long = "heading-from")]
    heading_from: Option<String>,
    #[arg(long = "heading-to")]
    heading_to: Option<String>,
    #[arg(long = "link-from")]
    link_from: Option<String>,
    #[arg(long = "link-to")]
    link_to: Option<String>,
}

#[derive(Args, Clone, Debug)]
struct DiagnosticsArgs {
    #[arg(long, default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long, value_enum)]
    lang: Option<crate::model::LanguageFilter>,
    #[arg(long, value_enum, default_value_t = Backend::Auto)]
    backend: Backend,
    #[arg(long)]
    compile_commands_dir: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = DiagnosticTool::Auto)]
    tool: DiagnosticTool,
    #[arg(long)]
    json: bool,
    #[arg(long, default_value_t = 20)]
    max_matches: usize,
}

#[derive(Args, Clone, Debug)]
struct NavigationArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    line: Option<usize>,
    #[arg(long)]
    column: Option<usize>,
}

pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit = match error.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => EXIT_FOUND,
                _ => EXIT_CONFIG,
            };
            let _ = error.print();
            return ExitCode::from(exit);
        }
    };

    match run_inner(cli) {
        Ok(RunOutput::Symbols {
            symbols,
            json,
            source_output,
        }) => {
            if symbols.is_empty() {
                return ExitCode::from(EXIT_NO_MATCH);
            }
            let rendered = if json {
                match crate::output::json(&symbols) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("{error:#}");
                        return ExitCode::from(EXIT_CONFIG);
                    }
                }
            } else if source_output {
                crate::output::with_source(&symbols)
            } else {
                crate::output::list_plain(&symbols)
            };
            println!("{rendered}");
            ExitCode::from(EXIT_FOUND)
        }
        Ok(RunOutput::Replace(summary)) => {
            if summary.replacements == 0 {
                return ExitCode::from(EXIT_NO_MATCH);
            }
            println!("{}", crate::replace::render(&summary));
            ExitCode::from(EXIT_FOUND)
        }
        Ok(RunOutput::Diagnostics {
            records,
            json,
            backend_failed,
        }) => {
            if records.is_empty() {
                return ExitCode::from(EXIT_NO_MATCH);
            }
            let rendered = if json {
                match serde_json::to_string_pretty(&records) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("{error:#}");
                        return ExitCode::from(EXIT_CONFIG);
                    }
                }
            } else {
                crate::diagnostics::render_plain(&records)
            };
            println!("{rendered}");
            if backend_failed {
                return ExitCode::from(EXIT_BACKEND);
            }
            ExitCode::from(EXIT_FOUND)
        }
        Ok(RunOutput::Navigation { records, json }) => {
            if records.is_empty() {
                return ExitCode::from(EXIT_NO_MATCH);
            }
            let rendered = if json {
                match crate::output::navigation_json(&records) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("{error:#}");
                        return ExitCode::from(EXIT_CONFIG);
                    }
                }
            } else {
                crate::output::navigation_plain(&records)
            };
            println!("{rendered}");
            ExitCode::from(EXIT_FOUND)
        }
        Ok(RunOutput::ContextPack { pack, json }) => {
            if pack.items.is_empty() {
                return ExitCode::from(EXIT_NO_MATCH);
            }
            let rendered = if json {
                match serde_json::to_string_pretty(&pack) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("{error:#}");
                        return ExitCode::from(EXIT_CONFIG);
                    }
                }
            } else {
                crate::context_pack::render_plain(&pack)
            };
            println!("{rendered}");
            ExitCode::from(EXIT_FOUND)
        }
        Err(AppError::Config(error)) => {
            eprintln!("{error:#}");
            ExitCode::from(EXIT_CONFIG)
        }
        Err(AppError::Backend(error)) => {
            eprintln!("{error:#}");
            ExitCode::from(EXIT_BACKEND)
        }
    }
}

enum RunOutput {
    Symbols {
        symbols: Vec<Symbol>,
        json: bool,
        source_output: bool,
    },
    Replace(crate::replace::ReplaceSummary),
    Diagnostics {
        records: Vec<DiagnosticRecord>,
        json: bool,
        backend_failed: bool,
    },
    Navigation {
        records: Vec<NavigationRecord>,
        json: bool,
    },
    ContextPack {
        pack: ContextPack,
        json: bool,
    },
}

fn symbols_output(symbols: Vec<Symbol>, json: bool, source_output: bool) -> RunOutput {
    RunOutput::Symbols {
        symbols,
        json,
        source_output,
    }
}

fn run_inner(cli: Cli) -> Result<RunOutput, AppError> {
    match cli.command {
        Command::ListFunctions(args) => {
            let mut symbols =
                collect_symbols(&args.common, Some(SymbolKindFilter::Function), None)?;
            if let Some(query) = args.query {
                let query = query.to_ascii_lowercase();
                symbols.retain(|symbol| {
                    symbol.name.to_ascii_lowercase().contains(&query)
                        || symbol.qualified_name.to_ascii_lowercase().contains(&query)
                        || symbol
                            .path
                            .display()
                            .to_string()
                            .to_ascii_lowercase()
                            .contains(&query)
                });
            }
            symbols.truncate(args.common.max_matches);
            Ok(symbols_output(symbols, args.common.json, false))
        }
        Command::ListHeadings(args) => {
            let mut symbols = collect_markdown_headings(&args.common, args.query.as_deref())?;
            symbols.truncate(args.common.max_matches);
            Ok(symbols_output(symbols, args.common.json, false))
        }
        Command::ExtractFunction(args) => {
            let mut symbols = collect_symbols(
                &args.common,
                Some(SymbolKindFilter::Function),
                Some(&args.name),
            )?;
            symbols.truncate(args.common.max_matches);
            Ok(symbols_output(symbols, args.common.json, true))
        }
        Command::ExtractSection(args) => {
            let mut symbols = collect_markdown_headings(&args.common, Some(&args.name))?;
            symbols.truncate(args.common.max_matches);
            Ok(symbols_output(symbols, args.common.json, true))
        }
        Command::ExtractBlock(args) => {
            let mut broad_common = args.common.clone();
            broad_common.max_matches = usize::MAX;
            let named_blocks = collect_symbols(
                &broad_common,
                Some(SymbolKindFilter::Block),
                Some(&args.name),
            )?;
            let mut symbols = if args.contains.is_some() || args.around_line.is_some() {
                collect_symbols(&broad_common, Some(SymbolKindFilter::Block), None)?
                    .into_iter()
                    .filter(|candidate| {
                        named_blocks.iter().any(|parent| {
                            parent.path == candidate.path
                                && parent.start_line <= candidate.start_line
                                && candidate.end_line <= parent.end_line
                        })
                    })
                    .collect()
            } else {
                named_blocks
            };
            if let Some(needle) = args.contains {
                symbols.retain(|symbol| symbol.source.contains(&needle));
            }
            if let Some(line) = args.around_line {
                symbols.retain(|symbol| symbol.start_line <= line && line <= symbol.end_line);
            }
            if args.selection.largest {
                symbols.sort_by_key(|symbol| {
                    std::cmp::Reverse(symbol.end_line.saturating_sub(symbol.start_line))
                });
                symbols.truncate(1);
            } else if args.selection.smallest {
                symbols.sort_by_key(|symbol| symbol.end_line.saturating_sub(symbol.start_line));
                symbols.truncate(1);
            }
            symbols.truncate(args.common.max_matches);
            Ok(symbols_output(symbols, args.common.json, true))
        }
        Command::ExtractSymbol(args) => {
            let filter = if args.kind == SymbolKindFilter::All {
                None
            } else {
                Some(args.kind)
            };
            let mut symbols = collect_symbols(&args.common, filter, Some(&args.name))?;
            symbols.truncate(args.common.max_matches);
            Ok(symbols_output(symbols, args.common.json, true))
        }
        Command::ExtractVariable(args) => {
            let mut symbols = collect_symbols(
                &args.common,
                Some(SymbolKindFilter::Variable),
                Some(&args.name),
            )?;
            if let Some(scope) = args.scope {
                symbols.retain(|symbol| {
                    symbol.qualified_name.starts_with(&format!("{scope}."))
                        || symbol.qualified_name.starts_with(&format!("{scope}::"))
                        || symbol.qualified_name.contains(&format!(".{scope}."))
                        || symbol.qualified_name.contains(&format!("::{scope}::"))
                });
            }
            symbols.truncate(args.common.max_matches);
            Ok(symbols_output(symbols, args.common.json, true))
        }
        Command::References(args) => {
            let mut symbols = collect_references(&args.common, &args.name)?;
            symbols.truncate(args.common.max_matches);
            Ok(symbols_output(symbols, args.common.json, false))
        }
        Command::Callers(args) => {
            let mut symbols = collect_callers(&args.common, &args.name)?;
            symbols.truncate(args.common.max_matches);
            Ok(symbols_output(symbols, args.common.json, true))
        }
        Command::Definition(args) => run_navigation(args, NavigationRequest::Definition),
        Command::TypeOf(args) => run_navigation(args, NavigationRequest::TypeOf),
        Command::Hover(args) => run_navigation(args, NavigationRequest::Hover),
        Command::Context(args) => {
            let filter = if args.kind == SymbolKindFilter::All {
                None
            } else {
                Some(args.kind)
            };
            let symbols = collect_symbols(&args.common, filter, Some(&args.name))?;
            let mut symbols = add_import_context(symbols);
            symbols.truncate(args.common.max_matches);
            Ok(symbols_output(symbols, args.common.json, true))
        }
        Command::ContextPack(args) => run_context_pack(args),
        Command::ReplaceText(args) => run_replacement(
            &args.common,
            Replacement {
                pattern: Pattern::Literal(args.find),
                replacement: args.replace,
                label: "literal text",
                expand_captures: false,
            },
        ),
        Command::ReplaceRegex(args) => run_replacement(
            &args.common,
            Replacement {
                pattern: Pattern::Regex(args.find),
                replacement: args.replace,
                label: "regex",
                expand_captures: true,
            },
        ),
        Command::Replace(args) => {
            run_symbol_replacement(&args.common, &args.name, &args.replacement, args.kind)
        }
        Command::RenameSymbol(args) => {
            run_symbol_replacement(&args.common, &args.from, &args.to, args.kind)
        }
        Command::RewriteImport(args) => run_replacement(
            &import_common(args.common),
            Replacement {
                pattern: import_pattern(&args.from)?,
                replacement: "${1}".to_string() + &args.to + "${2}",
                label: "import/module path",
                expand_captures: true,
            },
        ),
        Command::RewriteMarkdown(args) => run_markdown_rewrite(args),
        Command::Diagnostics(args) => run_diagnostics(args),
    }
}

fn run_context_pack(args: ContextPackArgs) -> Result<RunOutput, AppError> {
    let subject = context_pack_subject(&args)?;
    let mut pack = ContextPack::new(subject.clone(), args.budget);
    if let Some(intent) = &args.intent {
        pack.notes.push(format!("intent: {intent}"));
    }

    let primary = if let Some(name) = &args.name {
        let mut common = args.common.clone();
        common.max_matches = common.max_matches.max(1);
        collect_symbols(&common, None, Some(name))?
    } else {
        collect_enclosing_symbols(&args.common, args.file.as_ref(), args.around_line)?
    };

    if primary.is_empty() {
        pack.notes
            .push("no primary definition or enclosing symbol matched".to_string());
        return Ok(RunOutput::ContextPack {
            pack,
            json: args.common.json,
        });
    }

    for symbol in primary.iter().cloned() {
        let role = if args.name.is_some() {
            "definition"
        } else {
            "enclosing-symbol"
        };
        pack.push_symbol(role, symbol, 1000, "primary match");
    }
    add_import_items(&mut pack, &primary);

    if let Some(name) = &args.name {
        collect_named_context(&mut pack, &args.common, name, &primary);
    }

    collect_related_tests(&mut pack, &args.common, args.name.as_deref(), &primary);
    collect_related_docs(&mut pack, &args.common, args.name.as_deref());
    collect_related_diagnostics(&mut pack, &args.common, args.name.as_deref(), &primary);

    pack.rank_dedupe_and_truncate();
    Ok(RunOutput::ContextPack {
        pack,
        json: args.common.json,
    })
}

fn run_navigation(args: NavigationArgs, request: NavigationRequest) -> Result<RunOutput, AppError> {
    let records = match navigation_query(&args)? {
        NavigationQuery::Name(name) => collect_navigation_by_name(&args.common, request, &name)?,
        NavigationQuery::Position { file, line, column } => {
            collect_navigation_by_position(&args.common, request, &file, line, column)?
        }
    };
    let mut records = dedupe_navigation(records);
    records.truncate(args.common.max_matches);
    Ok(RunOutput::Navigation {
        records,
        json: args.common.json,
    })
}

enum NavigationQuery {
    Name(String),
    Position {
        file: PathBuf,
        line: usize,
        column: usize,
    },
}

fn navigation_query(args: &NavigationArgs) -> Result<NavigationQuery, AppError> {
    match (&args.name, &args.file, args.line, args.column) {
        (Some(name), None, None, None) => Ok(NavigationQuery::Name(name.clone())),
        (None, Some(file), Some(line), Some(column)) => {
            if line == 0 || column == 0 {
                return Err(AppError::Config(anyhow::anyhow!(
                    "--line and --column are 1-based and must be greater than zero"
                )));
            }
            Ok(NavigationQuery::Position {
                file: file.clone(),
                line,
                column,
            })
        }
        (Some(_), Some(_), _, _) => Err(AppError::Config(anyhow::anyhow!(
            "navigation accepts either --name or --file --line --column, not both"
        ))),
        (Some(_), None, _, _) => Err(AppError::Config(anyhow::anyhow!(
            "--name cannot be combined with --line or --column"
        ))),
        (None, Some(_), _, _) => Err(AppError::Config(anyhow::anyhow!(
            "position-based navigation requires --file, --line, and --column"
        ))),
        (None, None, _, _) => Err(AppError::Config(anyhow::anyhow!(
            "navigation requires --name or --file --line --column"
        ))),
    }
}

fn collect_navigation_by_name(
    common: &CommonArgs,
    request: NavigationRequest,
    name: &str,
) -> Result<Vec<NavigationRecord>, AppError> {
    let path = common
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", common.path.display()))
        .map_err(AppError::Config)?;
    let mut out = Vec::new();
    let mut c_family = Vec::new();
    for file in source_files(&path, common.lang) {
        let Some(text) = read_text(&file) else {
            continue;
        };
        match language_for_path(&file) {
            Some(Language::Python) => {
                out.extend(python_navigation_records(
                    &file,
                    &text,
                    request,
                    name,
                    common.max_matches - out.len(),
                ));
            }
            Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
                c_family.push((file, text));
            }
            _ => {}
        }
        if out.len() >= common.max_matches {
            return Ok(out);
        }
    }

    if !c_family.is_empty() && out.len() < common.max_matches {
        let options = clangd_options(common, &path)?;
        if common.backend == Backend::Lsp {
            out.extend(
                crate::lsp::navigate_name(
                    &c_family,
                    &options,
                    request,
                    name,
                    common.max_matches - out.len(),
                )
                .map_err(AppError::Backend)?,
            );
        } else if common.backend == Backend::Auto
            && crate::lsp::clangd_available()
            && let Ok(records) = crate::lsp::navigate_name(
                &c_family,
                &options,
                request,
                name,
                common.max_matches - out.len(),
            )
        {
            out.extend(records);
        } else if request == NavigationRequest::Definition {
            for (file, text) in &c_family {
                out.extend(
                    crate::cfamily::symbols(file, text, Backend::TreeSitter, None, Some(name))
                        .map_err(AppError::Backend)?
                        .into_iter()
                        .map(|symbol| NavigationRecord::from_symbol(symbol, 1, 1)),
                );
                if out.len() >= common.max_matches {
                    break;
                }
            }
        }
    }
    Ok(out)
}

fn collect_navigation_by_position(
    common: &CommonArgs,
    request: NavigationRequest,
    file: &PathBuf,
    line: usize,
    column: usize,
) -> Result<Vec<NavigationRecord>, AppError> {
    let base = common
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", common.path.display()))
        .map_err(AppError::Config)?;
    let path = if file.is_absolute() {
        file.clone()
    } else if base.is_file() {
        base.parent()
            .map(|parent| parent.join(file))
            .unwrap_or_else(|| file.clone())
    } else {
        base.join(file)
    }
    .canonicalize()
    .with_context(|| format!("failed to resolve --file {}", file.display()))
    .map_err(AppError::Config)?;
    let text = read_text(&path).ok_or_else(|| {
        AppError::Config(anyhow::anyhow!("failed to read --file {}", path.display()))
    })?;
    match language_for_path(&path) {
        Some(Language::Python) => {
            let Some(name) = token_at_position(&text, line, column) else {
                return Ok(Vec::new());
            };
            Ok(python_navigation_records(
                &path,
                &text,
                request,
                &name,
                common.max_matches,
            ))
        }
        Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
            let options = clangd_options(common, &base)?;
            crate::lsp::navigate_position(&path, &text, &options, request, line, column)
                .map_err(AppError::Backend)
        }
        _ => Ok(Vec::new()),
    }
}

fn python_navigation_records(
    path: &std::path::Path,
    text: &str,
    request: NavigationRequest,
    name: &str,
    max_matches: usize,
) -> Vec<NavigationRecord> {
    let mut symbols = crate::python::symbols(path, text, None, Some(name));
    if request == NavigationRequest::Definition {
        symbols.extend(crate::python::import_symbols(path, text, Some(name)));
    }
    let mut records = symbols
        .into_iter()
        .map(|symbol| {
            let mut record =
                NavigationRecord::from_symbol(symbol, 1, line_len(text, name).unwrap_or(1).max(1));
            match request {
                NavigationRequest::Definition => {
                    record.kind = SymbolKind::Definition;
                }
                NavigationRequest::TypeOf => {
                    record.kind = SymbolKind::Type;
                    record.detail =
                        "python type information is best-effort; structural definition shown"
                            .to_string();
                }
                NavigationRequest::Hover => {
                    record.kind = SymbolKind::Definition;
                    record.detail = format!(
                        "{} {}",
                        record.kind,
                        if record.qualified_name.is_empty() {
                            &record.name
                        } else {
                            &record.qualified_name
                        }
                    );
                }
            }
            record
        })
        .collect::<Vec<_>>();
    records.truncate(max_matches);
    records
}

fn token_at_position(text: &str, line: usize, column: usize) -> Option<String> {
    let line_text = text.lines().nth(line.checked_sub(1)?)?;
    let chars = line_text.chars().collect::<Vec<_>>();
    let mut idx = column.saturating_sub(1).min(chars.len().saturating_sub(1));
    if chars.is_empty() {
        return None;
    }
    if !is_name_char(chars[idx]) && idx > 0 {
        idx -= 1;
    }
    if !is_name_char(chars[idx]) {
        return None;
    }
    let mut start = idx;
    while start > 0 && is_name_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < chars.len() && is_name_char(chars[end]) {
        end += 1;
    }
    Some(chars[start..end].iter().collect())
}

fn is_name_char(ch: char) -> bool {
    ch == '_' || ch == '.' || ch.is_ascii_alphanumeric()
}

fn line_len(text: &str, name: &str) -> Option<usize> {
    text.lines()
        .find(|line| line.contains(name))
        .map(|line| line.chars().count())
}

fn dedupe_navigation(records: Vec<NavigationRecord>) -> Vec<NavigationRecord> {
    let mut out = Vec::new();
    for record in records {
        if !out.iter().any(|existing: &NavigationRecord| {
            existing.path == record.path
                && existing.start_line == record.start_line
                && existing.start_column == record.start_column
                && existing.end_line == record.end_line
                && existing.end_column == record.end_column
                && existing.kind == record.kind
        }) {
            out.push(record);
        }
    }
    out
}

fn context_pack_subject(args: &ContextPackArgs) -> Result<String, AppError> {
    match (&args.name, &args.file, args.around_line) {
        (Some(name), None, None) => Ok(name.clone()),
        (Some(name), None, Some(_)) => Ok(name.clone()),
        (None, Some(file), Some(line)) => Ok(format!("{} around line {line}", file.display())),
        (Some(_), Some(_), _) => Err(AppError::Config(anyhow::anyhow!(
            "context-pack accepts either --name or --file, not both"
        ))),
        (None, Some(_), None) => Err(AppError::Config(anyhow::anyhow!(
            "context-pack --file requires --around-line"
        ))),
        (None, None, _) => Err(AppError::Config(anyhow::anyhow!(
            "context-pack requires --name or --file --around-line"
        ))),
    }
}

fn collect_enclosing_symbols(
    common: &CommonArgs,
    file: Option<&PathBuf>,
    around_line: Option<usize>,
) -> Result<Vec<Symbol>, AppError> {
    let Some(file) = file else {
        return Ok(Vec::new());
    };
    let Some(line) = around_line else {
        return Ok(Vec::new());
    };
    let base = common
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", common.path.display()))
        .map_err(AppError::Config)?;
    let file = if file.is_absolute() {
        file.clone()
    } else {
        base.join(file)
    }
    .canonicalize()
    .with_context(|| format!("failed to resolve --file {}", file.display()))
    .map_err(AppError::Config)?;
    let mut file_common = common.clone();
    file_common.path = file;
    file_common.max_matches = usize::MAX;
    let mut symbols = collect_symbols(&file_common, None, None)?;
    symbols.retain(|symbol| symbol.start_line <= line && line <= symbol.end_line);
    symbols.sort_by_key(|symbol| {
        (
            symbol.end_line.saturating_sub(symbol.start_line),
            symbol.start_line,
        )
    });
    symbols.truncate(common.max_matches);
    Ok(symbols)
}

fn add_import_items(pack: &mut ContextPack, symbols: &[Symbol]) {
    for symbol in symbols {
        let Some(text) = read_text(&symbol.path) else {
            continue;
        };
        let Some((start_line, end_line, source)) =
            crate::context::import_context_range(symbol.language, &text)
        else {
            continue;
        };
        pack.push(ContextPackItem::synthetic(
            "imports",
            symbol.path.clone(),
            start_line,
            end_line,
            symbol.language,
            symbol.backend.clone(),
            900,
            "imports/includes for primary item",
            source,
        ));
    }
}

fn collect_named_context(
    pack: &mut ContextPack,
    common: &CommonArgs,
    name: &str,
    primary: &[Symbol],
) {
    match collect_callers(common, name) {
        Ok(callers) => {
            for caller in callers {
                pack.push_symbol("caller", caller, 800, "direct caller");
            }
        }
        Err(error) => pack.notes.push(format!(
            "caller collection failed: {}",
            app_error_note(error)
        )),
    }
    match collect_references(common, name) {
        Ok(references) => {
            for reference in references {
                if primary.iter().any(|symbol| same_range(symbol, &reference)) {
                    continue;
                }
                pack.push_symbol("reference", reference, 700, "direct reference");
            }
        }
        Err(error) => pack.notes.push(format!(
            "reference collection failed: {}",
            app_error_note(error)
        )),
    }
    let mut cmake_common = common.clone();
    cmake_common.lang = Some(crate::model::LanguageFilter::Cmake);
    if let Ok(symbols) = collect_symbols(&cmake_common, Some(SymbolKindFilter::Target), Some(name))
    {
        for symbol in symbols {
            pack.push_symbol("build", symbol, 550, "matching CMake target");
        }
    }
    if let Ok(references) = collect_references(&cmake_common, name) {
        for reference in references {
            pack.push_symbol("build", reference, 540, "CMake reference");
        }
    }
}

fn collect_related_tests(
    pack: &mut ContextPack,
    common: &CommonArgs,
    name: Option<&str>,
    primary: &[Symbol],
) {
    let Some(name) = name else {
        return;
    };
    let Ok(root) = common.path.canonicalize() else {
        return;
    };
    let needle = name
        .replace("::", ".")
        .rsplit('.')
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    for file in source_files(&root, common.lang) {
        if !is_test_path(&file) {
            continue;
        }
        let Some(text) = read_text(&file) else {
            continue;
        };
        if !text.to_ascii_lowercase().contains(&needle) {
            continue;
        }
        let line = first_matching_line(&text, &needle).unwrap_or(1);
        let start = line.saturating_sub(3).max(1);
        let end = line + 6;
        let language = language_for_path(&file).unwrap_or(Language::Text);
        pack.push(ContextPackItem::synthetic(
            "test",
            file,
            start,
            end,
            language,
            "lexical",
            if primary.iter().any(|symbol| symbol.language == language) {
                650
            } else {
                600
            },
            "nearby test mention",
            line_slice(&text, start, end),
        ));
    }
}

fn collect_related_docs(pack: &mut ContextPack, common: &CommonArgs, name: Option<&str>) {
    let Some(name) = name else {
        return;
    };
    let Ok(root) = common.path.canonicalize() else {
        return;
    };
    let needle = name
        .replace("::", ".")
        .rsplit('.')
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    for file in source_files(&root, Some(crate::model::LanguageFilter::Markdown)) {
        let Some(text) = read_text(&file) else {
            continue;
        };
        if !text.to_ascii_lowercase().contains(&needle) {
            continue;
        }
        let line = first_matching_line(&text, &needle).unwrap_or(1);
        let start = line.saturating_sub(3).max(1);
        let end = line + 8;
        pack.push(ContextPackItem::synthetic(
            "docs",
            file,
            start,
            end,
            Language::Markdown,
            "lexical",
            500,
            "documentation mention",
            line_slice(&text, start, end),
        ));
    }
}

fn collect_related_diagnostics(
    pack: &mut ContextPack,
    common: &CommonArgs,
    name: Option<&str>,
    primary: &[Symbol],
) {
    let run = match crate::diagnostics::collect(&DiagnosticOptions {
        path: common.path.clone(),
        file: None,
        root: common.root.clone(),
        lang: common.lang,
        backend: common.backend,
        compile_commands_dir: common.compile_commands_dir.clone(),
        tool: DiagnosticTool::Auto,
        max_matches: 20,
    }) {
        Ok(run) => run,
        Err(error) => {
            pack.notes
                .push(format!("diagnostic collection failed: {error:#}"));
            return;
        }
    };
    let needle = name.map(str::to_ascii_lowercase);
    for record in run.records {
        let touches_primary = primary.iter().any(|symbol| {
            symbol.path == record.path
                && ranges_overlap(
                    symbol.start_line,
                    symbol.end_line,
                    record.start_line,
                    record.end_line,
                )
        });
        let mentions_name = needle
            .as_ref()
            .is_some_and(|needle| record.message.to_ascii_lowercase().contains(needle));
        if !touches_primary && !mentions_name {
            continue;
        }
        pack.push(ContextPackItem::synthetic(
            "diagnostic",
            record.path,
            record.start_line,
            record.end_line,
            record.language,
            record.backend,
            450,
            format!("{} {}", record.tool, record.severity),
            record.message,
        ));
    }
}

fn same_range(left: &Symbol, right: &Symbol) -> bool {
    left.path == right.path
        && left.start_line == right.start_line
        && left.end_line == right.end_line
}

fn ranges_overlap(
    left_start: usize,
    left_end: usize,
    right_start: usize,
    right_end: usize,
) -> bool {
    left_start <= right_end && right_start <= left_end
}

fn is_test_path(path: &std::path::Path) -> bool {
    path.components().any(|part| {
        let value = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        value == "tests" || value == "test"
    }) || path
        .file_stem()
        .is_some_and(|stem| stem.to_string_lossy().to_ascii_lowercase().contains("test"))
}

fn first_matching_line(text: &str, needle: &str) -> Option<usize> {
    text.lines()
        .position(|line| line.to_ascii_lowercase().contains(needle))
        .map(|idx| idx + 1)
}

fn app_error_note(error: AppError) -> String {
    match error {
        AppError::Config(error) | AppError::Backend(error) => format!("{error:#}"),
    }
}

fn run_diagnostics(args: DiagnosticsArgs) -> Result<RunOutput, AppError> {
    let run = crate::diagnostics::collect(&DiagnosticOptions {
        path: args.path,
        file: args.file,
        root: args.root,
        lang: args.lang,
        backend: args.backend,
        compile_commands_dir: args.compile_commands_dir,
        tool: args.tool,
        max_matches: args.max_matches,
    })
    .map_err(AppError::Backend)?;
    Ok(RunOutput::Diagnostics {
        records: run.records,
        json: args.json,
        backend_failed: run.backend_failed,
    })
}

fn run_replacement(
    common: &EditCommonArgs,
    replacement: Replacement,
) -> Result<RunOutput, AppError> {
    let summary =
        crate::replace::run(&replace_options(common), &replacement).map_err(AppError::Config)?;
    Ok(RunOutput::Replace(summary))
}

fn run_symbol_replacement(
    common: &EditCommonArgs,
    from: &str,
    to: &str,
    kind: Option<SymbolKindFilter>,
) -> Result<RunOutput, AppError> {
    crate::replace::validate_symbol_request(from, to, kind).map_err(AppError::Config)?;
    if let Some(kind) = kind {
        let query = CommonArgs {
            path: common.path.clone(),
            root: None,
            lang: common.lang,
            backend: Backend::Auto,
            compile_commands_dir: None,
            json: false,
            max_matches: 1,
        };
        if collect_symbols(&query, Some(kind), Some(from))?.is_empty() {
            return Ok(RunOutput::Replace(crate::replace::ReplaceSummary {
                files_scanned: 0,
                files_changed: 0,
                replacements: 0,
                applied: common.apply,
                diffs: Vec::new(),
            }));
        }
    }
    run_replacement(
        common,
        Replacement {
            pattern: Pattern::Identifier(from.to_string()),
            replacement: to.to_string(),
            label: "symbol",
            expand_captures: false,
        },
    )
}

fn run_markdown_rewrite(args: RewriteMarkdownArgs) -> Result<RunOutput, AppError> {
    let mut common = args.common;
    common.lang = Some(crate::model::LanguageFilter::Markdown);
    let (pattern, replacement, label) = match (
        args.heading_from,
        args.heading_to,
        args.link_from,
        args.link_to,
    ) {
        (Some(from), Some(to), None, None) => (
            Pattern::Regex(format!(r"(?m)^(#+\s*){}(\s*#*\s*)$", regex::escape(&from))),
            "${1}".to_string() + &to + "${2}",
            "markdown heading",
        ),
        (None, None, Some(from), Some(to)) => (
            Pattern::Regex(format!(
                r"(\[[^\]]+\]\(){}((?:#[^)]+)?\))",
                regex::escape(&from)
            )),
            "${1}".to_string() + &to + "${2}",
            "markdown link",
        ),
        _ => {
            return Err(AppError::Config(anyhow::anyhow!(
                "rewrite-markdown requires either --heading-from/--heading-to or --link-from/--link-to"
            )));
        }
    };
    run_replacement(
        &common,
        Replacement {
            pattern,
            replacement,
            label,
            expand_captures: true,
        },
    )
}

fn replace_options(common: &EditCommonArgs) -> ReplaceOptions {
    ReplaceOptions {
        path: common.path.clone(),
        lang: common.lang,
        include: common.include.clone(),
        exclude: common.exclude.clone(),
        max_files: common.max_files,
        apply: common.apply,
        confirm: common.confirm,
    }
}

fn import_common(mut common: EditCommonArgs) -> EditCommonArgs {
    if common.lang.is_none() {
        common.lang = Some(crate::model::LanguageFilter::Python);
    }
    common
}

fn import_pattern(from: &str) -> Result<Pattern, AppError> {
    crate::replace::validate_qualified_identifier(from, "--from").map_err(AppError::Config)?;
    Ok(Pattern::Regex(format!(
        r"(?m)^(\s*(?:from|import)\s+){}(\b)",
        regex::escape(from)
    )))
}

fn collect_symbols(
    common: &CommonArgs,
    kind: Option<SymbolKindFilter>,
    wanted: Option<&str>,
) -> Result<Vec<Symbol>, AppError> {
    let path = common
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", common.path.display()))
        .map_err(AppError::Config)?;
    let mut out = Vec::new();
    let mut c_family = Vec::new();
    for file in source_files(&path, common.lang) {
        let Some(text) = read_text(&file) else {
            continue;
        };
        match language_for_path(&file) {
            Some(Language::Python) => {
                out.extend(crate::python::symbols(&file, &text, kind, wanted))
            }
            Some(Language::Cmake) => {
                out.extend(crate::cmake::symbols(&file, &text, kind, wanted));
            }
            Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
                c_family.push((file, text));
            }
            Some(Language::Markdown)
                if kind.is_none()
                    || matches!(
                        kind,
                        Some(SymbolKindFilter::All | SymbolKindFilter::Heading)
                    ) =>
            {
                out.extend(crate::markdown::headings(&file, &text, wanted));
            }
            _ => {}
        }
        if out.len() >= common.max_matches {
            break;
        }
    }
    if !c_family.is_empty() && out.len() < common.max_matches {
        out.extend(collect_c_family_symbols(
            common,
            &path,
            &c_family,
            kind,
            wanted,
            common.max_matches - out.len(),
        )?);
    }
    Ok(out)
}

fn collect_markdown_headings(
    common: &CommonArgs,
    wanted: Option<&str>,
) -> Result<Vec<Symbol>, AppError> {
    let path = common
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", common.path.display()))
        .map_err(AppError::Config)?;
    let mut out = Vec::new();
    for file in source_files(&path, common.lang) {
        let Some(text) = read_text(&file) else {
            continue;
        };
        if language_for_path(&file) == Some(Language::Markdown) {
            out.extend(crate::markdown::headings(&file, &text, wanted));
        }
        if out.len() >= common.max_matches {
            break;
        }
    }
    Ok(out)
}

fn collect_references(common: &CommonArgs, wanted: &str) -> Result<Vec<Symbol>, AppError> {
    let path = common
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", common.path.display()))
        .map_err(AppError::Config)?;
    let mut out = Vec::new();
    let mut c_family = Vec::new();
    for file in source_files(&path, common.lang) {
        let Some(text) = read_text(&file) else {
            continue;
        };
        match language_for_path(&file) {
            Some(Language::Python) => out.extend(crate::python::references(
                &file,
                &text,
                wanted,
                common.max_matches - out.len(),
            )),
            Some(Language::Cmake) => out.extend(crate::cmake::references(
                &file,
                &text,
                wanted,
                common.max_matches - out.len(),
            )),
            Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
                c_family.push((file, text));
            }
            _ => {}
        }
        if out.len() >= common.max_matches {
            break;
        }
    }
    if !c_family.is_empty() && out.len() < common.max_matches {
        out.extend(collect_c_family_references(
            common,
            &path,
            &c_family,
            wanted,
            common.max_matches - out.len(),
        )?);
    }
    Ok(out)
}

fn collect_callers(common: &CommonArgs, wanted: &str) -> Result<Vec<Symbol>, AppError> {
    let path = common
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", common.path.display()))
        .map_err(AppError::Config)?;
    let mut out = Vec::new();
    let mut c_family = Vec::new();
    for file in source_files(&path, common.lang) {
        let Some(text) = read_text(&file) else {
            continue;
        };
        match language_for_path(&file) {
            Some(Language::Python) => out.extend(crate::python::callers(
                &file,
                &text,
                wanted,
                common.max_matches - out.len(),
            )),
            Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
                c_family.push((file, text));
            }
            _ => {}
        }
        if out.len() >= common.max_matches {
            break;
        }
    }
    if !c_family.is_empty() && out.len() < common.max_matches {
        out.extend(collect_c_family_callers(
            common,
            &path,
            &c_family,
            wanted,
            common.max_matches - out.len(),
        )?);
    }
    Ok(out)
}

fn collect_c_family_symbols(
    common: &CommonArgs,
    search_root: &std::path::Path,
    files: &[(PathBuf, String)],
    kind: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    max_matches: usize,
) -> Result<Vec<Symbol>, AppError> {
    let options = clangd_options(common, search_root)?;
    if common.backend == Backend::Lsp {
        return crate::lsp::document_symbols(files, &options, kind, wanted, max_matches)
            .map_err(AppError::Backend);
    }
    if common.backend == Backend::Auto
        && crate::lsp::clangd_available()
        && let Ok(symbols) =
            crate::lsp::document_symbols(files, &options, kind, wanted, max_matches)
        && !symbols.is_empty()
    {
        return Ok(symbols);
    }

    let backend = match common.backend {
        Backend::Lexical => Backend::Lexical,
        _ => Backend::TreeSitter,
    };
    let mut out = Vec::new();
    for (file, text) in files {
        out.extend(
            crate::cfamily::symbols(file, text, backend, kind, wanted)
                .map_err(AppError::Backend)?,
        );
        if out.len() >= max_matches {
            break;
        }
    }
    out.truncate(max_matches);
    Ok(out)
}

fn collect_c_family_references(
    common: &CommonArgs,
    search_root: &std::path::Path,
    files: &[(PathBuf, String)],
    wanted: &str,
    max_matches: usize,
) -> Result<Vec<Symbol>, AppError> {
    let options = clangd_options(common, search_root)?;
    if common.backend == Backend::Lsp {
        return crate::lsp::references(files, &options, wanted, max_matches)
            .map_err(AppError::Backend);
    }
    if common.backend == Backend::Auto
        && crate::lsp::clangd_available()
        && let Ok(symbols) = crate::lsp::references(files, &options, wanted, max_matches)
        && !symbols.is_empty()
    {
        return Ok(symbols);
    }

    let mut out = Vec::new();
    for (file, text) in files {
        out.extend(crate::cfamily::references(
            file,
            text,
            wanted,
            max_matches - out.len(),
        ));
        if out.len() >= max_matches {
            break;
        }
    }
    Ok(out)
}

fn collect_c_family_callers(
    common: &CommonArgs,
    search_root: &std::path::Path,
    files: &[(PathBuf, String)],
    wanted: &str,
    max_matches: usize,
) -> Result<Vec<Symbol>, AppError> {
    let options = clangd_options(common, search_root)?;
    if common.backend == Backend::Lsp {
        return crate::lsp::callers(files, &options, wanted, max_matches)
            .map_err(AppError::Backend);
    }
    if common.backend == Backend::Auto
        && crate::lsp::clangd_available()
        && let Ok(symbols) = crate::lsp::callers(files, &options, wanted, max_matches)
        && !symbols.is_empty()
    {
        return Ok(symbols);
    }

    let symbols = collect_c_family_symbols(
        common,
        search_root,
        files,
        Some(SymbolKindFilter::Function),
        None,
        usize::MAX,
    )?;
    let short = wanted
        .replace('.', "::")
        .rsplit("::")
        .next()
        .unwrap_or(wanted)
        .to_string();
    let pattern = regex::Regex::new(&format!(r"(^|[^A-Za-z0-9_]){}\s*\(", regex::escape(&short)))
        .map_err(|error| AppError::Config(error.into()))?;
    Ok(symbols
        .into_iter()
        .filter(|symbol| symbol.name != short && symbol.qualified_name != wanted)
        .filter(|symbol| pattern.is_match(&symbol.source))
        .take(max_matches)
        .collect())
}

fn clangd_options(
    common: &CommonArgs,
    search_root: &std::path::Path,
) -> Result<ClangdOptions, AppError> {
    let root = match &common.root {
        Some(root) => root
            .canonicalize()
            .with_context(|| format!("failed to resolve --root {}", root.display()))
            .map_err(AppError::Config)?,
        None if search_root.is_dir() => search_root.to_path_buf(),
        None => search_root
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| search_root.to_path_buf()),
    };
    Ok(ClangdOptions {
        root,
        compile_commands_dir: common.compile_commands_dir.clone(),
    })
}

enum AppError {
    Config(anyhow::Error),
    Backend(anyhow::Error),
}
