use std::path::{Path, PathBuf};

use anyhow::Context;
use regex::Regex;

use crate::model::{Backend, Language, LanguageFilter, RelatedRecord, Relationship, SymbolKind};
use crate::workspace::{language_for_path, read_text, source_files};

#[derive(Clone, Debug)]
pub struct RelatedOptions {
    pub path: PathBuf,
    pub lang: Option<LanguageFilter>,
    pub name: Option<String>,
    pub file: Option<PathBuf>,
    pub max_matches: usize,
}

#[derive(Clone, Debug)]
struct Subject {
    root: PathBuf,
    file: Option<PathBuf>,
    name: Option<String>,
    stem: Option<String>,
}

pub fn collect(options: &RelatedOptions) -> anyhow::Result<Vec<RelatedRecord>> {
    let root = options
        .path
        .canonicalize()
        .with_context(|| format!("failed to resolve --path {}", options.path.display()))?;
    let subject = subject(options, &root)?;
    let search_root = if root.is_file() {
        root.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone())
    } else {
        root.clone()
    };
    let mut records = Vec::new();

    if let Some(file) = &subject.file {
        collect_file_relationships(&mut records, &subject, file, options.lang);
    }
    if let Some(name) = &subject.name {
        collect_name_relationships(&mut records, &subject, &search_root, name, options.lang);
    }

    collect_neighbors(&mut records, &subject, &search_root, options.lang);
    rank_dedupe(&mut records, &subject);
    records.truncate(options.max_matches);
    Ok(records)
}

fn subject(options: &RelatedOptions, root: &Path) -> anyhow::Result<Subject> {
    match (&options.name, &options.file) {
        (Some(name), None) => Ok(Subject {
            root: root.to_path_buf(),
            file: None,
            name: Some(short_name(name)),
            stem: Some(short_name(name)),
        }),
        (None, Some(file)) => {
            let resolved = resolve_file(root, file)?;
            let stem = resolved
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string());
            Ok(Subject {
                root: root.to_path_buf(),
                file: Some(resolved),
                name: stem.clone(),
                stem,
            })
        }
        (Some(_), Some(_)) => anyhow::bail!("related accepts either --name or --file, not both"),
        (None, None) => anyhow::bail!("related requires --name or --file"),
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

fn collect_file_relationships(
    records: &mut Vec<RelatedRecord>,
    subject: &Subject,
    file: &Path,
    lang: Option<LanguageFilter>,
) {
    match language_for_path(file) {
        Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
            collect_c_family_pair(records, subject, file, lang);
            collect_build_links(records, subject, file);
            collect_docs_for_file(records, subject);
        }
        Some(Language::Python) => {
            collect_python_tests(records, subject);
            collect_docs_for_file(records, subject);
        }
        Some(Language::Markdown) => collect_markdown_links(records, subject, file),
        _ => {}
    }
}

fn collect_name_relationships(
    records: &mut Vec<RelatedRecord>,
    subject: &Subject,
    root: &Path,
    name: &str,
    lang: Option<LanguageFilter>,
) {
    for file in source_files(root, lang) {
        let Some(text) = read_text(&file) else {
            continue;
        };
        let Some(language) = language_for_path(&file) else {
            continue;
        };
        match language {
            Language::Python => {
                for symbol in crate::python::symbols(&file, &text, None, Some(name)) {
                    records.push(RelatedRecord::new(
                        symbol.path,
                        Relationship::Definition,
                        100,
                        "definition matching subject name",
                        symbol.language,
                        symbol.start_line,
                        symbol.end_line,
                    ));
                }
                for reference in crate::python::references(&file, &text, name, 20) {
                    records.push(RelatedRecord::new(
                        reference.path,
                        Relationship::Reference,
                        60,
                        "direct reference to subject name",
                        reference.language,
                        reference.start_line,
                        reference.end_line,
                    ));
                }
            }
            Language::C | Language::Cpp | Language::Cuda | Language::Hip => {
                if let Ok(symbols) =
                    crate::cfamily::symbols(&file, &text, Backend::Lexical, None, Some(name))
                {
                    for symbol in symbols {
                        records.push(RelatedRecord::new(
                            symbol.path,
                            Relationship::Definition,
                            95,
                            "definition matching subject name",
                            symbol.language,
                            symbol.start_line,
                            symbol.end_line,
                        ));
                    }
                }
                for reference in crate::cfamily::references(&file, &text, name, 20) {
                    records.push(RelatedRecord::new(
                        reference.path,
                        Relationship::Reference,
                        55,
                        "direct reference to subject name",
                        reference.language,
                        reference.start_line,
                        reference.end_line,
                    ));
                }
            }
            Language::Cmake => {
                for symbol in crate::cmake::symbols(&file, &text, None, Some(name)) {
                    let relationship = if symbol.kind == SymbolKind::Target {
                        Relationship::Build
                    } else {
                        Relationship::Definition
                    };
                    records.push(RelatedRecord::new(
                        symbol.path,
                        relationship,
                        85,
                        "CMake symbol matching subject name",
                        symbol.language,
                        symbol.start_line,
                        symbol.end_line,
                    ));
                }
                for reference in crate::cmake::references(&file, &text, name, 20) {
                    records.push(RelatedRecord::new(
                        reference.path,
                        Relationship::Build,
                        65,
                        "CMake reference to subject name",
                        reference.language,
                        reference.start_line,
                        reference.end_line,
                    ));
                }
            }
            Language::Markdown => collect_doc_mentions(records, &file, &text, name),
            Language::Rust | Language::Text => {}
        }
    }

    collect_tests_for_name(records, subject, name, lang);
}

