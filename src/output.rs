use crate::model::Symbol;

pub fn json(symbols: &[Symbol]) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(symbols)?)
}

pub fn list_plain(symbols: &[Symbol]) -> String {
    symbols
        .iter()
        .map(|symbol| {
            format!(
                "{}:{}-{} ({}, {}, {}, {})",
                symbol.path.display(),
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
                symbol.path.display(),
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
