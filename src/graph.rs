use std::collections::HashMap;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Serialize;

use crate::model::{Language, Symbol, SymbolKind};
use crate::path_display::display_path;
use crate::workspace::{language_for_path, line_slice};

#[derive(Clone, Debug, Serialize)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphNode {
    pub id: String,
    #[serde(serialize_with = "crate::path_display::serialize")]
    pub path: PathBuf,
    pub language: Language,
    pub backend: String,
    pub kind: String,
    pub name: String,
    pub qualified_name: String,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
    pub backend: String,
    pub confidence: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Calls,
    CalledBy,
    Reads,
    Writes,
    Mutates,
    Imports,
}

impl std::fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Calls => "calls",
            Self::CalledBy => "called_by",
            Self::Reads => "reads",
            Self::Writes => "writes",
            Self::Mutates => "mutates",
            Self::Imports => "imports",
        };
        f.write_str(value)
    }
}

impl Graph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_symbol(&mut self, symbol: &Symbol) -> String {
        let node = GraphNode::from_symbol(symbol);
        let id = node.id.clone();
        self.add_node(node);
        id
    }

    pub fn add_node(&mut self, node: GraphNode) {
        if self.nodes.iter().any(|existing| existing.id == node.id) {
            return;
        }
        self.nodes.push(node);
        self.nodes.sort_by_key(|node| {
            (
                display_path(&node.path),
                node.start_line,
                node.end_line,
                node.qualified_name.clone(),
            )
        });
    }

    pub fn add_edge(
        &mut self,
        source: impl Into<String>,
        target: impl Into<String>,
        kind: EdgeKind,
        backend: impl Into<String>,
        confidence: impl Into<String>,
    ) {
        let edge = GraphEdge {
            source: source.into(),
            target: target.into(),
            kind,
            backend: backend.into(),
            confidence: confidence.into(),
        };
        if self.edges.iter().any(|existing| {
            existing.source == edge.source
                && existing.target == edge.target
                && existing.kind == edge.kind
        }) {
            return;
        }
        self.edges.push(edge);
        self.edges.sort_by_key(|edge| {
            (
                edge.source.clone(),
                edge.target.clone(),
                edge.kind.to_string(),
            )
        });
    }

    pub fn truncated(&self, max_nodes: usize) -> Self {
        if self.nodes.len() <= max_nodes {
            return self.clone();
        }
        let nodes = self
            .nodes
            .iter()
            .take(max_nodes)
            .cloned()
            .collect::<Vec<_>>();
        let keep = nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<std::collections::HashSet<_>>();
        let edges = self
            .edges
            .iter()
            .filter(|edge| keep.contains(&edge.source) && keep.contains(&edge.target))
            .cloned()
            .collect();
        Self { nodes, edges }
    }
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphNode {
    pub fn from_symbol(symbol: &Symbol) -> Self {
        Self {
            id: symbol_id(symbol),
            path: symbol.path.clone(),
            language: symbol.language,
            backend: symbol.backend.clone(),
            kind: symbol.kind.to_string(),
            name: symbol.name.clone(),
            qualified_name: symbol.qualified_name.clone(),
            start_line: symbol.start_line,
            end_line: symbol.end_line,
            source: symbol.source.clone(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn synthetic(
        path: PathBuf,
        language: Language,
        backend: impl Into<String>,
        kind: impl Into<String>,
        name: impl Into<String>,
        qualified_name: impl Into<String>,
        start_line: usize,
        end_line: usize,
        source: impl Into<String>,
    ) -> Self {
        let backend = backend.into();
        let kind = kind.into();
        let name = name.into();
        let qualified_name = qualified_name.into();
        let id = format!(
            "{}:{}:{}:{}",
            display_path(&path),
            start_line,
            end_line,
            qualified_name
        );
        Self {
            id,
            path,
            language,
            backend,
            kind,
            name,
            qualified_name,
            start_line,
            end_line,
            source: source.into(),
        }
    }
}

pub fn symbol_id(symbol: &Symbol) -> String {
    format!(
        "{}:{}:{}:{}",
        display_path(&symbol.path),
        symbol.start_line,
        symbol.end_line,
        symbol.qualified_name
    )
}

pub fn render_plain(graph: &Graph) -> String {
    let mut out = Vec::new();
    out.push(format!(
        "# Graph: {} nodes, {} edges",
        graph.nodes.len(),
        graph.edges.len()
    ));
    let by_id = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    for kind in [
        EdgeKind::Calls,
        EdgeKind::CalledBy,
        EdgeKind::Reads,
        EdgeKind::Writes,
        EdgeKind::Mutates,
        EdgeKind::Imports,
    ] {
        let edges = graph
            .edges
            .iter()
            .filter(|edge| edge.kind == kind)
            .collect::<Vec<_>>();
        if edges.is_empty() {
            continue;
        }
        out.push(String::new());
        out.push(format!("## {kind}"));
        for edge in edges {
            let source = by_id
                .get(edge.source.as_str())
                .map(|node| node_label(node))
                .unwrap_or_else(|| edge.source.clone());
            let target = by_id
                .get(edge.target.as_str())
                .map(|node| node_label(node))
                .unwrap_or_else(|| edge.target.clone());
            out.push(format!(
                "- {source} -> {target} ({}, {})",
                edge.backend, edge.confidence
            ));
        }
    }
    out.join("\n")
}

pub fn direct_call_names(source: &str, _subject_name: &str) -> Vec<String> {
    let Ok(pattern) = Regex::new(r"\b([A-Za-z_][A-Za-z0-9_:.\-]*)\s*\(") else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for capture in pattern.captures_iter(source) {
        let Some(raw) = capture.get(1).map(|m| m.as_str()) else {
            continue;
        };
        let short = short_name(raw);
        if is_common_call_token(&short) || names.contains(&short) {
            continue;
        }
        names.push(short);
    }
    names.sort();
    names
}

pub fn matching_function_symbols(symbols: &[Symbol], wanted: &str) -> Vec<Symbol> {
    let wanted_short = short_name(wanted);
    let mut out = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Function)
        .filter(|symbol| {
            symbol.name == wanted_short
                || symbol.qualified_name == wanted
                || symbol.qualified_name.ends_with(&format!(".{wanted}"))
                || symbol.qualified_name.ends_with(&format!("::{wanted}"))
        })
        .cloned()
        .collect::<Vec<_>>();
    out.sort_by_key(|symbol| {
        (
            display_path(&symbol.path),
            symbol.start_line,
            symbol.qualified_name.clone(),
        )
    });
    out
}

pub fn python_dataflow(path: &Path, text: &str, wanted: &str) -> Graph {
    dataflow_lines(path, text, Language::Python, wanted)
}

pub fn cfamily_dataflow(path: &Path, text: &str, wanted: &str) -> Graph {
    let language = language_for_path(path).unwrap_or(Language::Text);
    dataflow_lines(path, text, language, wanted)
}

pub fn cmake_dataflow(path: &Path, text: &str, wanted: &str) -> Graph {
    let mut graph = Graph::new();
    let root = variable_root(path, Language::Cmake, wanted);
    let root_id = root.id.clone();
    graph.add_node(root);
    let name = regex::escape(wanted);
    let set_re = Regex::new(&format!(r"(?i)\b(set|option|unset)\s*\(\s*{name}\b")).ok();
    let list_re = Regex::new(&format!(r"(?i)\blist\s*\(\s*[A-Za-z_]+\s+{name}\b")).ok();
    let ref_re = Regex::new(&format!(r"(?i)(\$\{{\s*{name}\s*\}}|\b{name}\b)")).ok();
    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let stripped = strip_cmake_comment(line);
        let kind = if set_re.as_ref().is_some_and(|re| re.is_match(stripped)) {
            Some(EdgeKind::Writes)
        } else if list_re.as_ref().is_some_and(|re| re.is_match(stripped)) {
            Some(EdgeKind::Mutates)
        } else if ref_re.as_ref().is_some_and(|re| re.is_match(stripped)) {
            Some(EdgeKind::Reads)
        } else {
            None
        };
        if let Some(kind) = kind {
            add_dataflow_occurrence(
                &mut graph,
                path,
                Language::Cmake,
                "lexical",
                wanted,
                line_no,
                line,
                kind,
                &root_id,
            );
        }
    }
    graph
}

fn dataflow_lines(path: &Path, text: &str, language: Language, wanted: &str) -> Graph {
    let mut graph = Graph::new();
    let root = variable_root(path, language, wanted);
    let root_id = root.id.clone();
    graph.add_node(root);
    let name = regex::escape(wanted);
    let assign_re = Regex::new(&format!(
        r"^\s*(?:[A-Za-z_][A-Za-z0-9_]*\.)?{name}\s*(?::[^=]+)?="
    ))
    .ok();
    let mutate_re = Regex::new(&format!(
        r"^\s*(?:[A-Za-z_][A-Za-z0-9_]*\.)?{name}\s*(?:\+=|-=|\*=|/=|//=|%=)"
    ))
    .ok();
    let import_re = Regex::new(&format!(
        r"^\s*(?:from\s+[A-Za-z0-9_.]+\s+import\s+.*\b{name}\b|import\s+.*\b{name}\b)"
    ))
    .ok();
    let ref_re = Regex::new(&format!(r"\b{name}\b")).ok();
    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let kind = if import_re.as_ref().is_some_and(|re| re.is_match(line)) {
            Some(EdgeKind::Imports)
        } else if mutate_re.as_ref().is_some_and(|re| re.is_match(line)) {
            Some(EdgeKind::Mutates)
        } else if assign_re.as_ref().is_some_and(|re| re.is_match(line)) {
            Some(EdgeKind::Writes)
        } else if ref_re.as_ref().is_some_and(|re| re.is_match(line)) {
            Some(EdgeKind::Reads)
        } else {
            None
        };
        if let Some(kind) = kind {
            add_dataflow_occurrence(
                &mut graph, path, language, "lexical", wanted, line_no, line, kind, &root_id,
            );
        }
    }
    graph
}

