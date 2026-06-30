use std::path::Path;

use regex::Regex;

use crate::model::{Language, Symbol, SymbolKind, SymbolKindFilter};
use crate::workspace::line_slice;

#[derive(Clone, Debug)]
struct CmakeCommand {
    name: String,
    args: Vec<String>,
    raw_args: String,
    start_line: usize,
    end_line: usize,
    source: String,
}

pub fn symbols(
    path: &Path,
    text: &str,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
) -> Vec<Symbol> {
    let commands = parse_commands(text);
    let mut out = Vec::new();
    if crate::model::kind_matches(kind_filter, SymbolKind::Variable) {
        out.extend(variable_symbols(path, &commands, wanted));
    }
    if crate::model::kind_matches(kind_filter, SymbolKind::Block) {
        out.extend(block_symbols(path, text, &commands, wanted));
    }
    if crate::model::kind_matches(kind_filter, SymbolKind::Target) {
        out.extend(target_symbols(path, &commands, wanted));
    }
    out.sort_by_key(|symbol| {
        (
            symbol.start_line,
            symbol.end_line,
            symbol.qualified_name.clone(),
        )
    });
    out
}

pub fn references(path: &Path, text: &str, wanted: &str, max_matches: usize) -> Vec<Symbol> {
    let pattern = Regex::new(&format!(
        r"(?i)(^|[^A-Za-z0-9_])(?:\$\{{\s*{}\s*\}}|{})([^A-Za-z0-9_]|$)",
        regex::escape(wanted),
        regex::escape(wanted)
    ))
    .ok();
    let Some(pattern) = pattern else {
        return Vec::new();
    };
    text.lines()
        .enumerate()
        .filter(|(_, line)| pattern.is_match(strip_line_comment(line)))
        .take(max_matches)
        .map(|(idx, _)| {
            let line = idx + 1;
            Symbol::new(
                path.to_path_buf(),
                Language::Cmake,
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

fn variable_symbols(path: &Path, commands: &[CmakeCommand], wanted: Option<&str>) -> Vec<Symbol> {
    commands
        .iter()
        .filter_map(|command| variable_name(command).map(|name| (command, name)))
        .filter(|(_, name)| wanted.is_none_or(|wanted| cmake_name_matches(wanted, name)))
        .map(|(command, name)| {
            let mut symbol = Symbol::new(
                path.to_path_buf(),
                Language::Cmake,
                "lexical",
                SymbolKind::Variable,
                name.clone(),
                name,
                command.start_line,
                command.end_line,
                command.source.clone(),
            );
            symbol.detail = command.name.clone();
            symbol
        })
        .collect()
}

fn block_symbols(
    path: &Path,
    text: &str,
    commands: &[CmakeCommand],
    wanted: Option<&str>,
) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (idx, command) in commands.iter().enumerate() {
        if !is_block_start(&command.name) {
            continue;
        }
        let Some(end) = matching_block_end(commands, idx) else {
            continue;
        };
        let qualified = format!("{}({})", command.name, command.raw_args.trim());
        if wanted.is_some_and(|wanted| {
            !cmake_name_matches(wanted, &command.name)
                && !cmake_name_matches(wanted, &qualified)
                && !command
                    .args
                    .iter()
                    .any(|arg| cmake_name_matches(wanted, arg))
        }) {
            continue;
        }
        out.push(Symbol::new(
            path.to_path_buf(),
            Language::Cmake,
            "lexical",
            SymbolKind::Block,
            command.name.clone(),
            qualified,
            command.start_line,
            commands[end].end_line,
            line_slice(text, command.start_line, commands[end].end_line),
        ));
    }
    out
}

fn target_symbols(path: &Path, commands: &[CmakeCommand], wanted: Option<&str>) -> Vec<Symbol> {
    let mut out = Vec::new();
    for command in commands
        .iter()
        .filter(|command| is_target_definition(&command.name))
    {
        let Some(target) = command.args.first().cloned() else {
            continue;
        };
        if wanted.is_some_and(|wanted| !cmake_name_matches(wanted, &target)) {
            continue;
        }
        let related = related_target_commands(commands, &target);
        let start_line = related
            .iter()
            .map(|command| command.start_line)
            .min()
            .unwrap_or(command.start_line);
        let end_line = related
            .iter()
            .map(|command| command.end_line)
            .max()
            .unwrap_or(command.end_line);
        let source = related
            .iter()
            .map(|command| command.source.trim_end())
            .collect::<Vec<_>>()
            .join("\n\n")
            + "\n";
        let mut symbol = Symbol::new(
            path.to_path_buf(),
            Language::Cmake,
            "lexical",
            SymbolKind::Target,
            target.clone(),
            target,
            start_line,
            end_line,
            source,
        );
        symbol.detail = command.name.clone();
        out.push(symbol);
    }
    out
}

fn related_target_commands<'a>(
    commands: &'a [CmakeCommand],
    target: &str,
) -> Vec<&'a CmakeCommand> {
    commands
        .iter()
        .filter(|command| command_mentions_target(command, target))
        .collect()
}

fn command_mentions_target(command: &CmakeCommand, target: &str) -> bool {
    if command.args.is_empty() {
        return false;
    }
    if (is_target_definition(&command.name)
        || command.name.starts_with("target_")
        || command.name == "set_target_properties")
        && cmake_name_matches(&command.args[0], target)
    {
        return true;
    }
    if command.name == "add_custom_command"
        && command
            .args
            .first()
            .is_some_and(|arg| arg.eq_ignore_ascii_case("TARGET"))
        && command
            .args
            .get(1)
            .is_some_and(|arg| cmake_name_matches(arg, target))
    {
        return true;
    }
    if matches!(command.name.as_str(), "add_dependencies" | "set_property")
        && command
            .args
            .iter()
            .any(|arg| cmake_name_matches(arg, target))
    {
        return true;
    }
    (command.name == "install"
        && command
            .args
            .iter()
            .any(|arg| arg.eq_ignore_ascii_case("TARGETS"))
        && command
            .args
            .iter()
            .any(|arg| cmake_name_matches(arg, target)))
        || command_has_generator_target_reference(command, target)
}

fn variable_name(command: &CmakeCommand) -> Option<String> {
    match command.name.as_str() {
        "set" | "option" | "unset" => command.args.first().cloned(),
        "list" if command.args.len() >= 2 => {
            let mode = command.args[0].to_ascii_lowercase();
            matches!(
                mode.as_str(),
                "append"
                    | "prepend"
                    | "insert"
                    | "remove_item"
                    | "remove_at"
                    | "remove_duplicates"
                    | "filter"
                    | "sort"
                    | "reverse"
                    | "transform"
                    | "pop_back"
                    | "pop_front"
            )
            .then(|| command.args[1].clone())
        }
        _ => None,
    }
}

fn is_target_definition(name: &str) -> bool {
    matches!(
        name,
        "add_library" | "add_executable" | "pybind11_add_module"
    )
}

fn is_block_start(name: &str) -> bool {
    matches!(name, "if" | "foreach" | "function" | "macro" | "while")
}

fn block_end_for(name: &str) -> Option<&'static str> {
    match name {
        "if" => Some("endif"),
        "foreach" => Some("endforeach"),
        "function" => Some("endfunction"),
        "macro" => Some("endmacro"),
        "while" => Some("endwhile"),
        _ => None,
    }
}

