use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Args, Parser, Subcommand};

use crate::context::add_import_context;
use crate::model::{Backend, Language, Symbol, SymbolKindFilter};
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
    ExtractFunction(NamedArgs),
    ExtractSymbol(SymbolArgs),
    ExtractVariable(VariableArgs),
    References(NamedArgs),
    Callers(NamedArgs),
    Context(ContextArgs),
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

pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(EXIT_CONFIG);
        }
    };

    match run_inner(cli) {
        Ok((symbols, json, source_output)) => {
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

fn run_inner(cli: Cli) -> Result<(Vec<Symbol>, bool, bool), AppError> {
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
            Ok((symbols, args.common.json, false))
        }
        Command::ExtractFunction(args) => {
            let mut symbols = collect_symbols(
                &args.common,
                Some(SymbolKindFilter::Function),
                Some(&args.name),
            )?;
            symbols.truncate(args.common.max_matches);
            Ok((symbols, args.common.json, true))
        }
        Command::ExtractSymbol(args) => {
            let filter = if args.kind == SymbolKindFilter::All {
                None
            } else {
                Some(args.kind)
            };
            let mut symbols = collect_symbols(&args.common, filter, Some(&args.name))?;
            symbols.truncate(args.common.max_matches);
            Ok((symbols, args.common.json, true))
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
            Ok((symbols, args.common.json, true))
        }
        Command::References(args) => {
            let mut symbols = collect_references(&args.common, &args.name)?;
            symbols.truncate(args.common.max_matches);
            Ok((symbols, args.common.json, false))
        }
        Command::Callers(args) => {
            let mut symbols = collect_callers(&args.common, &args.name)?;
            symbols.truncate(args.common.max_matches);
            Ok((symbols, args.common.json, true))
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
            Ok((symbols, args.common.json, true))
        }
    }
}

fn collect_symbols(
    common: &CommonArgs,
    kind: Option<SymbolKindFilter>,
    wanted: Option<&str>,
) -> Result<Vec<Symbol>, AppError> {
    if common.backend == Backend::Lsp {
        return Err(AppError::Backend(anyhow::anyhow!(
            "LSP backend is not implemented yet; use --backend auto or --backend tree-sitter"
        )));
    }
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
        match language_for_path(&file) {
            Some(Language::Python) => {
                out.extend(crate::python::symbols(&file, &text, kind, wanted))
            }
            Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
                out.extend(
                    crate::cfamily::symbols(&file, &text, common.backend, kind, wanted)
                        .map_err(AppError::Backend)?,
                );
            }
            _ => {}
        }
        if out.len() >= common.max_matches {
            break;
        }
    }
    Ok(out)
}

fn collect_references(common: &CommonArgs, wanted: &str) -> Result<Vec<Symbol>, AppError> {
    if common.backend == Backend::Lsp {
        return Err(AppError::Backend(anyhow::anyhow!(
            "LSP references are not implemented yet; use --backend auto or --backend tree-sitter"
        )));
    }
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
        match language_for_path(&file) {
            Some(Language::Python) => out.extend(crate::python::references(
                &file,
                &text,
                wanted,
                common.max_matches - out.len(),
            )),
            Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
                out.extend(crate::cfamily::references(
                    &file,
                    &text,
                    wanted,
                    common.max_matches - out.len(),
                ));
            }
            _ => {}
        }
        if out.len() >= common.max_matches {
            break;
        }
    }
    Ok(out)
}

fn collect_callers(common: &CommonArgs, wanted: &str) -> Result<Vec<Symbol>, AppError> {
    if common.backend == Backend::Lsp {
        return Err(AppError::Backend(anyhow::anyhow!(
            "LSP callers are not implemented yet; use --backend auto or --backend tree-sitter"
        )));
    }
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
        match language_for_path(&file) {
            Some(Language::Python) => out.extend(crate::python::callers(
                &file,
                &text,
                wanted,
                common.max_matches - out.len(),
            )),
            Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
                out.extend(
                    crate::cfamily::callers(
                        &file,
                        &text,
                        common.backend,
                        wanted,
                        common.max_matches - out.len(),
                    )
                    .map_err(AppError::Backend)?,
                );
            }
            _ => {}
        }
        if out.len() >= common.max_matches {
            break;
        }
    }
    Ok(out)
}

enum AppError {
    Config(anyhow::Error),
    Backend(anyhow::Error),
}
