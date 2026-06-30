use std::path::{Path, PathBuf};

use anyhow::Context;
use regex::Regex;

use crate::model::{Language, LanguageFilter, RelatedTestRecord};
use crate::workspace::{language_for_path, line_slice, read_text, source_files};

#[derive(Clone, Debug)]
pub struct RelatedTestOptions {
    pub path: PathBuf,
    pub lang: Option<LanguageFilter>,
    pub name: Option<String>,
    pub file: Option<PathBuf>,
    pub max_matches: usize,
}

#[derive(Clone, Debug)]
struct Subject {
    display: String,
    name: Option<String>,
    file: Option<PathBuf>,
    file_stem: Option<String>,
    module_name: Option<String>,
}

pub fn collect(options: &RelatedTestOptions) -> anyhow::Result<Vec<RelatedTestRecord>> {
    let root = options
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", options.path.display()))?;
    let subject = subject(options, &root)?;
    let mut records = Vec::new();
    for file in source_files(&root, options.lang) {
        let Some(language) = language_for_path(&file) else {
            continue;
        };
        if language == Language::Markdown {
            continue;
        }
        let Some(text) = read_text(&file) else {
            continue;
        };
        match language {
            Language::Python => collect_python(&mut records, &file, &text, &subject),
            Language::Rust => {}
            Language::C | Language::Cpp | Language::Cuda | Language::Hip => {
                collect_c_family(&mut records, &file, language, &text, &subject);
            }
            Language::Cmake => collect_cmake(&mut records, &file, &text, &subject),
            Language::Markdown | Language::Text => {}
        }
    }
    records.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.start_line.cmp(&right.start_line))
            .then_with(|| left.qualified_name.cmp(&right.qualified_name))
    });
    dedupe(&mut records);
    records.truncate(options.max_matches);
    Ok(records)
}

fn subject(options: &RelatedTestOptions, root: &Path) -> anyhow::Result<Subject> {
    match (&options.name, &options.file) {
        (Some(name), None) => Ok(Subject {
            display: name.clone(),
            name: Some(short_name(name)),
            file: None,
            file_stem: None,
            module_name: None,
        }),
        (None, Some(file)) => {
            let resolved = resolve_file(root, file)?;
            let file_stem = resolved
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string());
            let module_name = python_module_name(&resolved);
            Ok(Subject {
                display: file.display().to_string(),
                name: file_stem.clone(),
                file: Some(resolved),
                file_stem,
                module_name,
            })
        }
        (Some(_), Some(_)) => anyhow::bail!("tests-for accepts either --name or --file, not both"),
        (None, None) => anyhow::bail!("tests-for requires --name or --file"),
    }
}

fn resolve_file(root: &Path, file: &Path) -> anyhow::Result<PathBuf> {
    let path = if file.is_absolute() {
        file.to_path_buf()
    } else if root.is_file() {
        root.parent()
            .map(|parent| parent.join(file))
            .unwrap_or_else(|| file.to_path_buf())
    } else {
        root.join(file)
    };
    path.canonicalize()
        .with_context(|| format!("failed to resolve --file {}", file.display()))
}

fn collect_python(
    records: &mut Vec<RelatedTestRecord>,
    path: &Path,
    text: &str,
    subject: &Subject,
) {
    let path_score = test_path_score(path, text, Language::Python, subject);
    if path_score == 0 && !mentions_subject(text, subject) {
        return;
    }
    let symbols = crate::python::symbols(path, text, None, None);
    for symbol in symbols {
        if !matches!(
            symbol.kind,
            crate::model::SymbolKind::Function | crate::model::SymbolKind::Class
        ) {
            continue;
        }
        let mut reasons = Vec::new();
        let mut score = path_score;
        add_symbol_score(
            &mut score,
            &mut reasons,
            &symbol.qualified_name,
            &symbol.source,
            subject,
        );
        if !is_python_test_symbol(&symbol.name, &symbol.qualified_name)
            && reasons.is_empty()
            && score < 50
        {
            continue;
        }
        add_path_reasons(&mut reasons, path, subject);
        if reasons.is_empty() {
            reasons.push("Python test candidate".to_string());
        }
        records.push(RelatedTestRecord::new(
            path.to_path_buf(),
            Language::Python,
            "tree-sitter",
            symbol.name,
            symbol.qualified_name,
            symbol.start_line,
            symbol.end_line,
            reasons.join("; "),
            score.max(50),
            symbol.source,
        ));
    }
    if !records.iter().any(|record| record.path == path) && path_score > 0 {
        let line = first_subject_line(text, subject).unwrap_or(1);
        let mut reasons = Vec::new();
        add_path_reasons(&mut reasons, path, subject);
        records.push(line_record(
            path,
            Language::Python,
            subject,
            line,
            reasons.join("; "),
            path_score,
            text,
        ));
    }
}

