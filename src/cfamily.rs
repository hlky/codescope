use std::path::Path;

use regex::Regex;
use tree_sitter::{Node, Parser};

use crate::model::{Backend, Language, Symbol, SymbolKind, SymbolKindFilter, name_matches};
use crate::workspace::{language_for_path, line_slice};

pub fn symbols(
    path: &Path,
    text: &str,
    backend: Backend,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
) -> anyhow::Result<Vec<Symbol>> {
    match backend {
        Backend::Auto | Backend::Lsp | Backend::TreeSitter => {
            Ok(tree_sitter_symbols(path, text, kind_filter, wanted))
        }
        Backend::Lexical => Ok(lexical_symbols(path, text, kind_filter, wanted)),
    }
}

pub fn references(path: &Path, text: &str, wanted: &str, max_matches: usize) -> Vec<Symbol> {
    let Some(language) = language_for_path(path) else {
        return Vec::new();
    };
    let masked = mask_comments_and_strings(text);
    let short = wanted
        .replace('.', "::")
        .rsplit("::")
        .next()
        .unwrap_or(wanted)
        .to_string();
    let pattern = Regex::new(&format!(
        r"(^|[^A-Za-z0-9_]){}([^A-Za-z0-9_]|$)",
        regex::escape(&short)
    ))
    .ok();
    let Some(pattern) = pattern else {
        return Vec::new();
    };
    masked
        .lines()
        .enumerate()
        .filter(|(_, line)| pattern.is_match(line))
        .take(max_matches)
        .map(|(idx, _)| {
            let line = idx + 1;
            Symbol::new(
                path.to_path_buf(),
                language,
                "lexical",
                SymbolKind::Reference,
                wanted,
                wanted,
                line,
                line,
                line_slice(text, line, line),
            )
        })
        .collect()
}

pub fn callers(
    path: &Path,
    text: &str,
    backend: Backend,
    wanted: &str,
    max_matches: usize,
) -> anyhow::Result<Vec<Symbol>> {
    let short = wanted
        .replace('.', "::")
        .rsplit("::")
        .next()
        .unwrap_or(wanted)
        .to_string();
    let pattern = Regex::new(&format!(r"(^|[^A-Za-z0-9_]){}\s*\(", regex::escape(&short)))?;
    Ok(
        symbols(path, text, backend, Some(SymbolKindFilter::Function), None)?
            .into_iter()
            .filter(|symbol| symbol.name != short && symbol.qualified_name != wanted)
            .filter(|symbol| pattern.is_match(&symbol.source))
            .take(max_matches)
            .collect(),
    )
}

fn tree_sitter_symbols(
    path: &Path,
    text: &str,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
) -> Vec<Symbol> {
    let Some(language) = language_for_path(path) else {
        return Vec::new();
    };
    let Some(tree) = parse(path, text) else {
        return lexical_symbols(path, text, kind_filter, wanted);
    };
    let mut out = Vec::new();
    visit(
        path,
        text,
        language,
        tree.root_node(),
        &mut Vec::new(),
        kind_filter,
        wanted,
        &mut out,
    );
    out.sort_by_key(|symbol| {
        (
            symbol.start_line,
            symbol.end_line,
            symbol.qualified_name.clone(),
        )
    });
    out
}

fn parse(path: &Path, text: &str) -> Option<tree_sitter::Tree> {
    let language = match language_for_path(path)? {
        Language::C => tree_sitter_c::LANGUAGE.into(),
        Language::Cpp | Language::Cuda | Language::Hip => tree_sitter_cpp::LANGUAGE.into(),
        _ => return None,
    };
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    parser.parse(text, None)
}

#[allow(clippy::too_many_arguments)]
fn visit(
    path: &Path,
    text: &str,
    language: Language,
    node: Node<'_>,
    scope: &mut Vec<String>,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    match node.kind() {
        "namespace_definition" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|n| node_text(n, text))
            {
                scope.push(name);
                visit_children(path, text, language, node, scope, kind_filter, wanted, out);
                scope.pop();
                return;
            }
        }
        "function_definition" => {
            add_function(path, text, language, node, scope, kind_filter, wanted, out)
        }
        "class_specifier" => add_type(
            path,
            text,
            language,
            node,
            scope,
            SymbolKind::Class,
            kind_filter,
            wanted,
            out,
        ),
        "struct_specifier" => add_type(
            path,
            text,
            language,
            node,
            scope,
            SymbolKind::Struct,
            kind_filter,
            wanted,
            out,
        ),
        "enum_specifier" => add_type(
            path,
            text,
            language,
            node,
            scope,
            SymbolKind::Enum,
            kind_filter,
            wanted,
            out,
        ),
        "declaration" | "field_declaration" => {
            add_variable(path, text, language, node, scope, kind_filter, wanted, out)
        }
        _ => {}
    }
    if matches!(node.kind(), "class_specifier" | "struct_specifier")
        && let Some(name) = node
            .child_by_field_name("name")
            .and_then(|n| node_text(n, text))
    {
        scope.push(name);
        visit_children(path, text, language, node, scope, kind_filter, wanted, out);
        scope.pop();
        return;
    }
    visit_children(path, text, language, node, scope, kind_filter, wanted, out);
}

