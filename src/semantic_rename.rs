use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Serialize;
use tree_sitter::{Node, Parser, TreeCursor};

use crate::lsp::{ClangdOptions, TextEdit};
use crate::model::Language;
use crate::path_display::display_path;
use crate::replace::{FileDiff, ReplaceOptions};
use crate::workspace::{language_for_path, read_text, source_files};

#[derive(Clone, Debug)]
pub struct SemanticRenameOptions {
    pub replace: ReplaceOptions,
    pub root: Option<PathBuf>,
    pub compile_commands_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RenamePlan {
    pub backend: String,
    pub confidence: String,
    pub definitions_changed: usize,
    pub references_changed: usize,
    pub ambiguous_matches: Vec<RenameMatch>,
    pub skipped_matches: Vec<RenameMatch>,
    pub files_scanned: usize,
    pub files_changed: usize,
    pub replacements: usize,
    pub applied: bool,
    pub diffs: Vec<FileDiff>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RenameMatch {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub text: String,
    pub reason: String,
}

#[derive(Clone, Debug)]
struct PlannedEdit {
    path: PathBuf,
    start: usize,
    end: usize,
    replacement: String,
    kind: EditKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditKind {
    Definition,
    Reference,
}

pub fn run(options: &SemanticRenameOptions, from: &str, to: &str) -> anyhow::Result<RenamePlan> {
    crate::replace::validate_options(&options.replace)?;
    let search_root = options.replace.path.canonicalize().with_context(|| {
        format!(
            "failed to resolve --path {}",
            options.replace.path.display()
        )
    })?;
    if options.replace.apply && options.replace.confirm {
        crate::replace::ensure_clean_git_worktree(&search_root)?;
    }

    let mut files_scanned = 0;
    let mut python_files = Vec::new();
    let mut c_family_files = Vec::new();
    for file in source_files(&search_root, options.replace.lang) {
        if !crate::replace::path_allowed(
            &search_root,
            &file,
            &options.replace.include,
            &options.replace.exclude,
        ) {
            continue;
        }
        files_scanned += 1;
        let Some(text) = read_text(&file) else {
            continue;
        };
        match language_for_path(&file) {
            Some(Language::Python) => python_files.push((file, text)),
            Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip) => {
                c_family_files.push((file, text));
            }
            _ => {}
        }
    }

    let mut edits = Vec::new();
    let mut ambiguous_matches = Vec::new();
    let mut notes = Vec::new();
    let mut backends = Vec::new();

    if !python_files.is_empty() {
        backends.push("tree-sitter-python");
        let (mut python_edits, mut ambiguous) = python_edits(&python_files, from, to)?;
        edits.append(&mut python_edits);
        ambiguous_matches.append(&mut ambiguous);
    }

    if !c_family_files.is_empty() {
        backends.push("clangd");
        let root = match &options.root {
            Some(root) => root
                .canonicalize()
                .with_context(|| format!("failed to resolve --root {}", root.display()))?,
            None if search_root.is_dir() => search_root.clone(),
            None => search_root
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| search_root.clone()),
        };
        let lsp_edits = crate::lsp::rename(
            &c_family_files,
            &ClangdOptions {
                root,
                compile_commands_dir: options.compile_commands_dir.clone(),
            },
            from,
            to,
        )?;
        edits.extend(lsp_edits.into_iter().map(|edit| PlannedEdit {
            path: edit.path,
            start: edit.start,
            end: edit.end,
            replacement: edit.replacement,
            kind: EditKind::Reference,
        }));
    }

    if backends.is_empty() {
        bail!("semantic rename supports Python and C-family files only");
    }

    detect_conflicts(&edits)?;
    let edit_keys = edit_keys(&edits);
    let mut skipped_matches = Vec::new();
    for (path, text) in python_files.iter().chain(c_family_files.iter()) {
        skipped_matches.extend(textual_leftovers(path, text, from, &edit_keys));
    }

    let mut grouped: BTreeMap<PathBuf, Vec<PlannedEdit>> = BTreeMap::new();
    for edit in edits {
        grouped.entry(edit.path.clone()).or_default().push(edit);
    }

    let mut files_changed = 0;
    let mut replacements = 0;
    let mut definitions_changed = 0;
    let mut references_changed = 0;
    let mut diffs = Vec::new();
    for (path, mut file_edits) in grouped {
        let old_text =
            read_text(&path).with_context(|| format!("failed to read {}", path.display()))?;
        file_edits.sort_by_key(|edit| edit.start);
        if files_changed >= options.replace.max_files {
            bail!(
                "semantic rename would modify more than --max-files {} files",
                options.replace.max_files
            );
        }
        for edit in &file_edits {
            match edit.kind {
                EditKind::Definition => definitions_changed += 1,
                EditKind::Reference => references_changed += 1,
            }
        }
        let new_text = apply_edits(&old_text, &file_edits)?;
        if old_text == new_text {
            continue;
        }
        files_changed += 1;
        replacements += file_edits.len();
        diffs.push(FileDiff {
            path: path.clone(),
            replacements: file_edits.len(),
            diff: crate::replace::unified_diff(&path, &old_text, &new_text),
        });
        if options.replace.apply {
            fs::write(&path, new_text)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
    }

    if replacements == 0 {
        notes.push("no safe semantic edits found".to_string());
    }

    let confidence = if ambiguous_matches.is_empty() && skipped_matches.is_empty() {
        "high"
    } else if definitions_changed > 0 || references_changed > 0 {
        "medium"
    } else {
        "low"
    };

    Ok(RenamePlan {
        backend: backends.join("+"),
        confidence: confidence.to_string(),
        definitions_changed,
        references_changed,
        ambiguous_matches,
        skipped_matches,
        files_scanned,
        files_changed,
        replacements,
        applied: options.replace.apply,
        diffs,
        notes,
    })
}

pub fn render(plan: &RenamePlan) -> String {
    let mode = if plan.applied { "applied" } else { "preview" };
    let mut out = format!(
        "{mode}: {} semantic replacements across {} files ({} files scanned)\nbackend: {}\nconfidence: {}\ndefinitions changed: {}\nreferences changed: {}\nambiguous matches: {}\nskipped matches: {}",
        plan.replacements,
        plan.files_changed,
        plan.files_scanned,
        plan.backend,
        plan.confidence,
        plan.definitions_changed,
        plan.references_changed,
        plan.ambiguous_matches.len(),
        plan.skipped_matches.len()
    );
    if !plan.ambiguous_matches.is_empty() {
        out.push_str("\n\n## Ambiguous");
        for item in &plan.ambiguous_matches {
            out.push_str(&format!(
                "\n{}:{}:{}: {} ({})",
                display_path(&item.path),
                item.line,
                item.column,
                item.text,
                item.reason
            ));
        }
    }
    if !plan.skipped_matches.is_empty() {
        out.push_str("\n\n## Skipped");
        for item in &plan.skipped_matches {
            out.push_str(&format!(
                "\n{}:{}:{}: {} ({})",
                display_path(&item.path),
                item.line,
                item.column,
                item.text,
                item.reason
            ));
        }
    }
    for note in &plan.notes {
        out.push_str(&format!("\nnote: {note}"));
    }
    for diff in &plan.diffs {
        out.push_str(&format!(
            "\n\n# {} ({} replacements)\n{}",
            display_path(&diff.path),
            diff.replacements,
            diff.diff
        ));
    }
    out
}

fn python_edits(
    files: &[(PathBuf, String)],
    from: &str,
    to: &str,
) -> anyhow::Result<(Vec<PlannedEdit>, Vec<RenameMatch>)> {
    let short = short_name(from);
    let mut edits = Vec::new();
    let mut definitions = Vec::new();
    for (path, text) in files {
        let Some(tree) = parse_python(text) else {
            continue;
        };
        visit_all(tree.root_node(), &mut |node| {
            if node.kind() != "identifier" {
                return;
            }
            if node_text(node, text).as_deref() != Some(short.as_str()) {
                return;
            }
            let kind = if is_definition_name(node) || is_assignment_target(node) {
                EditKind::Definition
            } else {
                EditKind::Reference
            };
            if kind == EditKind::Definition {
                definitions.push((path.clone(), node.start_byte(), node.end_byte()));
            }
            edits.push(PlannedEdit {
                path: path.clone(),
                start: node.start_byte(),
                end: node.end_byte(),
                replacement: to.to_string(),
                kind,
            });
        });
    }

    let mut ambiguous = Vec::new();
    let definition_files = definitions
        .iter()
        .map(|(path, _, _)| path.clone())
        .collect::<BTreeSet<_>>();
    if definition_files.len() > 1 {
        for (path, start, _) in definitions {
            if let Some(text) = files
                .iter()
                .find(|(candidate, _)| *candidate == path)
                .map(|(_, text)| text)
            {
                let (line, column) = line_column(text, start);
                ambiguous.push(RenameMatch {
                    path,
                    line,
                    column,
                    text: short.clone(),
                    reason: "duplicate Python definitions matched; review rename scope".to_string(),
                });
            }
        }
    }

    Ok((edits, ambiguous))
}

fn parse_python(text: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .ok()?;
    parser.parse(text, None)
}

fn node_text(node: Node<'_>, text: &str) -> Option<String> {
    node.utf8_text(text.as_bytes()).ok().map(str::to_string)
}

fn visit_all(node: Node<'_>, visitor: &mut impl FnMut(Node<'_>)) {
    visitor(node);
    let mut cursor: TreeCursor<'_> = node.walk();
    for child in node.children(&mut cursor) {
        visit_all(child, visitor);
    }
}

fn is_definition_name(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    matches!(parent.kind(), "function_definition" | "class_definition")
        && parent
            .child_by_field_name("name")
            .is_some_and(|name| same_node(name, node))
}

fn is_assignment_target(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() == "attribute" {
        return false;
    }
    let Some(assignment) = parent.parent() else {
        return false;
    };
    matches!(
        assignment.kind(),
        "assignment" | "augmented_assignment" | "type_alias_statement"
    ) && assignment
        .child_by_field_name("left")
        .is_some_and(|left| contains_node(left, node))
}

fn contains_node(container: Node<'_>, needle: Node<'_>) -> bool {
    container.start_byte() <= needle.start_byte() && needle.end_byte() <= container.end_byte()
}

fn same_node(left: Node<'_>, right: Node<'_>) -> bool {
    left.start_byte() == right.start_byte() && left.end_byte() == right.end_byte()
}

fn apply_edits(text: &str, edits: &[PlannedEdit]) -> anyhow::Result<String> {
    let mut out = text.to_string();
    for edit in edits.iter().rev() {
        if edit.start > edit.end || edit.end > out.len() {
            bail!("semantic rename produced an out-of-range edit");
        }
        out.replace_range(edit.start..edit.end, &edit.replacement);
    }
    Ok(out)
}

fn detect_conflicts(edits: &[PlannedEdit]) -> anyhow::Result<()> {
    let mut grouped: BTreeMap<&Path, Vec<&PlannedEdit>> = BTreeMap::new();
    for edit in edits {
        grouped.entry(&edit.path).or_default().push(edit);
    }
    for (path, mut file_edits) in grouped {
        file_edits.sort_by_key(|edit| edit.start);
        for pair in file_edits.windows(2) {
            if pair[0].end > pair[1].start {
                bail!(
                    "semantic rename produced overlapping edits in {}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

fn edit_keys(edits: &[PlannedEdit]) -> BTreeSet<(PathBuf, usize, usize)> {
    edits
        .iter()
        .map(|edit| (edit.path.clone(), edit.start, edit.end))
        .collect()
}

fn textual_leftovers(
    path: &Path,
    text: &str,
    from: &str,
    edit_keys: &BTreeSet<(PathBuf, usize, usize)>,
) -> Vec<RenameMatch> {
    let mut out = Vec::new();
    let mut index = 0;
    while let Some(relative_start) = text[index..].find(from) {
        let start = index + relative_start;
        let end = start + from.len();
        if crate::replace::identifier_boundary_before(text, start, from)
            && crate::replace::identifier_boundary_after(text, end, from)
            && !edit_keys.contains(&(path.to_path_buf(), start, end))
        {
            let (line, column) = line_column(text, start);
            out.push(RenameMatch {
                path: path.to_path_buf(),
                line,
                column,
                text: from.to_string(),
                reason: "textual identifier match outside semantic edit set".to_string(),
            });
        }
        let next = text[start..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(from.len());
        index = start + next;
    }
    out
}

fn line_column(text: &str, byte_index: usize) -> (usize, usize) {
    let mut line = 1;
    let mut line_start = 0;
    for (idx, ch) in text.char_indices() {
        if idx >= byte_index {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = idx + 1;
        }
    }
    (line, text[line_start..byte_index].chars().count() + 1)
}

fn short_name(name: &str) -> String {
    name.replace("::", ".")
        .rsplit('.')
        .next()
        .unwrap_or(name)
        .to_string()
}

impl From<TextEdit> for PlannedEdit {
    fn from(value: TextEdit) -> Self {
        Self {
            path: value.path,
            start: value.start,
            end: value.end,
            replacement: value.replacement,
            kind: EditKind::Reference,
        }
    }
}
