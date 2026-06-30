use crate::model::{NavigationRecord, RelatedTestRecord, Symbol};
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

pub fn navigation_json(records: &[NavigationRecord]) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(records)?)
}

pub fn navigation_plain(records: &[NavigationRecord]) -> String {
    records
        .iter()
        .map(|record| {
            let detail = if record.detail.is_empty() {
                String::new()
            } else {
                format!("\n{}", record.detail.trim_end())
            };
            format!(
                "// {}:{}:{}-{}:{} ({}, {}, {}, {}){}\n{}\n",
                display_path(&record.path),
                record.start_line,
                record.start_column,
                record.end_line,
                record.end_column,
                record.language,
                record.backend,
                record.kind,
                record.qualified_name,
                detail,
                record.source.trim_end()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn related_tests_json(records: &[RelatedTestRecord]) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(records)?)
}

pub fn related_tests_plain(records: &[RelatedTestRecord]) -> String {
    records
        .iter()
        .map(|record| {
            format!(
                "// {}:{}-{} ({}, {}, score {}, {})\n// reason: {}\n{}\n",
                display_path(&record.path),
                record.start_line,
                record.end_line,
                record.language,
                record.backend,
                record.score,
                record.qualified_name,
                record.reason,
                record.source.trim_end()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