#[allow(clippy::too_many_arguments)]
fn add_dataflow_occurrence(
    graph: &mut Graph,
    path: &Path,
    language: Language,
    backend: &str,
    wanted: &str,
    line_no: usize,
    line: &str,
    kind: EdgeKind,
    root_id: &str,
) {
    let node = GraphNode::synthetic(
        path.to_path_buf(),
        language,
        backend,
        "reference",
        wanted,
        format!("{wanted}@{line_no}"),
        line_no,
        line_no,
        format!("{line}\n"),
    );
    let id = node.id.clone();
    graph.add_node(node);
    match kind {
        EdgeKind::Reads => graph.add_edge(root_id, id, kind, backend, confidence(backend)),
        _ => graph.add_edge(id, root_id, kind, backend, confidence(backend)),
    }
}

fn variable_root(path: &Path, language: Language, wanted: &str) -> GraphNode {
    GraphNode::synthetic(
        path.to_path_buf(),
        language,
        "query",
        "variable",
        wanted,
        wanted,
        0,
        0,
        "",
    )
}

pub fn confidence(backend: &str) -> &'static str {
    match backend {
        "clangd" | "lsp" => "high",
        "tree-sitter" | "tree-sitter-python" => "medium",
        _ => "low",
    }
}

pub fn short_name(name: &str) -> String {
    name.replace("::", ".")
        .rsplit('.')
        .next()
        .unwrap_or(name)
        .to_string()
}

fn node_label(node: &GraphNode) -> String {
    format!(
        "{}:{}-{} ({}, {})",
        display_path(&node.path),
        node.start_line,
        node.end_line,
        node.kind,
        node.qualified_name
    )
}

fn strip_cmake_comment(line: &str) -> &str {
    line.split('#').next().unwrap_or(line)
}

fn is_common_call_token(name: &str) -> bool {
    matches!(
        name,
        "if" | "for"
            | "while"
            | "with"
            | "return"
            | "yield"
            | "match"
            | "switch"
            | "catch"
            | "sizeof"
            | "decltype"
            | "static_cast"
            | "reinterpret_cast"
            | "const_cast"
            | "dynamic_cast"
            | "super"
            | "print"
    )
}

pub fn occurrence_symbol(
    path: &Path,
    text: &str,
    language: Language,
    name: &str,
    line: usize,
) -> Symbol {
    Symbol::new(
        path.to_path_buf(),
        language,
        "lexical",
        SymbolKind::Reference,
        name,
        name,
        line,
        line,
        line_slice(text, line, line),
    )
}
