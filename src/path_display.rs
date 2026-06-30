use std::path::Path;

use serde::Serializer;

pub fn display_path(path: &Path) -> String {
    normalize_path_text(&path.to_string_lossy())
}

pub fn serialize<S>(path: &Path, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&display_path(path))
}

fn normalize_path_text(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_path_removes_windows_verbatim_prefix_and_normalizes_separators() {
        assert_eq!(
            normalize_path_text(r"\\?\H:\codescope\src\main.rs"),
            "H:/codescope/src/main.rs"
        );
    }

    #[test]
    fn display_path_preserves_unc_paths_without_verbatim_prefix() {
        assert_eq!(
            normalize_path_text(r"\\?\UNC\server\share\file.py"),
            "//server/share/file.py"
        );
    }
}
