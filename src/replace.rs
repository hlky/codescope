use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use regex::{Captures, Regex};

use crate::model::{LanguageFilter, SymbolKindFilter};
use crate::workspace::{read_text, source_files};

#[derive(Clone, Debug)]
pub struct ReplaceOptions {
    pub path: PathBuf,
    pub lang: Option<LanguageFilter>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub max_files: usize,
    pub apply: bool,
    pub confirm: bool,
}

#[derive(Clone, Debug)]
pub enum Pattern {
    Literal(String),
    Regex(String),
    Identifier(String),
}

#[derive(Clone, Debug)]
pub struct Replacement {
    pub pattern: Pattern,
    pub replacement: String,
    pub label: &'static str,
    pub expand_captures: bool,
}

#[derive(Clone, Debug)]
pub struct ReplaceSummary {
    pub files_scanned: usize,
    pub files_changed: usize,
    pub replacements: usize,
    pub applied: bool,
    pub diffs: Vec<FileDiff>,
}

#[derive(Clone, Debug)]
pub struct FileDiff {
    pub path: PathBuf,
    pub replacements: usize,
    pub diff: String,
}

pub fn run(options: &ReplaceOptions, replacement: &Replacement) -> anyhow::Result<ReplaceSummary> {
    validate_options(options)?;
    let search_root = options
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", options.path.display()))?;
    if options.apply && options.confirm {
        ensure_clean_git_worktree(&search_root)?;
    }

    let regex = replacement_regex(&replacement.pattern)?;
    let replacement_text = replacement.replacement.as_str();
    let mut files_scanned = 0;
    let mut files_changed = 0;
    let mut replacements = 0;
    let mut diffs = Vec::new();

    for file in source_files(&search_root, options.lang) {
        if !path_allowed(&search_root, &file, &options.include, &options.exclude) {
            continue;
        }
        files_scanned += 1;
        let Some(old_text) = read_text(&file) else {
            continue;
        };
        let (new_text, count) = apply_replacement(&old_text, replacement, &regex, replacement_text);
        if count == 0 {
            continue;
        }
        if files_changed >= options.max_files {
            bail!(
                "replacement would modify more than --max-files {} files",
                options.max_files
            );
        }
        files_changed += 1;
        replacements += count;
        let diff = unified_diff(&file, &old_text, &new_text);
        diffs.push(FileDiff {
            path: file.clone(),
            replacements: count,
            diff,
        });
        if options.apply {
            fs::write(&file, new_text)
                .with_context(|| format!("failed to write {}", file.display()))?;
        }
    }

    Ok(ReplaceSummary {
        files_scanned,
        files_changed,
        replacements,
        applied: options.apply,
        diffs,
    })
}

pub fn validate_symbol_request(
    name: &str,
    replacement: &str,
    kind: Option<SymbolKindFilter>,
) -> anyhow::Result<()> {
    validate_qualified_identifier(name, "--name/--from")?;
    validate_qualified_identifier(replacement, "--with/--to")?;
    if matches!(kind, Some(SymbolKindFilter::All)) {
        bail!("symbol replacement --kind must be a concrete kind, not all");
    }
    Ok(())
}

pub fn render(summary: &ReplaceSummary) -> String {
    let mode = if summary.applied {
        "applied"
    } else {
        "preview"
    };
    let mut out = format!(
        "{mode}: {} replacements across {} files ({} files scanned)",
        summary.replacements, summary.files_changed, summary.files_scanned
    );
    for diff in &summary.diffs {
        out.push_str(&format!(
            "\n\n# {} ({} replacements)\n{}",
            diff.path.display(),
            diff.replacements,
            diff.diff
        ));
    }
    out
}

fn validate_options(options: &ReplaceOptions) -> anyhow::Result<()> {
    if options.max_files == 0 {
        bail!("--max-files must be greater than zero");
    }
    if options.confirm && !options.apply {
        bail!("--confirm only has an effect with --apply");
    }
    Ok(())
}

fn validate_identifier(value: &str, label: &str) -> anyhow::Result<()> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        bail!("{label} cannot be empty");
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        bail!("{label} must start with an ASCII identifier character");
    }
    if !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        bail!("{label} must be a single ASCII identifier");
    }
    Ok(())
}

pub fn validate_qualified_identifier(value: &str, label: &str) -> anyhow::Result<()> {
    let normalized = value.replace("::", ".");
    if normalized.split('.').any(str::is_empty) {
        bail!("{label} must be a valid qualified identifier");
    }
    for part in normalized.split('.') {
        validate_identifier(part, label)?;
    }
    Ok(())
}

fn replacement_regex(pattern: &Pattern) -> anyhow::Result<Regex> {
    match pattern {
        Pattern::Literal(value) => Ok(Regex::new(&regex::escape(value))?),
        Pattern::Regex(value) => Ok(Regex::new(value)?),
        Pattern::Identifier(value) => {
            validate_qualified_identifier(value, "symbol name")?;
            let escaped = regex::escape(value);
            if value.contains("::") {
                Ok(Regex::new(&format!(
                    r"(^|[^A-Za-z0-9_:])({})([^A-Za-z0-9_:]|$)",
                    escaped
                ))?)
            } else if value.contains('.') {
                Ok(Regex::new(&format!(
                    r"(^|[^A-Za-z0-9_\.])({})([^A-Za-z0-9_\.]|$)",
                    escaped
                ))?)
            } else {
                Ok(Regex::new(&format!(
                    r"(^|[^A-Za-z0-9_])({})([^A-Za-z0-9_]|$)",
                    escaped
                ))?)
            }
        }
    }
}

