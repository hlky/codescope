use std::fmt;
use std::path::PathBuf;

use clap::ValueEnum;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum LanguageFilter {
    Python,
    Rust,
    C,
    Cpp,
    #[value(alias = "c++")]
    Cxx,
    Cuda,
    Hip,
    Cmake,
    Markdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    Rust,
    C,
    Cpp,
    Cuda,
    Hip,
    Cmake,
    Markdown,
    Text,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Python => "python",
            Self::Rust => "rust",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Cuda => "cuda",
            Self::Hip => "hip",
            Self::Cmake => "cmake",
            Self::Markdown => "markdown",
            Self::Text => "text",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SymbolKindFilter {
    Function,
    Class,
    Struct,
    Enum,
    Variable,
    Target,
    Block,
    Heading,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Class,
    Struct,
    Enum,
    Variable,
    Target,
    Block,
    Heading,
    Reference,
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Function => "function",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Variable => "variable",
            Self::Target => "target",
            Self::Block => "block",
            Self::Heading => "heading",
            Self::Reference => "reference",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Backend {
    Auto,
    Lsp,
    #[value(name = "tree-sitter")]
    TreeSitter,
    Lexical,
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Auto => "auto",
            Self::Lsp => "lsp",
            Self::TreeSitter => "tree-sitter",
            Self::Lexical => "lexical",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Symbol {
    #[serde(serialize_with = "crate::path_display::serialize")]
    pub path: PathBuf,
    pub language: Language,
    pub backend: String,
    pub kind: SymbolKind,
    pub name: String,
    pub qualified_name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

impl Symbol {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: PathBuf,
        language: Language,
        backend: impl Into<String>,
        kind: SymbolKind,
        name: impl Into<String>,
        qualified_name: impl Into<String>,
        start_line: usize,
        end_line: usize,
        source: impl Into<String>,
    ) -> Self {
        Self {
            path,
            language,
            backend: backend.into(),
            kind,
            name: name.into(),
            qualified_name: qualified_name.into(),
            start_line,
            end_line,
            source: source.into(),
            detail: String::new(),
        }
    }
}

pub fn kind_matches(filter: Option<SymbolKindFilter>, kind: SymbolKind) -> bool {
    match filter {
        None | Some(SymbolKindFilter::All) => true,
        Some(SymbolKindFilter::Function) => kind == SymbolKind::Function,
        Some(SymbolKindFilter::Class) => kind == SymbolKind::Class,
        Some(SymbolKindFilter::Struct) => kind == SymbolKind::Struct,
        Some(SymbolKindFilter::Enum) => kind == SymbolKind::Enum,
        Some(SymbolKindFilter::Variable) => kind == SymbolKind::Variable,
        Some(SymbolKindFilter::Target) => kind == SymbolKind::Target,
        Some(SymbolKindFilter::Block) => kind == SymbolKind::Block,
        Some(SymbolKindFilter::Heading) => kind == SymbolKind::Heading,
    }
}

pub fn name_matches(wanted: &str, short: &str, qualified: &str, sep: &str) -> bool {
    let normalized = wanted.trim();
    if normalized.is_empty() {
        return false;
    }
    if normalized == short || normalized == qualified {
        return true;
    }
    if sep == "::" {
        let cpp = normalized.replace('.', "::");
        qualified.ends_with(&format!("::{normalized}")) || qualified.ends_with(&cpp)
    } else {
        qualified.ends_with(&format!(".{normalized}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_name_matching_accepts_suffix_qualification() {
        assert!(name_matches("method", "method", "Outer.Inner.method", "."));
        assert!(name_matches(
            "Inner.method",
            "method",
            "Outer.Inner.method",
            "."
        ));
        assert!(!name_matches("other", "method", "Outer.Inner.method", "."));
    }

    #[test]
    fn c_family_name_matching_accepts_dot_or_colon_qualification() {
        assert!(name_matches(
            "Namespace::Class::method",
            "method",
            "Namespace::Class::method",
            "::"
        ));
        assert!(name_matches(
            "Class.method",
            "method",
            "Namespace::Class::method",
            "::"
        ));
        assert!(name_matches(
            "Class::method",
            "method",
            "Namespace::Class::method",
            "::"
        ));
        assert!(!name_matches(
            "Other::method",
            "method",
            "Namespace::Class::method",
            "::"
        ));
    }

    #[test]
    fn kind_filter_matches_all_by_default() {
        assert!(kind_matches(None, SymbolKind::Function));
        assert!(kind_matches(
            Some(SymbolKindFilter::All),
            SymbolKind::Variable
        ));
        assert!(!kind_matches(
            Some(SymbolKindFilter::Class),
            SymbolKind::Struct
        ));
    }
}