fn collect_c_family(
    records: &mut Vec<RelatedTestRecord>,
    path: &Path,
    language: Language,
    text: &str,
    subject: &Subject,
) {
    let path_score = test_path_score(path, text, language, subject);
    if path_score == 0 && !mentions_subject(text, subject) {
        return;
    }
    let macro_pattern =
        Regex::new(r"\b(TEST|TEST_F|TEST_P|TYPED_TEST|SCENARIO|SECTION|CHECK|REQUIRE)\b").unwrap();
    let mut found = false;
    for (idx, line) in text.lines().enumerate() {
        let is_test_line =
            macro_pattern.is_match(line) || is_test_path(path) && mentions_subject(line, subject);
        if !is_test_line {
            continue;
        }
        let line_no = idx + 1;
        let name = c_family_test_name(line).unwrap_or_else(|| subject.display.clone());
        let source = line_slice(text, line_no, (line_no + 6).min(text.lines().count()));
        let mut reasons = Vec::new();
        let mut score = path_score + 20;
        add_symbol_score(&mut score, &mut reasons, &name, &source, subject);
        add_path_reasons(&mut reasons, path, subject);
        if score < 40 && !mentions_subject(&source, subject) {
            continue;
        }
        found = true;
        records.push(RelatedTestRecord::new(
            path.to_path_buf(),
            language,
            "lexical",
            name.clone(),
            name,
            line_no,
            line_no,
            reasons.join("; "),
            score,
            source,
        ));
    }
    if !found && path_score > 0 {
        let line = first_subject_line(text, subject).unwrap_or(1);
        let mut reasons = Vec::new();
        add_path_reasons(&mut reasons, path, subject);
        records.push(line_record(
            path,
            language,
            subject,
            line,
            reasons.join("; "),
            path_score,
            text,
        ));
    }
}

fn collect_cmake(records: &mut Vec<RelatedTestRecord>, path: &Path, text: &str, subject: &Subject) {
    let Ok(pattern) = Regex::new(r"(?is)add_test\s*\((?P<args>.*?)\)") else {
        return;
    };
    for capture in pattern.captures_iter(text) {
        let Some(matched) = capture.get(0) else {
            continue;
        };
        let args = capture.name("args").map(|m| m.as_str()).unwrap_or_default();
        if !mentions_subject(args, subject) {
            continue;
        }
        let parsed = split_args(args);
        let test_name = cmake_test_name(&parsed).unwrap_or_else(|| subject.display.clone());
        let start_line = byte_line(text, matched.start());
        let end_line = byte_line(text, matched.end());
        let mut score = 90;
        let mut reasons = vec!["CMake add_test references subject".to_string()];
        add_symbol_score(&mut score, &mut reasons, &test_name, args, subject);
        records.push(RelatedTestRecord::new(
            path.to_path_buf(),
            Language::Cmake,
            "lexical",
            test_name.clone(),
            test_name,
            start_line,
            end_line,
            reasons.join("; "),
            score,
            line_slice(text, start_line, end_line),
        ));
    }
}

fn line_record(
    path: &Path,
    language: Language,
    subject: &Subject,
    line: usize,
    reason: String,
    score: usize,
    text: &str,
) -> RelatedTestRecord {
    let start = line.saturating_sub(3).max(1);
    let end = line + 6;
    RelatedTestRecord::new(
        path.to_path_buf(),
        language,
        "lexical",
        &subject.display,
        &subject.display,
        start,
        end,
        reason,
        score,
        line_slice(text, start, end),
    )
}

fn add_symbol_score(
    score: &mut usize,
    reasons: &mut Vec<String>,
    qualified_name: &str,
    source: &str,
    subject: &Subject,
) {
    if let Some(name) = &subject.name {
        if token_contains(qualified_name, name) {
            *score += 35;
            reasons.push("test name contains subject".to_string());
        }
        if token_contains(source, name) {
            *score += 25;
            reasons.push("test source references subject".to_string());
        }
    }
    if let Some(module) = &subject.module_name
        && token_contains(source, module)
    {
        *score += 35;
        reasons.push("test imports or references module under test".to_string());
    }
    if let Some(file_stem) = &subject.file_stem
        && token_contains(qualified_name, file_stem)
    {
        *score += 25;
        reasons.push("test name contains file stem".to_string());
    }
}