#[allow(clippy::too_many_arguments)]
fn visit_children(
    path: &Path,
    text: &str,
    language: Language,
    node: Node<'_>,
    scope: &mut Vec<String>,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit(path, text, language, child, scope, kind_filter, wanted, out);
    }
}

#[allow(clippy::too_many_arguments)]
fn add_function(
    path: &Path,
    text: &str,
    language: Language,
    node: Node<'_>,
    scope: &[String],
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    if !crate::model::kind_matches(kind_filter, SymbolKind::Function) {
        return;
    }
    let declarator = node.child_by_field_name("declarator").unwrap_or(node);
    let Some((name, mut qualified)) = function_name(declarator, text) else {
        return;
    };
    if !scope.is_empty() && !qualified.contains("::") {
        qualified = format!("{}::{qualified}", scope.join("::"));
    }
    if wanted
        .is_some_and(|wanted| !name_matches(&wanted.replace('.', "::"), &name, &qualified, "::"))
    {
        return;
    }
    push_symbol(
        path,
        text,
        language,
        node,
        SymbolKind::Function,
        name,
        qualified,
        out,
    );
}

#[allow(clippy::too_many_arguments)]
fn add_type(
    path: &Path,
    text: &str,
    language: Language,
    node: Node<'_>,
    scope: &[String],
    kind: SymbolKind,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    if !crate::model::kind_matches(kind_filter, kind) {
        return;
    }
    let Some(name) = node
        .child_by_field_name("name")
        .and_then(|n| node_text(n, text))
    else {
        return;
    };
    let qualified = if scope.is_empty() {
        name.clone()
    } else {
        format!("{}::{name}", scope.join("::"))
    };
    if wanted
        .is_some_and(|wanted| !name_matches(&wanted.replace('.', "::"), &name, &qualified, "::"))
    {
        return;
    }
    push_symbol(path, text, language, node, kind, name, qualified, out);
}