fn matching_block_end(commands: &[CmakeCommand], start: usize) -> Option<usize> {
    let end_name = block_end_for(&commands[start].name)?;
    let mut depth = 0usize;
    for (idx, command) in commands.iter().enumerate().skip(start + 1) {
        if command.name == commands[start].name {
            depth += 1;
        } else if command.name == end_name {
            if depth == 0 {
                return Some(idx);
            }
            depth -= 1;
        }
    }
    None
}

fn cmake_name_matches(left: &str, right: &str) -> bool {
    normalize_cmake_name(left).eq_ignore_ascii_case(&normalize_cmake_name(right))
}

fn normalize_cmake_name(value: &str) -> String {
    let mut value = value.trim();
    while value.starts_with("${") && value.ends_with('}') && value.len() > 3 {
        value = &value[2..value.len() - 1];
    }
    value.trim().to_string()
}

fn command_has_generator_target_reference(command: &CmakeCommand, target: &str) -> bool {
    let normalized = regex::escape(&normalize_cmake_name(target));
    let pattern = Regex::new(&format!(
        r"\$<TARGET_[A-Za-z0-9_]+:\s*(?:\$\{{\s*{normalized}\s*\}}|{normalized})\s*>"
    ))
    .ok();
    pattern.is_some_and(|pattern| pattern.is_match(&command.raw_args))
}

