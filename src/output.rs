use crate::model::Symbol;
use crate::path_display::display_path;

pub fn json(symbols: &[Symbol]) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(symbols)?)
}

pub fn list_plain(symbols: &[Symbol]) -> String {
    symbols
        .iter()
        .map(|symbol| {
            format!(
                "{}:{}-{} ({}, {}, {}, {})",
                display_path(&symbol.path),
                symbol.start_line,
                symbol.end_line,
                symbol.language,
                symbol.backend,
                symbol.kind,
                symbol.qualified_name
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn with_source(symbols: &[Symbol]) -> String {
    symbols
        .iter()
        .map(|symbol| {
            format!(
                "// {}:{}-{} ({}, {}, {}, {})\n{}\n",
                display_path(&symbol.path),
                symbol.start_line,
                symbol.end_line,
                symbol.language,
                symbol.backend,
                symbol.kind,
                symbol.qualified_name,
                symbol.source.trim_end()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
