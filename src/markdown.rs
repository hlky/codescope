use std::path::Path;

use tree_sitter::{Node, Parser, TreeCursor};

use crate::model::{Language, Symbol, SymbolKind};
use crate::workspace::line_slice;

#[derive(Clone, Debug)]
struct Heading {
    level: usize,
    title: String,
    start_line: usize,
    heading_end_line: usize,
}

pub fn headings(path: &Path, text: &str, wanted: Option<&str>) -> Vec<Symbol> {
    let mut headings = raw_headings(text);
    headings.sort_by_key(|heading| heading.start_line);
    let line_count = text.lines().count().max(1);
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();

    for (idx, heading) in headings.iter().enumerate() {
        while stack
            .last()
            .is_some_and(|(level, _)| *level >= heading.level)
        {
            stack.pop();
        }
        let mut qualified_parts = stack
            .iter()
            .map(|(_, title)| title.clone())
            .collect::<Vec<_>>();
        qualified_parts.push(heading.title.clone());
        let qualified = qualified_parts.join(".");
        stack.push((heading.level, heading.title.clone()));

        if wanted.is_some_and(|wanted| !heading_matches(wanted, &heading.title, &qualified)) {
            continue;
        }

        let end_line = headings
            .iter()
            .skip(idx + 1)
            .find(|next| next.level <= heading.level)
            .map(|next| next.start_line.saturating_sub(1))
            .unwrap_or(line_count);
        let mut symbol = Symbol::new(
            path.to_path_buf(),
            Language::Markdown,
            "tree-sitter",
            SymbolKind::Heading,
            heading.title.clone(),
            qualified,
            heading.start_line,
            end_line.max(heading.heading_end_line),
            line_slice(
                text,
                heading.start_line,
                end_line.max(heading.heading_end_line),
            ),
        );
        symbol.detail = format!("level={}", heading.level);
        out.push(symbol);
    }
    out
}

fn raw_headings(text: &str) -> Vec<Heading> {
    let Some(tree) = parse(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    visit_all(tree.root_node(), &mut |node| {
        if matches!(node.kind(), "atx_heading" | "setext_heading")
            && let Some(heading) = heading_from_node(node, text)
        {
            out.push(heading);
        }
    });
    out
}

fn parse(text: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_md_025::LANGUAGE.into())
        .ok()?;
    parser.parse(text, None)
}

fn heading_from_node(node: Node<'_>, text: &str) -> Option<Heading> {
    let level = match node.kind() {
        "atx_heading" => atx_level(node)?,
        "setext_heading" => setext_level(node)?,
        _ => return None,
    };
    let title = node
        .child_by_field_name("heading_content")
        .and_then(|content| node_text(content, text))
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())?;
    Some(Heading {
        level,
        title,
        start_line: node.start_position().row + 1,
        heading_end_line: node.end_position().row + 1,
    })
}

fn atx_level(node: Node<'_>) -> Option<usize> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find_map(|child| {
        child
            .kind()
            .strip_prefix("atx_h")
            .and_then(|value| value.strip_suffix("_marker"))
            .and_then(|value| value.parse::<usize>().ok())
    })
}

fn setext_level(node: Node<'_>) -> Option<usize> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find_map(|child| match child.kind() {
            "setext_h1_underline" => Some(1),
            "setext_h2_underline" => Some(2),
            _ => None,
        })
}

fn heading_matches(wanted: &str, title: &str, qualified: &str) -> bool {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return false;
    }
    title == wanted || qualified == wanted || qualified.ends_with(&format!(".{wanted}"))
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn headings_ignore_fenced_code_and_extract_sections() {
        let text = r#"# Intro
top

```markdown
# Not a heading
```

## Usage
body

### Details
more

## API
next
"#;
        let symbols = headings(Path::new("README.md"), text, None);
        assert_eq!(symbols.len(), 4);
        assert_eq!(symbols[1].qualified_name, "Intro.Usage");
        assert!(symbols[1].source.contains("### Details"));
        assert!(!symbols[1].source.contains("## API"));
    }

    #[test]
    fn heading_lookup_accepts_qualified_suffix() {
        let text = "# Intro\n\n## Usage\nbody\n";
        let symbols = headings(Path::new("README.md"), text, Some("Intro.Usage"));
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Usage");
    }
}
