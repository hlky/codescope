use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::model::{Language, Symbol, SymbolKind, SymbolKindFilter, name_matches};
use crate::workspace::line_slice;

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
    visit_items(
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
    let short = wanted.rsplit("::").next().unwrap_or(wanted);
    let mut out = Vec::new();
    visit_all(tree.root_node(), &mut |node| {
        if out.len() >= max_matches || !is_reference_identifier(node) {
            return;
        }
        if node_text(node, text).is_some_and(|value| value == short) && !is_definition_name(node) {
            let start_line = node.start_position().row + 1;
            out.push(Symbol::new(
                path.to_path_buf(),
                Language::Rust,
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
    let short = wanted.rsplit("::").next().unwrap_or(wanted);
    symbols(path, text, Some(SymbolKindFilter::Function), None)
        .into_iter()
        .filter(|symbol| symbol.name != short && symbol.qualified_name != wanted)
        .filter(|symbol| contains_call(&symbol.source, short))
        .take(max_matches)
        .collect()
}

pub(crate) fn import_context(text: &str) -> String {
    let Some(tree) = parse(text) else {
        return String::new();
    };
    let mut context = Vec::new();
    collect_use_declarations(tree.root_node(), text, &mut context);

    let mut cursor = tree.root_node().walk();
    for node in tree.root_node().named_children(&mut cursor) {
        match node.kind() {
            "inner_attribute_item" | "extern_crate_declaration" => {
                if let Some(source) = node_text(node, text) {
                    context.push(source);
                }
            }
            "mod_item" => {
                if node.child_by_field_name("body").is_none()
                    && let Some(source) = node_text(node, text)
                {
                    context.push(source);
                }
            }
            _ => {}
        }
    }
    context.join("\n")
}

fn parse(text: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .ok()?;
    parser.parse(text, None)
}

fn collect_use_declarations(node: Node<'_>, text: &str, context: &mut Vec<String>) {
    if node.kind() == "use_declaration" {
        if let Some(source) = node_text(node, text) {
            context.push(source);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_use_declarations(child, text, context);
    }
}

#[allow(clippy::too_many_arguments)]
fn visit_items(
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
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "function_item" | "function_signature_item" => {
                add_named_symbol(
                    path,
                    text,
                    child,
                    prefix,
                    SymbolKind::Function,
                    kind_filter,
                    wanted,
                    out,
                );
                with_named_scope(child, text, prefix, |prefix| {
                    if let Some(body) = child.child_by_field_name("body") {
                        visit_items(path, text, body, prefix, true, kind_filter, wanted, out);
                    }
                });
            }
            "mod_item" => visit_named_container(
                path,
                text,
                child,
                prefix,
                SymbolKind::Module,
                kind_filter,
                wanted,
                out,
            ),
            "trait_item" => visit_named_container(
                path,
                text,
                child,
                prefix,
                SymbolKind::Trait,
                kind_filter,
                wanted,
                out,
            ),
            "impl_item" => {
                let scope = child
                    .child_by_field_name("type")
                    .and_then(|kind| node_text(kind, text))
                    .map(|kind| normalize_impl_type(&kind));
                if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
                    prefix.push(scope);
                    if let Some(body) = child.child_by_field_name("body") {
                        visit_items(path, text, body, prefix, false, kind_filter, wanted, out);
                    }
                    prefix.pop();
                }
            }
            "struct_item" => visit_named_container(
                path,
                text,
                child,
                prefix,
                SymbolKind::Struct,
                kind_filter,
                wanted,
                out,
            ),
            "enum_item" => add_named_symbol(
                path,
                text,
                child,
                prefix,
                SymbolKind::Enum,
                kind_filter,
                wanted,
                out,
            ),
            "union_item" => visit_named_container(
                path,
                text,
                child,
                prefix,
                SymbolKind::Union,
                kind_filter,
                wanted,
                out,
            ),
            "type_item" => add_named_symbol(
                path,
                text,
                child,
                prefix,
                SymbolKind::TypeAlias,
                kind_filter,
                wanted,
                out,
            ),
            "macro_definition" => add_named_symbol(
                path,
                text,
                child,
                prefix,
                SymbolKind::Macro,
                kind_filter,
                wanted,
                out,
            ),
            "const_item" | "static_item" | "field_declaration" => add_named_symbol(
                path,
                text,
                child,
                prefix,
                SymbolKind::Variable,
                kind_filter,
                wanted,
                out,
            ),
            "let_declaration" if include_locals => {
                add_let_symbols(path, text, child, prefix, kind_filter, wanted, out)
            }
            _ => visit_items(
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
fn visit_named_container(
    path: &Path,
    text: &str,
    node: Node<'_>,
    prefix: &mut Vec<String>,
    kind: SymbolKind,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    add_named_symbol(path, text, node, prefix, kind, kind_filter, wanted, out);
    with_named_scope(node, text, prefix, |prefix| {
        if let Some(body) = node.child_by_field_name("body") {
            visit_items(path, text, body, prefix, false, kind_filter, wanted, out);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn add_named_symbol(
    path: &Path,
    text: &str,
    node: Node<'_>,
    prefix: &[String],
    kind: SymbolKind,
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
    let qualified = qualify(prefix, &name);
    if wanted.is_some_and(|wanted| !name_matches(wanted, &name, &qualified, "::")) {
        return;
    }
    let start_line = source_start(node, text);
    let end_line = node.end_position().row + 1;
    out.push(Symbol::new(
        path.to_path_buf(),
        Language::Rust,
        "tree-sitter",
        kind,
        name,
        qualified,
        start_line,
        end_line,
        line_slice(text, start_line, end_line),
    ));
}

#[allow(clippy::too_many_arguments)]
fn add_let_symbols(
    path: &Path,
    text: &str,
    node: Node<'_>,
    prefix: &[String],
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    if !crate::model::kind_matches(kind_filter, SymbolKind::Variable) {
        return;
    }
    let pattern = node
        .child_by_field_name("pattern")
        .or_else(|| node.named_child(0));
    let Some(pattern) = pattern else {
        return;
    };
    let mut names = Vec::new();
    collect_pattern_names(pattern, text, &mut names);
    for name in names {
        let qualified = qualify(prefix, &name);
        if wanted.is_some_and(|wanted| !name_matches(wanted, &name, &qualified, "::")) {
            continue;
        }
        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;
        out.push(Symbol::new(
            path.to_path_buf(),
            Language::Rust,
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

fn collect_pattern_names(node: Node<'_>, text: &str, names: &mut Vec<String>) {
    if node.kind() == "identifier" {
        if let Some(name) = node_text(node, text) {
            names.push(name);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_pattern_names(child, text, names);
    }
}

fn with_named_scope(
    node: Node<'_>,
    text: &str,
    prefix: &mut Vec<String>,
    visit: impl FnOnce(&mut Vec<String>),
) {
    let Some(name) = child_name(node, text) else {
        return;
    };
    prefix.push(name);
    visit(prefix);
    prefix.pop();
}

fn child_name(node: Node<'_>, text: &str) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|name| node_text(name, text))
}

fn source_start(node: Node<'_>, text: &str) -> usize {
    let mut start = node.start_position().row + 1;
    let mut sibling = node.prev_named_sibling();
    while let Some(previous) = sibling {
        let include = match previous.kind() {
            "attribute_item" => true,
            "line_comment" | "block_comment" => node_text(previous, text)
                .is_some_and(|comment| comment.starts_with("///") || comment.starts_with("/**")),
            _ => false,
        };
        if !include {
            break;
        }
        start = previous.start_position().row + 1;
        sibling = previous.prev_named_sibling();
    }
    start
}

fn normalize_impl_type(value: &str) -> String {
    let mut normalized = String::new();
    let mut generic_depth = 0usize;
    for character in value.chars() {
        match character {
            '<' => generic_depth += 1,
            '>' => generic_depth = generic_depth.saturating_sub(1),
            _ if generic_depth == 0 && !character.is_whitespace() => normalized.push(character),
            _ => {}
        }
    }
    normalized
        .trim_start_matches('&')
        .trim_start_matches("mut")
        .trim()
        .to_string()
}

fn contains_call(text: &str, wanted: &str) -> bool {
    let Some(tree) = parse(text) else {
        return false;
    };
    let mut found = false;
    visit_all(tree.root_node(), &mut |node| {
        if found || node.kind() != "call_expression" {
            return;
        }
        found = node
            .child_by_field_name("function")
            .and_then(|function| terminal_identifier(function, text))
            .is_some_and(|name| name == wanted);
    });
    found
}

fn terminal_identifier(node: Node<'_>, text: &str) -> Option<String> {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier"
    ) {
        return node_text(node, text);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter_map(|child| terminal_identifier(child, text))
        .last()
}

fn is_reference_identifier(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier"
    )
}

fn is_definition_name(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    matches!(
        parent.kind(),
        "function_item"
            | "function_signature_item"
            | "struct_item"
            | "enum_item"
            | "union_item"
            | "trait_item"
            | "mod_item"
            | "type_item"
            | "macro_definition"
            | "const_item"
            | "static_item"
            | "field_declaration"
    ) && parent.child_by_field_name("name") == Some(node)
}

fn visit_all(node: Node<'_>, visit: &mut impl FnMut(Node<'_>)) {
    visit(node);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_all(child, visit);
    }
}

fn node_text(node: Node<'_>, text: &str) -> Option<String> {
    node.utf8_text(text.as_bytes()).ok().map(str::to_string)
}

fn qualify(prefix: &[String], name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{}::{name}", prefix.join("::"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_impl_type_removes_generic_arguments() {
        assert_eq!(normalize_impl_type("crate::Worker<T, U>"), "crate::Worker");
    }

    #[test]
    fn contains_call_matches_scoped_function_calls() {
        assert!(contains_call("fn caller() { crate::helper(); }", "helper"));
    }
}
