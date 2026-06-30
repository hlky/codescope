use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Args, Parser, Subcommand};

use crate::context::add_import_context;
use crate::lsp::ClangdOptions;
use crate::model::{Backend, Language, Symbol, SymbolKindFilter};
use crate::replace::{Pattern, ReplaceOptions, Replacement};
use crate::workspace::{language_for_path, read_text, source_files};

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
    Context(ContextArgs),
    ReplaceText(ReplaceTextArgs),
    ReplaceRegex(ReplaceRegexArgs),
    Replace(ReplaceSymbolArgs),
    RenameSymbol(RenameSymbolArgs),
    RewriteImport(RewriteImportArgs),
    RewriteMarkdown(RewriteMarkdownArgs),
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

pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(EXIT_CONFIG);
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
    }
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