fn collect_c_family_pair(
    records: &mut Vec<RelatedRecord>,
    subject: &Subject,
    file: &Path,
    lang: Option<LanguageFilter>,
) {
    let Some(stem) = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
    else {
        return;
    };
    let subject_is_header = is_header(file);
    let root = search_root(subject);
    for candidate in source_files(&root, lang) {
        if candidate == file || language_for_path(&candidate).is_none() {
            continue;
        }
        if candidate
            .file_stem()
            .is_none_or(|candidate_stem| candidate_stem.to_string_lossy() != stem)
        {
            continue;
        }
        let relationship = match (subject_is_header, is_header(&candidate)) {
            (true, false) => Relationship::Implementation,
            (false, true) => Relationship::Header,
            _ => continue,
        };
        records.push(RelatedRecord::new(
            candidate.clone(),
            relationship,
            100,
            "C-family header/source basename pair",
            language_for_path(&candidate).unwrap_or(Language::Text),
            1,
            1,
        ));
    }
}

fn collect_build_links(records: &mut Vec<RelatedRecord>, subject: &Subject, file: &Path) {
    let Some(file_name) = file
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
    else {
        return;
    };
    let root = search_root(subject);
    let Ok(pattern) = Regex::new(
        r"(?is)(add_library|add_executable|pybind11_add_module|target_sources)\s*\((?P<args>.*?)\)",
    ) else {
        return;
    };
    for cmake_file in source_files(&root, Some(LanguageFilter::Cmake)) {
        let Some(text) = read_text(&cmake_file) else {
            continue;
        };
        for capture in pattern.captures_iter(&text) {
            let Some(matched) = capture.get(0) else {
                continue;
            };
            let args = capture.name("args").map(|m| m.as_str()).unwrap_or_default();
            if !args.contains(&file_name) {
                continue;
            }
            let start = byte_line(&text, matched.start());
            let end = byte_line(&text, matched.end());
            records.push(RelatedRecord::new(
                cmake_file.clone(),
                Relationship::Build,
                90,
                "CMake target references subject file",
                Language::Cmake,
                start,
                end,
            ));
        }
    }
}

fn collect_python_tests(records: &mut Vec<RelatedRecord>, subject: &Subject) {
    collect_tests(records, subject, None);
}

fn collect_tests_for_name(
    records: &mut Vec<RelatedRecord>,
    subject: &Subject,
    name: &str,
    lang: Option<LanguageFilter>,
) {
    collect_tests(records, subject, Some((name, lang)));
}

fn collect_tests(
    records: &mut Vec<RelatedRecord>,
    subject: &Subject,
    name: Option<(&str, Option<LanguageFilter>)>,
) {
    let options = crate::related_tests::RelatedTestOptions {
        path: subject.root.clone(),
        lang: name.and_then(|(_, lang)| lang),
        name: name.map(|(name, _)| name.to_string()),
        file: if name.is_none() {
            subject.file.clone()
        } else {
            None
        },
        max_matches: 20,
    };
    let Ok(tests) = crate::related_tests::collect(&options) else {
        return;
    };
    for test in tests {
        records.push(RelatedRecord::new(
            test.path,
            Relationship::Test,
            test.score,
            test.reason,
            test.language,
            test.start_line,
            test.end_line,
        ));
    }
}

fn collect_docs_for_file(records: &mut Vec<RelatedRecord>, subject: &Subject) {
    let mut names = Vec::new();
    if let Some(stem) = &subject.stem {
        names.push(stem.clone());
    }
    if let Some(file) = &subject.file
        && let Some(text) = read_text(file)
    {
        match language_for_path(file) {
            Some(Language::Python) => {
                names.extend(
                    crate::python::symbols(file, &text, None, None)
                        .into_iter()
                        .map(|symbol| symbol.name),
                );
            }
            Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
                if let Ok(symbols) =
                    crate::cfamily::symbols(file, &text, Backend::Lexical, None, None)
                {
                    names.extend(symbols.into_iter().map(|symbol| symbol.name));
                }
            }
            _ => {}
        }
    }
    if names.is_empty() {
        return;
    }
    let root = search_root(subject);
    for file in source_files(&root, Some(LanguageFilter::Markdown)) {
        let Some(text) = read_text(&file) else {
            continue;
        };
        for name in &names {
            collect_doc_mentions(records, &file, &text, name);
        }
    }
}

