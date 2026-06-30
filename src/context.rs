use crate::model::{Language, Symbol};
use crate::workspace::read_text;

pub fn add_import_context(symbols: Vec<Symbol>) -> Vec<Symbol> {
    symbols
        .into_iter()
        .map(|mut symbol| {
            let Some(text) = read_text(&symbol.path) else {
                return symbol;
            };
            let context = import_context(symbol.language, &text);
            if !context.is_empty() {
                symbol.source = format!("{}\n{}", context.trim_end(), symbol.source);
                symbol.detail = "context".to_string();
            }
            symbol
        })
        .collect()
}

fn import_context(language: Language, text: &str) -> String {
    import_context_range(language, text)
        .map(|(_, _, source)| source)
        .unwrap_or_default()
}

pub fn import_context_range(language: Language, text: &str) -> Option<(usize, usize, String)> {
    let lines: Vec<(usize, &str)> = match language {
        Language::Python => text
            .lines()
            .enumerate()
            .filter(|(_, line)| {
                let stripped = line.trim_start();
                stripped.starts_with("import ") || stripped.starts_with("from ")
            })
            .map(|(idx, line)| (idx + 1, line))
            .collect(),
        Language::C | Language::Cpp | Language::Cuda | Language::Hip => text
            .lines()
            .enumerate()
            .filter(|(_, line)| {
                let stripped = line.trim_start();
                stripped.starts_with("#include") || stripped.starts_with("#define")
            })
            .map(|(idx, line)| (idx + 1, line))
            .collect(),
        Language::Rust | Language::Cmake | Language::Markdown | Language::Text => Vec::new(),
    };
    let first = lines.first()?.0;
    let last = lines.last()?.0;
    let source = lines
        .into_iter()
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n");
    Some((first, last, source))
}
