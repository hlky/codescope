use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use ignore::WalkBuilder;
use serde::Serialize;

use crate::model::{Language, SymbolKindFilter};
use crate::path_display::display_path;
use crate::workspace::{
    DEFAULT_IGNORED_DIRS, is_default_ignored_dir, language_for_path, read_text, source_files,
};

const COMMON_FILES: &[(&str, &str)] = &[
    ("Cargo.toml", "cargo"),
    ("pyproject.toml", "python"),
    ("setup.py", "python"),
    ("CMakeLists.txt", "cmake"),
    ("compile_commands.json", "compile_commands"),
    ("package.json", "node"),
];

const TOOLS: &[&str] = &[
    "clangd", "cargo", "ruff", "mypy", "pyright", "pytest", "cmake",
];

#[derive(Clone, Debug)]
pub struct WorkspaceMapOptions {
    pub path: PathBuf,
    pub max_targets: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceMap {
    pub root: String,
    pub languages: Vec<LanguageCount>,
    pub roots: Vec<WorkspaceRoot>,
    pub build_systems: Vec<BuildSystem>,
    pub targets: Vec<WorkspaceTarget>,
    pub test_roots: Vec<String>,
    pub doc_roots: Vec<String>,
    pub tools: Vec<WorkspaceTool>,
    pub git: GitSummary,
    pub ignored_patterns: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LanguageCount {
    pub language: String,
    pub files: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceRoot {
    pub kind: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BuildSystem {
    pub kind: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceTarget {
    pub name: String,
    pub kind: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceTool {
    pub name: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GitSummary {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub changed_files: usize,
}

pub fn collect(options: &WorkspaceMapOptions) -> anyhow::Result<WorkspaceMap> {
    let root = options
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", options.path.display()))?;
    let root = if root.is_file() {
        root.parent()
            .map(Path::to_path_buf)
            .unwrap_or(root)
            .canonicalize()
            .with_context(|| {
                format!(
                    "failed to resolve parent for --path {}",
                    options.path.display()
                )
            })?
    } else {
        root
    };

    let mut notes = Vec::new();
    let source_files = source_files(&root, None);
    let languages = language_counts(&source_files);
    let all_files = project_files(&root);
    let build_systems = build_systems(&root, &all_files);
    let roots = roots(&root, &source_files);
    let test_roots = convention_roots(&root, &all_files, is_test_path);
    let doc_roots = convention_roots(&root, &all_files, is_doc_path);
    let targets = cmake_targets(&root, &source_files, options.max_targets, &mut notes);

    Ok(WorkspaceMap {
        root: display_path(&root),
        languages,
        roots,
        build_systems,
        targets,
        test_roots,
        doc_roots,
        tools: tools(),
        git: git_summary(&root, &mut notes),
        ignored_patterns: DEFAULT_IGNORED_DIRS
            .iter()
            .map(|value| value.to_string())
            .collect(),
        notes,
    })
}

pub fn render_plain(map: &WorkspaceMap) -> String {
    let mut out = String::new();
    out.push_str("# Workspace Map\n\n");
    out.push_str(&format!("root: {}\n", map.root));
    push_list(
        &mut out,
        "languages",
        map.languages
            .iter()
            .map(|entry| format!("{} ({})", entry.language, entry.files)),
    );
    push_list(
        &mut out,
        "roots",
        map.roots
            .iter()
            .map(|entry| format!("{}: {}", entry.kind, entry.path)),
    );
    push_list(
        &mut out,
        "tests",
        map.test_roots.iter().map(std::string::String::as_str),
    );
    push_list(
        &mut out,
        "docs",
        map.doc_roots.iter().map(std::string::String::as_str),
    );
    push_list(
        &mut out,
        "build",
        map.build_systems
            .iter()
            .map(|entry| format!("{}: {}", entry.kind, entry.path)),
    );
    push_list(
        &mut out,
        "targets",
        map.targets
            .iter()
            .map(|entry| format!("{} ({}, {})", entry.name, entry.kind, entry.path)),
    );
    push_list(
        &mut out,
        "tools",
        map.tools.iter().map(|tool| {
            if tool.available {
                format!("{}: available", tool.name)
            } else {
                format!("{}: unavailable", tool.name)
            }
        }),
    );
    out.push_str("\ngit:\n");
    if map.git.available {
        let branch = map.git.branch.as_deref().unwrap_or("unknown");
        out.push_str(&format!(
            "- branch: {branch}, changed: {}, staged: {}, unstaged: {}, untracked: {}, ahead: {}, behind: {}\n",
            map.git.changed_files,
            map.git.staged,
            map.git.unstaged,
            map.git.untracked,
            map.git.ahead,
            map.git.behind
        ));
    } else {
        out.push_str("- unavailable\n");
    }
    if !map.notes.is_empty() {
        push_list(
            &mut out,
            "notes",
            map.notes.iter().map(std::string::String::as_str),
        );
    }
    out
}

fn language_counts(files: &[PathBuf]) -> Vec<LanguageCount> {
    let mut counts = BTreeMap::<String, usize>::new();
    for file in files {
        if let Some(language) = language_for_path(file) {
            *counts.entry(language.to_string()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .map(|(language, files)| LanguageCount { language, files })
        .collect()
}

fn project_files(root: &Path) -> Vec<PathBuf> {
    let root = root.to_path_buf();
    let mut builder = WalkBuilder::new(&root);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(move |entry| {
            if entry.path() == root {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            !is_default_ignored_dir(&name)
        });

    let mut files = builder
        .build()
        .filter_map(Result::ok)
        .map(|entry| entry.into_path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn build_systems(root: &Path, files: &[PathBuf]) -> Vec<BuildSystem> {
    let mut out = Vec::new();
    for file in files {
        let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some((_, kind)) = COMMON_FILES
            .iter()
            .find(|(common, _)| name.eq_ignore_ascii_case(common))
        {
            out.push(BuildSystem {
                kind: (*kind).to_string(),
                path: relative(root, file),
            });
        }
    }
    out.sort_by(|left, right| left.path.cmp(&right.path).then(left.kind.cmp(&right.kind)));
    out.dedup_by(|left, right| left.path == right.path && left.kind == right.kind);
    out
}

fn roots(root: &Path, files: &[PathBuf]) -> Vec<WorkspaceRoot> {
    let mut roots = BTreeSet::<(String, String)>::new();
    for file in files {
        let Some(parent) = file.parent() else {
            continue;
        };
        if parent == root {
            if let Some(language) = language_for_path(file) {
                roots.insert((format!("{language}-source"), ".".to_string()));
            }
            continue;
        }
        if let Some(kind) = root_kind(parent) {
            roots.insert((kind.to_string(), relative(root, parent)));
        }
    }
    roots
        .into_iter()
        .map(|(kind, path)| WorkspaceRoot { kind, path })
        .collect()
}

fn root_kind(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    match name.as_str() {
        "src" | "source" | "sources" | "lib" | "app" => Some("source"),
        "include" | "includes" => Some("include"),
        _ => None,
    }
}

fn convention_roots(
    root: &Path,
    files: &[PathBuf],
    predicate: impl Fn(&Path) -> bool,
) -> Vec<String> {
    let mut out = BTreeSet::<String>::new();
    for file in files {
        if !predicate(file) {
            continue;
        }
        if let Some(parent) = convention_parent(root, file) {
            out.insert(relative(root, &parent));
        }
    }
    out.into_iter().collect()
}

fn convention_parent(root: &Path, file: &Path) -> Option<PathBuf> {
    let relative = file.strip_prefix(root).ok()?;
    for ancestor in relative.ancestors().skip(1) {
        let Some(name) = ancestor.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "tests" | "test" | "__tests__" | "docs" | "doc" | "documentation"
        ) {
            return Some(root.join(ancestor));
        }
    }
    file.parent().map(Path::to_path_buf)
}

fn cmake_targets(
    root: &Path,
    files: &[PathBuf],
    max_targets: usize,
    notes: &mut Vec<String>,
) -> Vec<WorkspaceTarget> {
    let mut out = Vec::new();
    for file in files
        .iter()
        .filter(|file| language_for_path(file) == Some(Language::Cmake))
    {
        let Some(text) = read_text(file) else {
            notes.push(format!("failed to read {}", relative(root, file)));
            continue;
        };
        for symbol in crate::cmake::symbols(file, &text, Some(SymbolKindFilter::Target), None) {
            out.push(WorkspaceTarget {
                name: symbol.name,
                kind: symbol.detail,
                path: relative(root, file),
            });
            if out.len() >= max_targets {
                notes.push(format!("target list truncated at {max_targets} entries"));
                return out;
            }
        }
    }
    out
}

fn tools() -> Vec<WorkspaceTool> {
    TOOLS
        .iter()
        .map(|name| match which::which(name) {
            Ok(path) => WorkspaceTool {
                name: (*name).to_string(),
                available: true,
                path: Some(display_path(&path)),
            },
            Err(_) => WorkspaceTool {
                name: (*name).to_string(),
                available: false,
                path: None,
            },
        })
        .collect()
}

fn git_summary(root: &Path, notes: &mut Vec<String>) -> GitSummary {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain", "--branch"])
        .output();
    let Ok(output) = output else {
        notes.push("git status failed to run".to_string());
        return empty_git();
    };
    if !output.status.success() {
        return empty_git();
    }
    parse_git_status(&String::from_utf8_lossy(&output.stdout))
}

fn parse_git_status(text: &str) -> GitSummary {
    let mut summary = GitSummary {
        available: true,
        branch: None,
        ahead: 0,
        behind: 0,
        staged: 0,
        unstaged: 0,
        untracked: 0,
        changed_files: 0,
    };
    for line in text.lines() {
        if let Some(branch) = line.strip_prefix("## ") {
            parse_branch(branch, &mut summary);
            continue;
        }
        if line.len() < 2 {
            continue;
        }
        summary.changed_files += 1;
        let bytes = line.as_bytes();
        if bytes[0] == b'?' && bytes[1] == b'?' {
            summary.untracked += 1;
            continue;
        }
        if bytes[0] != b' ' {
            summary.staged += 1;
        }
        if bytes[1] != b' ' {
            summary.unstaged += 1;
        }
    }
    summary
}

fn parse_branch(line: &str, summary: &mut GitSummary) {
    let branch = line.split("...").next().unwrap_or(line);
    summary.branch = Some(branch.to_string());
    if let Some(status) = line
        .split('[')
        .nth(1)
        .and_then(|value| value.strip_suffix(']'))
    {
        for part in status.split(", ") {
            if let Some(value) = part.strip_prefix("ahead ") {
                summary.ahead = value.parse().unwrap_or(0);
            } else if let Some(value) = part.strip_prefix("behind ") {
                summary.behind = value.parse().unwrap_or(0);
            }
        }
    }
}

fn empty_git() -> GitSummary {
    GitSummary {
        available: false,
        branch: None,
        ahead: 0,
        behind: 0,
        staged: 0,
        unstaged: 0,
        untracked: 0,
        changed_files: 0,
    }
}

fn is_test_path(path: &Path) -> bool {
    path.components().any(|part| {
        let value = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        matches!(value.as_str(), "tests" | "test" | "__tests__")
    }) || path.file_stem().is_some_and(|stem| {
        let value = stem.to_string_lossy().to_ascii_lowercase();
        value.starts_with("test_") || value.ends_with("_test") || value.ends_with("_spec")
    })
}

fn is_doc_path(path: &Path) -> bool {
    path.components().any(|part| {
        let value = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        matches!(value.as_str(), "docs" | "doc" | "documentation")
    }) || path.file_name().is_some_and(|name| {
        let value = name.to_string_lossy().to_ascii_lowercase();
        matches!(
            value.as_str(),
            "readme.md" | "changelog.md" | "contributing.md" | "security.md"
        )
    })
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .filter(|value| !value.as_os_str().is_empty())
        .map(|value| value.display().to_string().replace('\\', "/"))
        .unwrap_or_else(|| ".".to_string())
}

fn push_list(out: &mut String, title: &str, values: impl IntoIterator<Item = impl AsRef<str>>) {
    out.push('\n');
    out.push_str(title);
    out.push_str(":\n");
    let mut wrote = false;
    for value in values {
        wrote = true;
        out.push_str("- ");
        out.push_str(value.as_ref());
        out.push('\n');
    }
    if !wrote {
        out.push_str("- none\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_porcelain_counts() {
        let summary = parse_git_status(
            "## main...origin/main [ahead 1, behind 2]\n M src/lib.rs\nA  new.rs\n?? note.md\n",
        );
        assert!(summary.available);
        assert_eq!(summary.branch.as_deref(), Some("main"));
        assert_eq!(summary.ahead, 1);
        assert_eq!(summary.behind, 2);
        assert_eq!(summary.changed_files, 3);
        assert_eq!(summary.staged, 1);
        assert_eq!(summary.unstaged, 1);
        assert_eq!(summary.untracked, 1);
    }
}
