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
    match language {
        Language::Python => text
            .lines()
            .filter(|line| {
                let stripped = line.trim_start();
                stripped.starts_with("import ") || stripped.starts_with("from ")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Language::C | Language::Cpp | Language::Cuda | Language::Hip => text
            .lines()
            .filter(|line| {
                let stripped = line.trim_start();
                stripped.starts_with("#include") || stripped.starts_with("#define")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Language::Markdown | Language::Text => String::new(),
    }
}
