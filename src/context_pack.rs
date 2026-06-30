use std::collections::HashSet;
use std::path::PathBuf;

use serde::Serialize;

use crate::model::{Language, Symbol};
use crate::path_display::display_path;

#[derive(Clone, Debug, Serialize)]
pub struct ContextPack {
    pub subject: String,
    pub budget: usize,
    pub items: Vec<ContextPackItem>,
    pub omitted: Vec<OmittedContextItem>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContextPackItem {
    pub role: String,
    #[serde(serialize_with = "crate::path_display::serialize")]
    pub path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub language: Language,
    pub backend: String,
    pub score: u32,
    pub reason: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct OmittedContextItem {
    pub role: String,
    #[serde(serialize_with = "crate::path_display::serialize")]
    pub path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub reason: String,
    pub source_chars: usize,
}

impl ContextPack {
    pub fn new(subject: impl Into<String>, budget: usize) -> Self {
        Self {
            subject: subject.into(),
            budget,
            items: Vec::new(),
            omitted: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn push(&mut self, item: ContextPackItem) {
        self.items.push(item);
    }

    pub fn push_symbol(
        &mut self,
        role: &str,
        symbol: Symbol,
        score: u32,
        reason: impl Into<String>,
    ) {
        self.items.push(ContextPackItem::from_symbol(
            role,
            symbol,
            score,
            reason.into(),
        ));
    }

    pub fn rank_dedupe_and_truncate(&mut self) {
        self.items.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.start_line.cmp(&right.start_line))
                .then_with(|| left.end_line.cmp(&right.end_line))
                .then_with(|| left.role.cmp(&right.role))
        });

        let mut seen = HashSet::new();
        self.items.retain(|item| {
            seen.insert((
                item.role.clone(),
                item.path.clone(),
                item.start_line,
                item.end_line,
                item.source.clone(),
            ))
        });

        let mut kept = Vec::new();
        let mut used = 0usize;
        for item in self.items.drain(..) {
            let source_chars = item.source.chars().count();
            let required = item.role == "definition" || item.role == "enclosing-symbol";
            if required || used + source_chars <= self.budget {
                used += source_chars;
                kept.push(item);
            } else {
                self.omitted.push(OmittedContextItem {
                    role: item.role,
                    path: item.path,
                    start_line: item.start_line,
                    end_line: item.end_line,
                    reason: "budget exceeded".to_string(),
                    source_chars,
                });
            }
        }
        self.items = kept;
    }
}

impl ContextPackItem {
    pub fn from_symbol(role: &str, symbol: Symbol, score: u32, reason: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            path: symbol.path,
            start_line: symbol.start_line,
            end_line: symbol.end_line,
            language: symbol.language,
            backend: symbol.backend,
            score,
            reason: reason.into(),
            source: symbol.source,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn synthetic(
        role: &str,
        path: impl Into<PathBuf>,
        start_line: usize,
        end_line: usize,
        language: Language,
        backend: impl Into<String>,
        score: u32,
        reason: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            role: role.to_string(),
            path: path.into(),
            start_line,
            end_line,
            language,
            backend: backend.into(),
            score,
            reason: reason.into(),
            source: source.into(),
        }
    }
}

pub fn render_plain(pack: &ContextPack) -> String {
    let mut out = Vec::new();
    out.push(format!(
        "# Context pack: {} (budget {} chars)",
        pack.subject, pack.budget
    ));
    if !pack.notes.is_empty() {
        out.push(String::new());
        out.push("## Notes".to_string());
        out.extend(pack.notes.iter().map(|note| format!("- {note}")));
    }
    for item in &pack.items {
        out.push(String::new());
        out.push(format!(
            "## {} {}:{}-{} score={}",
            item.role,
            display_path(&item.path),
            item.start_line,
            item.end_line,
            item.score
        ));
        out.push(format!(
            "reason: {} ({}, {})",
            item.reason, item.language, item.backend
        ));
        out.push(item.source.trim_end().to_string());
    }
    if !pack.omitted.is_empty() {
        out.push(String::new());
        out.push("## Omitted".to_string());
        out.extend(pack.omitted.iter().map(render_omitted));
    }
    out.join("\n")
}

fn render_omitted(item: &OmittedContextItem) -> String {
    format!(
        "- {} {}:{}-{} ({} chars): {}",
        item.role,
        display_path(&item.path),
        item.start_line,
        item.end_line,
        item.source_chars,
        item.reason
    )
}