fn collect_doc_mentions(records: &mut Vec<RelatedRecord>, file: &Path, text: &str, name: &str) {
    let needle = short_name(name).to_ascii_lowercase();
    if needle.len() < 3 || !text.to_ascii_lowercase().contains(&needle) {
        return;
    }
    let line = text
        .lines()
        .position(|line| line.to_ascii_lowercase().contains(&needle))
        .map(|idx| idx + 1)
        .unwrap_or(1);
    records.push(RelatedRecord::new(
        file.to_path_buf(),
        Relationship::Doc,
        50,
        "Markdown mention of subject",
        Language::Markdown,
        line,
        line,
    ));
}

fn collect_markdown_links(records: &mut Vec<RelatedRecord>, subject: &Subject, file: &Path) {
    let Some(text) = read_text(file) else {
        return;
    };
    let Some(parent) = file.parent() else {
        return;
    };
    let Ok(pattern) = Regex::new(r"\[[^\]]+\]\((?P<target>[^)#]+)(?:#[^)]+)?\)") else {
        return;
    };
    for capture in pattern.captures_iter(&text) {
        let Some(target) = capture.name("target").map(|m| m.as_str().trim()) else {
            continue;
        };
        if target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("mailto:")
            || target.is_empty()
        {
            continue;
        }
        let linked = parent.join(target);
        if !linked.exists() {
            continue;
        }
        let linked_language = language_for_path(&linked).unwrap_or(Language::Text);
        records.push(RelatedRecord::new(
            linked,
            Relationship::Linked,
            90,
            "Markdown link target",
            linked_language,
            1,
            1,
        ));
    }

    let root = search_root(subject);
    let Some(file_name) = file
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
    else {
        return;
    };
    for candidate in source_files(&root, Some(LanguageFilter::Markdown)) {
        if candidate == file {
            continue;
        }
        let Some(candidate_text) = read_text(&candidate) else {
            continue;
        };
        if !candidate_text.contains(&file_name) {
            continue;
        }
        let line = candidate_text
            .lines()
            .position(|line| line.contains(&file_name))
            .map(|idx| idx + 1)
            .unwrap_or(1);
        records.push(RelatedRecord::new(
            candidate,
            Relationship::Linked,
            80,
            "Markdown backlink to subject file",
            Language::Markdown,
            line,
            line,
        ));
    }
}

fn collect_neighbors(
    records: &mut Vec<RelatedRecord>,
    subject: &Subject,
    root: &Path,
    lang: Option<LanguageFilter>,
) {
    let Some(file) = &subject.file else {
        return;
    };
    let Some(parent) = file.parent() else {
        return;
    };
    for candidate in source_files(root, lang) {
        if candidate == *file || candidate.parent() != Some(parent) {
            continue;
        }
        let Some(language) = language_for_path(&candidate) else {
            continue;
        };
        records.push(RelatedRecord::new(
            candidate,
            Relationship::Neighbor,
            20,
            "nearby source file in same directory",
            language,
            1,
            1,
        ));
    }
}

fn rank_dedupe(records: &mut Vec<RelatedRecord>, subject: &Subject) {
    for record in records.iter_mut() {
        if let Some(file) = &subject.file {
            record.score += proximity_bonus(file, &record.path);
        }
        if subject.file.as_ref() == Some(&record.path) {
            record.score = record.score.saturating_sub(20);
        }
    }
    records.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| (left.relationship as u8).cmp(&(right.relationship as u8)))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.start_line.cmp(&right.start_line))
    });
    let mut out = Vec::new();
    for record in records.drain(..) {
        if !out.iter().any(|existing: &RelatedRecord| {
            existing.path == record.path
                && existing.relationship == record.relationship
                && existing.start_line == record.start_line
                && existing.end_line == record.end_line
        }) {
            out.push(record);
        }
    }
    *records = out;
}

fn proximity_bonus(subject: &Path, candidate: &Path) -> usize {
    if subject.parent() == candidate.parent() {
        return 20;
    }
    let left = subject.components().collect::<Vec<_>>();
    let right = candidate.components().collect::<Vec<_>>();
    let common = left
        .iter()
        .zip(right.iter())
        .take_while(|(left, right)| left == right)
        .count();
    common.min(10)
}

fn is_header(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        matches!(
            ext.to_string_lossy().to_ascii_lowercase().as_str(),
            "h" | "hh" | "hpp" | "hxx" | "cuh"
        )
    })
}

fn search_root(subject: &Subject) -> PathBuf {
    if subject.root.is_file() {
        subject
            .root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| subject.root.clone())
    } else {
        subject.root.clone()
    }
}

fn short_name(name: &str) -> String {
    name.replace("::", ".")
        .rsplit('.')
        .next()
        .unwrap_or(name)
        .to_string()
}

fn byte_line(text: &str, byte: usize) -> usize {
    text[..byte].bytes().filter(|value| *value == b'\n').count() + 1
}
