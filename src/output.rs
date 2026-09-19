use std::borrow::Cow;
use std::path::Path;

use serde::{Serializer, ser::Error};

use crate::model::Symbol;

fn normalize_path(path: &str) -> Cow<'_, str> {
    #[cfg(windows)]
    {
        if let Some(path) = path.strip_prefix(r"\\?\UNC\") {
            return Cow::Owned(format!("//{}", path.replace('\\', "/")));
        }
        let path = path.strip_prefix(r"\\?\").unwrap_or(path);
        Cow::Owned(path.replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        Cow::Borrowed(path)
    }
}

pub(crate) fn serialize_path<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    let path = path
        .to_str()
        .ok_or_else(|| S::Error::custom("path contains invalid UTF-8 characters"))?;
    serializer.serialize_str(&normalize_path(path))
}

pub fn json(symbols: &[Symbol]) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(symbols)?)
}

pub fn list_plain(symbols: &[Symbol]) -> String {
    symbols
        .iter()
        .map(|symbol| {
            format!(
                "{}:{}-{} ({}, {}, {}, {})",
                normalize_path(&symbol.path.to_string_lossy()),
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
                normalize_path(&symbol.path.to_string_lossy()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Language, SymbolKind};

    fn assert_output_path(input: &str, expected: &str) {
        let symbol = Symbol::new(
            input.into(),
            Language::Python,
            "tree-sitter",
            SymbolKind::Function,
            "example",
            "example",
            1,
            2,
            "def example():\n    return r'C:\\data'\n",
        );
        let symbols = [symbol];
        let location = format!("{expected}:1-2 (python, tree-sitter, function, example)");
        assert_eq!(list_plain(&symbols), location);
        assert_eq!(
            with_source(&symbols),
            format!("// {location}\n{}\n", symbols[0].source.trim_end())
        );
        let value: serde_json::Value = serde_json::from_str(&json(&symbols).unwrap()).unwrap();
        assert_eq!(value[0]["path"], expected);
        assert_eq!(value[0]["source"], symbols[0].source);
        assert_eq!(symbols[0].path, Path::new(input));
    }

    #[cfg(windows)]
    #[test]
    fn windows_output_normalizes_drive_and_unc_paths() {
        for (input, expected) in [
            (r"\\?\H:\directory\file.py", "H:/directory/file.py"),
            (r"H:\directory\file.py", "H:/directory/file.py"),
            (r"src\file.py", "src/file.py"),
            (r"\\?\UNC\server\share\file.py", "//server/share/file.py"),
            (r"\\server\share\file.py", "//server/share/file.py"),
            (r"\\?\H:\my files\café.py", "H:/my files/café.py"),
            ("H:/directory/file.py", "H:/directory/file.py"),
        ] {
            assert_output_path(input, expected);
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_output_preserves_literal_backslashes() {
        assert_output_path(
            r"/tmp/directory/file\name.py",
            r"/tmp/directory/file\name.py",
        );
    }
}