fn replace_text(
    text: &str,
    regex: &Regex,
    replacement: &str,
    expand_captures: bool,
) -> (String, usize) {
    let mut count = 0;
    let replaced = regex.replace_all(text, |captures: &Captures<'_>| {
        count += 1;
        if captures.len() == 4 {
            format!("{}{}{}", &captures[1], replacement, &captures[3])
        } else if expand_captures {
            let mut expanded = String::new();
            captures.expand(replacement, &mut expanded);
            expanded
        } else {
            replacement.to_string()
        }
    });
    (replaced.into_owned(), count)
}

fn apply_replacement(
    text: &str,
    replacement: &Replacement,
    regex: &Regex,
    replacement_text: &str,
) -> (String, usize) {
    match &replacement.pattern {
        Pattern::Identifier(identifier) => {
            replace_identifier(text, identifier, &replacement.replacement)
        }
        Pattern::Literal(_) | Pattern::Regex(_) => {
            replace_text(text, regex, replacement_text, replacement.expand_captures)
        }
    }
}

fn replace_identifier(text: &str, identifier: &str, replacement: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut count = 0;
    while let Some(relative_start) = text[index..].find(identifier) {
        let start = index + relative_start;
        let end = start + identifier.len();
        if identifier_boundary_before(text, start, identifier)
            && identifier_boundary_after(text, end, identifier)
        {
            out.push_str(&text[index..start]);
            out.push_str(replacement);
            index = end;
            count += 1;
        } else {
            let next = text[start..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(identifier.len());
            out.push_str(&text[index..start + next]);
            index = start + next;
        }
    }
    out.push_str(&text[index..]);
    (out, count)
}

fn identifier_boundary_before(text: &str, byte_index: usize, identifier: &str) -> bool {
    boundary_char_allowed(text[..byte_index].chars().next_back(), identifier)
}

fn identifier_boundary_after(text: &str, byte_index: usize, identifier: &str) -> bool {
    boundary_char_allowed(text[byte_index..].chars().next(), identifier)
}

fn boundary_char_allowed(neighbor: Option<char>, identifier: &str) -> bool {
    let Some(ch) = neighbor else {
        return true;
    };
    if ch == '_' || ch.is_ascii_alphanumeric() {
        return false;
    }
    if identifier.contains("::") {
        ch != ':'
    } else if identifier.contains('.') {
        ch != '.'
    } else {
        true
    }
}

fn path_allowed(root: &Path, path: &Path, include: &[String], exclude: &[String]) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let normalized = rel.to_string_lossy().replace('\\', "/");
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let included = include.is_empty()
        || include
            .iter()
            .any(|pattern| glob_matches(pattern, &normalized) || glob_matches(pattern, &file_name));
    let excluded = exclude
        .iter()
        .any(|pattern| glob_matches(pattern, &normalized) || glob_matches(pattern, &file_name));
    included && !excluded
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let mut regex = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '/' | '\\' => regex.push('/'),
            _ => regex.push_str(&regex::escape(&ch.to_string())),
        }
    }
    regex.push('$');
    Regex::new(&regex).is_ok_and(|compiled| compiled.is_match(value))
}

fn unified_diff(path: &Path, old_text: &str, new_text: &str) -> String {
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();
    let max = old_lines.len().max(new_lines.len());
    let mut out = format!("--- {}\n+++ {}\n", path.display(), path.display());
    for index in 0..max {
        match (old_lines.get(index), new_lines.get(index)) {
            (Some(old), Some(new)) if old == new => {}
            (Some(old), Some(new)) => {
                out.push_str(&format!("@@ line {} @@\n-{}\n+{}\n", index + 1, old, new));
            }
            (Some(old), None) => {
                out.push_str(&format!("@@ line {} @@\n-{}\n", index + 1, old));
            }
            (None, Some(new)) => {
                out.push_str(&format!("@@ line {} @@\n+{}\n", index + 1, new));
            }
            (None, None) => {}
        }
    }
    out
}

fn ensure_clean_git_worktree(path: &Path) -> anyhow::Result<()> {
    let root = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["status", "--porcelain"])
        .output()
        .context("failed to run git status for --confirm")?;
    if !output.status.success() {
        bail!("--confirm requires --path to be inside a Git worktree");
    }
    if !output.stdout.is_empty() {
        bail!("--confirm requires a clean Git worktree before applying replacements");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_replace_preserves_boundaries() {
        let (text, count) = replace_identifier("old older x.old(old)\n", "old", "new");
        assert_eq!(text, "new older x.new(new)\n");
        assert_eq!(count, 3);
    }

    #[test]
    fn glob_matching_supports_simple_file_filters() {
        assert!(glob_matches("src/*.py", "src/main.py"));
        assert!(glob_matches("*.md", "README.md"));
        assert!(!glob_matches("*.py", "README.md"));
    }
}
