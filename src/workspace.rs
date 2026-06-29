use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::model::{Language, LanguageFilter};

pub const PY_EXTS: &[&str] = &["py"];
pub const CPP_EXTS: &[&str] = &[
    "c", "cc", "cpp", "cxx", "h", "hh", "hpp", "hxx", "ipp", "tpp", "inl",
];
pub const CUDA_EXTS: &[&str] = &["cu", "cuh"];
pub const HIP_EXTS: &[&str] = &["hip"];

pub fn language_for_path(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    if PY_EXTS.contains(&ext.as_str()) {
        Some(Language::Python)
    } else if CUDA_EXTS.contains(&ext.as_str()) {
        Some(Language::Cuda)
    } else if HIP_EXTS.contains(&ext.as_str()) {
        Some(Language::Hip)
    } else if ext == "c" {
        Some(Language::C)
    } else if CPP_EXTS.contains(&ext.as_str()) {
        Some(Language::Cpp)
    } else {
        None
    }
}

pub fn language_allowed(language: Language, filter: Option<LanguageFilter>) -> bool {
    match filter {
        None => true,
        Some(LanguageFilter::Python) => language == Language::Python,
        Some(LanguageFilter::C) => language == Language::C,
        Some(LanguageFilter::Cpp | LanguageFilter::Cxx) => language == Language::Cpp,
        Some(LanguageFilter::Cuda) => language == Language::Cuda,
        Some(LanguageFilter::Hip) => language == Language::Hip,
    }
}

pub fn source_files(path: &Path, filter: Option<LanguageFilter>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if path.is_file() {
        if let Some(language) = language_for_path(path) {
            if language_allowed(language, filter) {
                files.push(path.to_path_buf());
            }
        }
        return files;
    }

    let mut builder = WalkBuilder::new(path);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                ".git"
                    | ".hg"
                    | ".svn"
                    | ".mypy_cache"
                    | ".pytest_cache"
                    | ".ruff_cache"
                    | "__pycache__"
                    | "build"
                    | "dist"
                    | "node_modules"
                    | "venv"
                    | ".venv"
                    | "target"
            )
        });

    for entry in builder.build().filter_map(Result::ok) {
        let file_path = entry.path();
        if !file_path.is_file() {
            continue;
        }
        if let Some(language) = language_for_path(file_path) {
            if language_allowed(language, filter) {
                files.push(file_path.to_path_buf());
            }
        }
    }
    files.sort();
    files
}

pub fn read_text(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

pub fn line_slice(text: &str, start_line: usize, end_line: usize) -> String {
    text.lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let line_no = idx + 1;
            (start_line <= line_no && line_no <= end_line).then_some(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

pub fn line_for_byte(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}
