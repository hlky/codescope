use std::path::Path;

use tree_sitter::{Node, Parser, TreeCursor};

use crate::model::{Language, Symbol, SymbolKind, SymbolKindFilter, name_matches};
use crate::workspace::{line_for_byte, line_slice};

pub fn symbols(
    path: &Path,
    text: &str,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
) -> Vec<Symbol> {
    let Some(tree) = parse(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    visit_stmt_list(
        path,
        text,
        tree.root_node(),
        &mut Vec::new(),
        false,
        kind_filter,
        wanted,
        &mut out,
    );
    out
}

pub fn references(path: &Path, text: &str, wanted: &str, max_matches: usize) -> Vec<Symbol> {
    let Some(tree) = parse(text) else {
        return Vec::new();
    };
    let short = wanted
        .replace("::", ".")
        .rsplit('.')
        .next()
        .unwrap_or(wanted)
        .to_string();
    let mut out = Vec::new();
    visit_all(tree.root_node(), &mut |node| {
        if out.len() >= max_matches {
            return;
        }
        if matches!(node.kind(), "identifier" | "attribute")
            && node_text(node, text).is_some_and(|value| value == short)
        {
            let start_line = node.start_position().row + 1;
            out.push(Symbol::new(
                path.to_path_buf(),
                Language::Python,
                "tree-sitter",
                SymbolKind::Reference,
                wanted,
                wanted,
                start_line,
                start_line,
                line_slice(text, start_line, start_line),
            ));
        }
    });
    out
}

pub fn callers(path: &Path, text: &str, wanted: &str, max_matches: usize) -> Vec<Symbol> {
    let functions = symbols(path, text, Some(SymbolKindFilter::Function), None);
    let short = wanted
        .replace("::", ".")
        .rsplit('.')
        .next()
        .unwrap_or(wanted)
        .to_string();
    functions
        .into_iter()
        .filter(|symbol| symbol.name != short && symbol.qualified_name != wanted)
        .filter(|symbol| contains_python_call(&symbol.source, &short))
        .take(max_matches)
        .collect()
}

fn parse(text: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .ok()?;
    parser.parse(text, None)
}

#[allow(clippy::too_many_arguments)]
fn visit_stmt_list(
    path: &Path,
    text: &str,
    node: Node<'_>,
    prefix: &mut Vec<String>,
    include_locals: bool,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                add_def_symbol(
                    path,
                    text,
                    child,
                    prefix,
                    SymbolKind::Function,
                    "",
                    kind_filter,
                    wanted,
                    out,
                );
                if let Some(name) = child_name(child, text) {
                    prefix.push(name);
                    visit_body(path, text, child, prefix, true, kind_filter, wanted, out);
                    prefix.pop();
                }
            }
            "class_definition" => {
                add_def_symbol(
                    path,
                    text,
                    child,
                    prefix,
                    SymbolKind::Class,
                    "",
                    kind_filter,
                    wanted,
                    out,
                );
                if let Some(name) = child_name(child, text) {
                    prefix.push(name);
                    visit_body(path, text, child, prefix, false, kind_filter, wanted, out);
                    prefix.pop();
                }
            }
            "decorated_definition" => {
                if let Some(def) = named_children(child)
                    .into_iter()
                    .find(|n| matches!(n.kind(), "function_definition" | "class_definition"))
                {
                    match def.kind() {
                        "function_definition" => {
                            add_def_symbol(
                                path,
                                text,
                                def,
                                prefix,
                                SymbolKind::Function,
                                "",
                                kind_filter,
                                wanted,
                                out,
                            );
                            if let Some(name) = child_name(def, text) {
                                prefix.push(name);
                                visit_body(path, text, def, prefix, true, kind_filter, wanted, out);
                                prefix.pop();
                            }
                        }
                        "class_definition" => {
                            add_def_symbol(
                                path,
                                text,
                                def,
                                prefix,
                                SymbolKind::Class,
                                "",
                                kind_filter,
                                wanted,
                                out,
                            );
                            if let Some(name) = child_name(def, text) {
                                prefix.push(name);
                                visit_body(
                                    path,
                                    text,
                                    def,
                                    prefix,
                                    false,
                                    kind_filter,
                                    wanted,
                                    out,
                                );
                                prefix.pop();
                            }
                        }
                        _ => {}
                    }
                }
            }
            "assignment" | "augmented_assignment" | "type_alias_statement" => {
                add_assignments(
                    path,
                    text,
                    child,
                    prefix,
                    include_locals,
                    kind_filter,
                    wanted,
                    out,
                );
            }
            _ => visit_stmt_list(
                path,
                text,
                child,
                prefix,
                include_locals,
                kind_filter,
                wanted,
                out,
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn visit_body(
    path: &Path,
    text: &str,
    def: Node<'_>,
    prefix: &mut Vec<String>,
    include_locals: bool,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    if let Some(body) = def.child_by_field_name("body") {
        visit_stmt_list(
            path,
            text,
            body,
            prefix,
            include_locals,
            kind_filter,
            wanted,
            out,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn add_def_symbol(
    path: &Path,
    text: &str,
    node: Node<'_>,
    prefix: &[String],
    kind: SymbolKind,
    detail: &str,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    if !crate::model::kind_matches(kind_filter, kind) {
        return;
    }
    let Some(name) = child_name(node, text) else {
        return;
    };
    let qualified = qualify(prefix, &name, ".");
    if wanted.is_some_and(|wanted| !name_matches(wanted, &name, &qualified, ".")) {
        return;
    }
    let range_node = node
        .parent()
        .filter(|parent| parent.kind() == "decorated_definition")
        .unwrap_or(node);
    let start_line = range_node.start_position().row + 1;
    let end_line = range_node.end_position().row + 1;
    let mut symbol = Symbol::new(
        path.to_path_buf(),
        Language::Python,
        "tree-sitter",
        kind,
        name,
        qualified,
        start_line,
        end_line,
        line_slice(text, start_line, end_line),
    );
    symbol.detail = detail.to_string();
    out.push(symbol);
}

#[allow(clippy::too_many_arguments)]
fn add_assignments(
    path: &Path,
    text: &str,
    node: Node<'_>,
    prefix: &[String],
    include_locals: bool,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    if !crate::model::kind_matches(kind_filter, SymbolKind::Variable) {
        return;
    }
    let Some(left) = node
        .child_by_field_name("left")
        .or_else(|| node.named_child(0))
    else {
        return;
    };
    let mut names = Vec::new();
    collect_assignment_names(left, text, &mut names);
    for name in names {
        let qualified = if include_locals || !prefix.is_empty() {
            qualify(prefix, &name, ".")
        } else {
            name.clone()
        };
        if wanted.is_some_and(|wanted| !name_matches(wanted, &name, &qualified, ".")) {
            continue;
        }
        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;
        out.push(Symbol::new(
            path.to_path_buf(),
            Language::Python,
            "tree-sitter",
            SymbolKind::Variable,
            name,
            qualified,
            start_line,
            end_line,
            line_slice(text, start_line, end_line),
        ));
    }
}

fn collect_assignment_names(node: Node<'_>, text: &str, names: &mut Vec<String>) {
    match node.kind() {
        "identifier" => {
            if let Some(value) = node_text(node, text) {
                names.push(value);
            }
        }
        "attribute" => {
            if let Some(attr) = node
                .child_by_field_name("attribute")
                .and_then(|n| node_text(n, text))
            {
                names.push(attr);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_assignment_names(child, text, names);
            }
        }
    }
}

fn child_name(node: Node<'_>, text: &str) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|name| node_text(name, text))
}

fn node_text(node: Node<'_>, text: &str) -> Option<String> {
    node.utf8_text(text.as_bytes()).ok().map(str::to_string)
}

fn qualify(prefix: &[String], name: &str, sep: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{}{}{}", prefix.join(sep), sep, name)
    }
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn visit_all(node: Node<'_>, visitor: &mut impl FnMut(Node<'_>)) {
    visitor(node);
    let mut cursor: TreeCursor<'_> = node.walk();
    for child in node.children(&mut cursor) {
        visit_all(child, visitor);
    }
}

fn contains_python_call(source: &str, name: &str) -> bool {
    let pattern =
        regex::Regex::new(&format!(r"(^|[^A-Za-z0-9_]){}\s*\(", regex::escape(name))).ok();
    pattern.is_some_and(|pattern| pattern.is_match(source))
}

#[allow(dead_code)]
fn line_for_node_start(text: &str, node: Node<'_>) -> usize {
    line_for_byte(text, node.start_byte())
}