fn parse_commands(text: &str) -> Vec<CmakeCommand> {
    let mut commands = Vec::new();
    let mut cursor = 0usize;
    while cursor < text.len() {
        let Some((name_start, name_end, name)) = next_command_name(text, cursor) else {
            break;
        };
        let mut open = name_end;
        while open < text.len() && text.as_bytes()[open].is_ascii_whitespace() {
            open += 1;
        }
        if text.as_bytes().get(open) != Some(&b'(') {
            cursor = name_end;
            continue;
        }
        let Some(close) = find_command_close(text, open) else {
            break;
        };
        let start_line = byte_line(text, name_start);
        let end_line = byte_line(text, close);
        let raw_args = text[open + 1..close].to_string();
        commands.push(CmakeCommand {
            name: name.to_ascii_lowercase(),
            args: parse_args(&raw_args),
            raw_args,
            start_line,
            end_line,
            source: line_slice(text, start_line, end_line),
        });
        cursor = close + 1;
    }
    commands
}

fn next_command_name(text: &str, start: usize) -> Option<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let mut idx = start;
    while idx < bytes.len() {
        if bytes[idx] == b'#' {
            idx = text[idx..]
                .find('\n')
                .map_or(bytes.len(), |offset| idx + offset + 1);
            continue;
        }
        if is_name_start(bytes[idx]) {
            let name_start = idx;
            idx += 1;
            while idx < bytes.len() && is_name_continue(bytes[idx]) {
                idx += 1;
            }
            return Some((name_start, idx, &text[name_start..idx]));
        }
        idx += 1;
    }
    None
}

fn find_command_close(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut idx = open + 1;
    let mut depth = 1usize;
    let mut in_quote = false;
    while idx < bytes.len() {
        let byte = bytes[idx];
        if in_quote {
            if byte == b'\\' {
                idx += 2;
                continue;
            }
            if byte == b'"' {
                in_quote = false;
            }
            idx += 1;
            continue;
        }
        match byte {
            b'"' => in_quote = true,
            b'#' => {
                idx = text[idx..]
                    .find('\n')
                    .map_or(bytes.len(), |offset| idx + offset);
                continue;
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

fn parse_args(raw: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = raw.chars().peekable();
    let mut in_quote = false;
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

fn strip_line_comment(line: &str) -> &str {
    let mut in_quote = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_quote => escaped = true,
            '"' => in_quote = !in_quote,
            '#' if !in_quote => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn byte_line(text: &str, byte: usize) -> usize {
    text[..byte].bytes().filter(|value| *value == b'\n').count() + 1
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_name_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiline_set_and_list_variables() {
        let commands = parse_commands(
            r#"
set(ITEMS
    one
    two
)
list(APPEND ITEMS three)
"#,
        );
        assert_eq!(commands.len(), 2);
        assert_eq!(variable_name(&commands[0]).as_deref(), Some("ITEMS"));
        assert_eq!(commands[0].start_line, 2);
        assert_eq!(commands[0].end_line, 5);
        assert_eq!(variable_name(&commands[1]).as_deref(), Some("ITEMS"));
    }

    #[test]
    fn finds_nested_block_end() {
        let commands = parse_commands(
            r#"
if(OUTER)
  if(INNER)
  endif()
endif()
"#,
        );
        assert_eq!(matching_block_end(&commands, 0), Some(3));
    }
}