#[allow(clippy::too_many_arguments)]
fn add_variable(
    path: &Path,
    text: &str,
    language: Language,
    node: Node<'_>,
    scope: &[String],
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    if !crate::model::kind_matches(kind_filter, SymbolKind::Variable) {
        return;
    }
    if descendant_kind(node, "function_declarator")
        || descendant_kind(node, "parameter_declaration")
    {
        return;
    }
    let Some(name) = first_identifier(node, text) else {
        return;
    };
    let qualified = if scope.is_empty() {
        name.clone()
    } else {
        format!("{}::{name}", scope.join("::"))
    };
    if wanted
        .is_some_and(|wanted| !name_matches(&wanted.replace('.', "::"), &name, &qualified, "::"))
    {
        return;
    }
    push_symbol(
        path,
        text,
        language,
        node,
        SymbolKind::Variable,
        name.clone(),
        qualified,
        out,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_symbol(
    path: &Path,
    text: &str,
    language: Language,
    node: Node<'_>,
    kind: SymbolKind,
    name: String,
    qualified: String,
    out: &mut Vec<Symbol>,
) {
    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    out.push(Symbol::new(
        path.to_path_buf(),
        language,
        "tree-sitter",
        kind,
        name,
        qualified,
        start_line,
        end_line,
        line_slice(text, start_line, end_line),
    ));
}

fn function_name(node: Node<'_>, text: &str) -> Option<(String, String)> {
    let mut names = Vec::new();
    collect_name_nodes(node, text, &mut names);
    let qualified = names.last()?.clone();
    let name = qualified
        .split("::")
        .last()
        .unwrap_or(&qualified)
        .trim_start_matches('~')
        .to_string();
    Some((name, qualified))
}

fn collect_name_nodes(node: Node<'_>, text: &str, names: &mut Vec<String>) {
    if matches!(node.kind(), "parameter_list" | "compound_statement") {
        return;
    }
    match node.kind() {
        "identifier"
        | "field_identifier"
        | "qualified_identifier"
        | "destructor_name"
        | "operator_name" => {
            if let Some(value) = node_text(node, text) {
                names.push(value.replace(' ', ""));
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_name_nodes(child, text, names);
            }
        }
    }
}

fn first_identifier(node: Node<'_>, text: &str) -> Option<String> {
    if matches!(node.kind(), "identifier" | "field_identifier") {
        return node_text(node, text);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(value) = first_identifier(child, text) {
            return Some(value);
        }
    }
    None
}

fn descendant_kind(node: Node<'_>, kind: &str) -> bool {
    if node.kind() == kind {
        return true;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| descendant_kind(child, kind))
}

fn node_text(node: Node<'_>, text: &str) -> Option<String> {
    node.utf8_text(text.as_bytes()).ok().map(str::to_string)
}

fn lexical_symbols(
    path: &Path,
    text: &str,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
) -> Vec<Symbol> {
    let Some(language) = language_for_path(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if crate::model::kind_matches(kind_filter, SymbolKind::Function) {
        out.extend(lexical_functions(path, text, language, wanted));
    }
    if kind_filter.is_none()
        || matches!(
            kind_filter,
            Some(
                SymbolKindFilter::All
                    | SymbolKindFilter::Class
                    | SymbolKindFilter::Struct
                    | SymbolKindFilter::Enum
            )
        )
    {
        out.extend(lexical_types(path, text, language, kind_filter, wanted));
    }
    if crate::model::kind_matches(kind_filter, SymbolKind::Variable) {
        out.extend(lexical_variables(path, text, language, wanted));
    }
    out
}

fn lexical_functions(
    path: &Path,
    text: &str,
    language: Language,
    wanted: Option<&str>,
) -> Vec<Symbol> {
    let masked = mask_comments_and_strings(text);
    let name_re =
        Regex::new(r"(?m)(~?[A-Za-z_]\w*(?:::[~A-Za-z_]\w*)*|operator\s*[^\s(]+)\s*\(").unwrap();
    let mut out = Vec::new();
    for mat in name_re.find_iter(&masked) {
        let found = mat.as_str().trim_end_matches('(').trim().replace(' ', "");
        let short = found
            .split("::")
            .last()
            .unwrap_or(&found)
            .trim_start_matches('~')
            .to_string();
        if wanted
            .is_some_and(|wanted| !name_matches(&wanted.replace('.', "::"), &short, &found, "::"))
        {
            continue;
        }
        let Some(close_paren) = find_matching(&masked, mat.end() - 1, b'(', b')') else {
            continue;
        };
        let mut cursor = close_paren + 1;
        while cursor < masked.len() && masked.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= masked.len() || masked.as_bytes()[cursor] != b'{' {
            continue;
        }
        let Some(end_brace) = find_matching(&masked, cursor, b'{', b'}') else {
            continue;
        };
        let start = masked[..mat.start()].rfind('\n').map_or(0, |idx| idx + 1);
        let start_line = text[..start].bytes().filter(|b| *b == b'\n').count() + 1;
        let end_line = text[..end_brace].bytes().filter(|b| *b == b'\n').count() + 1;
        out.push(Symbol::new(
            path.to_path_buf(),
            language,
            "lexical",
            SymbolKind::Function,
            short,
            found,
            start_line,
            end_line,
            line_slice(text, start_line, end_line),
        ));
    }
    out
}

fn lexical_types(
    path: &Path,
    text: &str,
    language: Language,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
) -> Vec<Symbol> {
    let masked = mask_comments_and_strings(text);
    let type_re =
        Regex::new(r"\b(class|struct|enum(?:\s+class)?)\s+([A-Za-z_]\w*)[^;{]*\{").unwrap();
    let mut out = Vec::new();
    for captures in type_re.captures_iter(&masked) {
        let Some(full) = captures.get(0) else {
            continue;
        };
        let raw_kind = captures.get(1).map(|m| m.as_str()).unwrap_or("");
        let kind = if raw_kind.starts_with("enum") {
            SymbolKind::Enum
        } else if raw_kind == "struct" {
            SymbolKind::Struct
        } else {
            SymbolKind::Class
        };
        if !crate::model::kind_matches(kind_filter, kind) {
            continue;
        }
        let name = captures
            .get(2)
            .map(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        if wanted
            .is_some_and(|wanted| !name_matches(&wanted.replace('.', "::"), &name, &name, "::"))
        {
            continue;
        }
        let Some(open_brace) = masked[full.start()..full.end()]
            .rfind('{')
            .map(|idx| full.start() + idx)
        else {
            continue;
        };
        let Some(end_brace) = find_matching(&masked, open_brace, b'{', b'}') else {
            continue;
        };
        let end = masked[end_brace..]
            .find(';')
            .map(|idx| end_brace + idx)
            .unwrap_or(end_brace);
        let start_line = text[..full.start()].bytes().filter(|b| *b == b'\n').count() + 1;
        let end_line = text[..end].bytes().filter(|b| *b == b'\n').count() + 1;
        out.push(Symbol::new(
            path.to_path_buf(),
            language,
            "lexical",
            kind,
            name.clone(),
            name,
            start_line,
            end_line,
            line_slice(text, start_line, end_line),
        ));
    }
    out
}

fn lexical_variables(
    path: &Path,
    text: &str,
    language: Language,
    wanted: Option<&str>,
) -> Vec<Symbol> {
    let masked = mask_comments_and_strings(text);
    let declaration_re = Regex::new(
        r"(?m)^\s*(?:static\s+|extern\s+|constexpr\s+|const\s+|volatile\s+|__device__\s+|__constant__\s+)*[A-Za-z_][\w:<>,\s*&]*\s+([A-Za-z_]\w*)\s*(?:\[[^\]]*\])?\s*(?:=[^;]*)?;",
    )
    .unwrap();
    let mut out = Vec::new();
    for captures in declaration_re.captures_iter(&masked) {
        let Some(full) = captures.get(0) else {
            continue;
        };
        let line = &masked[full.start()..full.end()];
        let stripped = line.trim_start();
        if line.contains('(')
            || line.contains(')')
            || stripped.starts_with('#')
            || stripped.starts_with("return")
            || stripped.starts_with("using")
            || stripped.starts_with("namespace")
            || stripped.starts_with("typedef")
        {
            continue;
        }
        let name = captures
            .get(1)
            .map(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        if wanted
            .is_some_and(|wanted| !name_matches(&wanted.replace('.', "::"), &name, &name, "::"))
        {
            continue;
        }
        let start_line = text[..full.start()].bytes().filter(|b| *b == b'\n').count() + 1;
        let end_line = text[..full.end()].bytes().filter(|b| *b == b'\n').count() + 1;
        out.push(Symbol::new(
            path.to_path_buf(),
            language,
            "lexical",
            SymbolKind::Variable,
            name.clone(),
            name,
            start_line,
            end_line,
            line_slice(text, start_line, end_line),
        ));
    }
    out
}

fn find_matching(text: &str, start: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, byte) in text.bytes().enumerate().skip(start) {
        if byte == open {
            depth += 1;
        } else if byte == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

pub fn mask_comments_and_strings(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut state = "code";
    while i < chars.len() {
        let c = chars[i];
        let n = chars.get(i + 1).copied().unwrap_or('\0');
        match state {
            "code" if c == '/' && n == '/' => {
                chars[i] = ' ';
                chars[i + 1] = ' ';
                i += 2;
                state = "line";
            }
            "code" if c == '/' && n == '*' => {
                chars[i] = ' ';
                chars[i + 1] = ' ';
                i += 2;
                state = "block";
            }
            "code" if c == '"' => {
                chars[i] = ' ';
                i += 1;
                state = "string";
            }
            "code" if c == '\'' => {
                chars[i] = ' ';
                i += 1;
                state = "char";
            }
            "code" => i += 1,
            "line" if c == '\n' => {
                state = "code";
                i += 1;
            }
            "line" => {
                chars[i] = ' ';
                i += 1;
            }
            "block" if c == '*' && n == '/' => {
                chars[i] = ' ';
                chars[i + 1] = ' ';
                i += 2;
                state = "code";
            }
            "block" => {
                if c != '\n' {
                    chars[i] = ' ';
                }
                i += 1;
            }
            "string" | "char" => {
                let quote = if state == "string" { '"' } else { '\'' };
                if c == '\\' {
                    chars[i] = ' ';
                    if i + 1 < chars.len() && chars[i + 1] != '\n' {
                        chars[i + 1] = ' ';
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else if c == quote {
                    chars[i] = ' ';
                    i += 1;
                    state = "code";
                } else {
                    if c != '\n' {
                        chars[i] = ' ';
                    }
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    chars.into_iter().collect()
}