fn add_path_reasons(reasons: &mut Vec<String>, path: &Path, subject: &Subject) {
    if is_test_path(path) {
        reasons.push("path is under or named like tests".to_string());
    }
    if test_file_name_matches(path, subject) {
        reasons.push("test filename matches subject file".to_string());
    }
}

fn test_path_score(path: &Path, text: &str, language: Language, subject: &Subject) -> usize {
    let mut score = 0;
    if is_test_path(path) {
        score += 35;
    }
    if test_file_name_matches(path, subject) {
        score += 55;
    }
    match language {
        Language::Python if is_python_test_file(path) => score += 30,
        Language::C | Language::Cpp | Language::Cuda | Language::Hip
            if contains_c_family_test_marker(text) =>
        {
            score += 30;
        }
        _ => {}
    }
    score
}

fn mentions_subject(text: &str, subject: &Subject) -> bool {
    subject
        .name
        .as_ref()
        .is_some_and(|name| token_contains(text, name))
        || subject
            .module_name
            .as_ref()
            .is_some_and(|module| token_contains(text, module))
        || subject
            .file
            .as_ref()
            .and_then(|file| file.file_name())
            .is_some_and(|file| text.contains(&file.to_string_lossy().to_string()))
}

fn first_subject_line(text: &str, subject: &Subject) -> Option<usize> {
    text.lines()
        .position(|line| mentions_subject(line, subject))
        .map(|idx| idx + 1)
}

fn is_test_path(path: &Path) -> bool {
    path.components().any(|part| {
        let value = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        value == "tests" || value == "test" || value == "spec"
    }) || path.file_stem().is_some_and(|stem| {
        let value = stem.to_string_lossy().to_ascii_lowercase();
        value.starts_with("test_")
            || value.ends_with("_test")
            || value.ends_with("_tests")
            || value.contains("test")
            || value.contains("spec")
    })
}

fn is_python_test_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let value = name.to_string_lossy().to_ascii_lowercase();
        value.starts_with("test_") || value.ends_with("_test.py")
    })
}

fn is_python_test_symbol(name: &str, qualified_name: &str) -> bool {
    name.starts_with("test")
        || qualified_name
            .split('.')
            .any(|part| part.starts_with("Test") || part.starts_with("test"))
}

fn contains_c_family_test_marker(text: &str) -> bool {
    text.contains("TEST(")
        || text.contains("TEST_F(")
        || text.contains("TEST_P(")
        || text.contains("SCENARIO(")
        || text.contains("REQUIRE(")
        || text.contains("CHECK(")
}

fn test_file_name_matches(path: &Path, subject: &Subject) -> bool {
    let Some(stem) = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
    else {
        return false;
    };
    subject.file_stem.as_ref().is_some_and(|file_stem| {
        let file_stem = file_stem.to_ascii_lowercase();
        stem == format!("test_{file_stem}")
            || stem == format!("{file_stem}_test")
            || stem == format!("{file_stem}_tests")
            || stem.contains(&file_stem)
    })
}

fn token_contains(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn short_name(name: &str) -> String {
    name.replace("::", ".")
        .rsplit('.')
        .next()
        .unwrap_or(name)
        .to_string()
}

fn python_module_name(path: &Path) -> Option<String> {
    if language_for_path(path) != Some(Language::Python) {
        return None;
    }
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .filter(|stem| stem != "__init__")
}

fn c_family_test_name(line: &str) -> Option<String> {
    let open = line.find('(')?;
    let close = line[open + 1..].find(')')? + open + 1;
    Some(line[open + 1..close].trim().replace(',', "."))
}

fn cmake_test_name(args: &[String]) -> Option<String> {
    if args
        .first()
        .is_some_and(|arg| arg.eq_ignore_ascii_case("NAME"))
    {
        args.get(1).cloned()
    } else {
        args.first().cloned()
    }
}

fn split_args(raw: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quote {
            match ch {
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                '"' => in_quote = false,
                _ => current.push(ch),
            }
            continue;
        }
        match ch {
            '"' => in_quote = true,
            '#' => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            ch if ch.is_whitespace() => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn byte_line(text: &str, byte: usize) -> usize {
    text[..byte].bytes().filter(|value| *value == b'\n').count() + 1
}

fn dedupe(records: &mut Vec<RelatedTestRecord>) {
    let mut out = Vec::new();
    for record in records.drain(..) {
        if !out.iter().any(|existing: &RelatedTestRecord| {
            existing.path == record.path
                && existing.start_line == record.start_line
                && existing.qualified_name == record.qualified_name
        }) {
            out.push(record);
        }
    }
    *records = out;
}
