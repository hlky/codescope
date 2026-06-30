use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, anyhow};
use serde_json::{Value, json};
use url::Url;

use crate::diagnostics::{DiagnosticRecord, DiagnosticSeverity, RelatedDiagnostic};
use crate::model::{Language, Symbol, SymbolKind, SymbolKindFilter, kind_matches, name_matches};
use crate::workspace::{language_for_path, line_slice, read_text};

pub struct ClangdOptions {
    pub root: PathBuf,
    pub compile_commands_dir: Option<PathBuf>,
}

pub fn clangd_available() -> bool {
    which::which("clangd").is_ok()
}

pub fn document_symbols(
    files: &[(PathBuf, String)],
    options: &ClangdOptions,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    max_matches: usize,
) -> anyhow::Result<Vec<Symbol>> {
    let mut client = ClangdClient::start(options)?;
    let mut out: Vec<Symbol> = Vec::new();
    for (path, text) in files {
        let file_symbols = client.document_symbols_for_file(path, text, kind_filter, wanted)?;
        out.extend(file_symbols);
        if out.len() >= max_matches {
            break;
        }
    }
    client.shutdown();
    out.truncate(max_matches);
    Ok(out)
}

pub fn references(
    files: &[(PathBuf, String)],
    options: &ClangdOptions,
    wanted: &str,
    max_matches: usize,
) -> anyhow::Result<Vec<Symbol>> {
    let mut client = ClangdClient::start(options)?;
    let mut definition_position = None;
    for (path, text) in files {
        let positions = client.symbol_positions_for_file(path, text, wanted)?;
        if let Some(position) = positions.into_iter().next() {
            definition_position = Some(position);
            break;
        }
    }

    let mut out: Vec<Symbol> = Vec::new();
    if let Some((uri, line, character)) = definition_position {
        let result = client.request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": true }
            }),
        )?;
        if let Some(locations) = result.as_array() {
            for location in locations.iter().take(max_matches) {
                let Some(uri) = location.get("uri").and_then(Value::as_str) else {
                    continue;
                };
                let Some(path) = path_from_uri(uri) else {
                    continue;
                };
                let Some(range) = location.get("range") else {
                    continue;
                };
                let start_line = range
                    .pointer("/start/line")
                    .and_then(Value::as_u64)
                    .map(|line| line as usize + 1)
                    .unwrap_or(1);
                let end_line = range
                    .pointer("/end/line")
                    .and_then(Value::as_u64)
                    .map(|line| line as usize + 1)
                    .unwrap_or(start_line);
                let text = read_text(&path).unwrap_or_default();
                out.push(Symbol::new(
                    path.clone(),
                    language_for_path(&path).unwrap_or(Language::Text),
                    "clangd",
                    SymbolKind::Reference,
                    wanted,
                    wanted,
                    start_line,
                    end_line,
                    line_slice(&text, start_line, start_line),
                ));
            }
        }
    }
    client.shutdown();
    Ok(out)
}

pub fn callers(
    files: &[(PathBuf, String)],
    options: &ClangdOptions,
    wanted: &str,
    max_matches: usize,
) -> anyhow::Result<Vec<Symbol>> {
    let mut client = ClangdClient::start(options)?;
    let mut definition_position = None;
    for (path, text) in files {
        let positions = client.symbol_positions_for_file(path, text, wanted)?;
        if let Some(position) = positions.into_iter().next() {
            definition_position = Some(position);
            break;
        }
    }

    let mut out: Vec<Symbol> = Vec::new();
    if let Some((uri, line, character)) = definition_position {
        let prepared = client.request(
            "textDocument/prepareCallHierarchy",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )?;
        if let Some(items) = prepared.as_array() {
            for item in items {
                let incoming =
                    client.request("callHierarchy/incomingCalls", json!({ "item": item }))?;
                let Some(calls) = incoming.as_array() else {
                    continue;
                };
                for call in calls {
                    let Some(from) = call.get("from") else {
                        continue;
                    };
                    let Some(symbol) = call_hierarchy_item_to_symbol(from) else {
                        continue;
                    };
                    if !out.iter().any(|existing| {
                        existing.path == symbol.path
                            && existing.qualified_name == symbol.qualified_name
                            && existing.start_line == symbol.start_line
                    }) {
                        out.push(symbol);
                    }
                    if out.len() >= max_matches {
                        client.shutdown();
                        return Ok(out);
                    }
                }
            }
        }
    }
    client.shutdown();
    Ok(out)
}

pub fn diagnostics(
    files: &[(PathBuf, String)],
    options: &ClangdOptions,
    max_matches: usize,
) -> anyhow::Result<Vec<DiagnosticRecord>> {
    let mut client = ClangdClient::start(options)?;
    let mut out = Vec::new();
    for (path, text) in files {
        let mut file_records = client.diagnostics_for_file(path, text)?;
        out.append(&mut file_records);
        if out.len() >= max_matches {
            break;
        }
    }
    client.shutdown();
    out.truncate(max_matches);
    Ok(out)
}

struct ClangdClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl ClangdClient {
    fn start(options: &ClangdOptions) -> anyhow::Result<Self> {
        let clangd = which::which("clangd").context("clangd was not found on PATH")?;
        let mut command = Command::new(clangd);
        command.arg("--background-index").arg("--log=error");
        if let Some(dir) = &options.compile_commands_dir {
            command.arg(format!("--compile-commands-dir={}", dir.display()));
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to start clangd")?;
        let stdin = child
            .stdin
            .take()
            .context("failed to capture clangd stdin")?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .context("failed to capture clangd stdout")?,
        );
        let mut client = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        client.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": uri_for_path(&options.root)?,
                "capabilities": {
                    "textDocument": {
                        "documentSymbol": { "hierarchicalDocumentSymbolSupport": true }
                    }
                }
            }),
        )?;
        client.notify("initialized", json!({}))?;
        Ok(client)
    }

    fn document_symbols_for_file(
        &mut self,
        path: &Path,
        text: &str,
        kind_filter: Option<SymbolKindFilter>,
        wanted: Option<&str>,
    ) -> anyhow::Result<Vec<Symbol>> {
        let uri = uri_for_path(path)?;
        self.open(path, text, &uri)?;
        let result = self.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
        )?;
        let mut out = Vec::new();
        if let Some(symbols) = result.as_array() {
            if symbols.first().and_then(|v| v.get("location")).is_some() {
                self.visit_symbol_information(path, text, symbols, kind_filter, wanted, &mut out);
            } else {
                self.visit_document_symbols(
                    path,
                    text,
                    symbols,
                    &mut Vec::new(),
                    kind_filter,
                    wanted,
                    &mut out,
                );
            }
        }
        self.close(&uri)?;
        Ok(out)
    }

    fn symbol_positions_for_file(
        &mut self,
        path: &Path,
        text: &str,
        wanted: &str,
    ) -> anyhow::Result<Vec<(String, usize, usize)>> {
        let uri = uri_for_path(path)?;
        self.open(path, text, &uri)?;
        let result = self.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
        )?;
        let mut out = Vec::new();
        if let Some(symbols) = result.as_array() {
            self.visit_positions(&uri, symbols, &mut Vec::new(), wanted, &mut out);
        }
        Ok(out)
    }

    fn diagnostics_for_file(
        &mut self,
        path: &Path,
        text: &str,
    ) -> anyhow::Result<Vec<DiagnosticRecord>> {
        let uri = uri_for_path(path)?;
        self.open(path, text, &uri)?;
        let result = self.request_collecting_diagnostics(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
            &uri,
        )?;
        self.close(&uri)?;
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_document_symbols(
        &self,
        path: &Path,
        text: &str,
        symbols: &[Value],
        prefix: &mut Vec<String>,
        kind_filter: Option<SymbolKindFilter>,
        wanted: Option<&str>,
        out: &mut Vec<Symbol>,
    ) {
        for raw in symbols {
            let Some(name) = raw
                .get("name")
                .and_then(Value::as_str)
                .map(clean_symbol_name)
            else {
                continue;
            };
            let qualified = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}::{name}", prefix.join("::"))
            };
            if let Some(kind) = raw.get("kind").and_then(Value::as_u64).and_then(lsp_kind) {
                let short = symbol_short_name(&name);
                if kind_matches(kind_filter, kind)
                    && wanted.is_none_or(|wanted| {
                        name_matches(&wanted.replace('.', "::"), &short, &qualified, "::")
                    })
                    && let Some(symbol) =
                        symbol_from_lsp(path, text, raw, kind, short, qualified.clone())
                {
                    out.push(symbol);
                }
            }
            let container = raw
                .get("kind")
                .and_then(Value::as_u64)
                .is_some_and(|kind| matches!(kind, 3 | 5 | 10 | 23));
            if container {
                prefix.push(name);
            }
            if let Some(children) = raw.get("children").and_then(Value::as_array) {
                self.visit_document_symbols(path, text, children, prefix, kind_filter, wanted, out);
            }
            if container {
                prefix.pop();
            }
        }
    }

    fn visit_symbol_information(
        &self,
        path: &Path,
        text: &str,
        symbols: &[Value],
        kind_filter: Option<SymbolKindFilter>,
        wanted: Option<&str>,
        out: &mut Vec<Symbol>,
    ) {
        for raw in symbols {
            let Some(kind) = raw.get("kind").and_then(Value::as_u64).and_then(lsp_kind) else {
                continue;
            };
            if !kind_matches(kind_filter, kind) {
                continue;
            }
            let name = raw
                .get("name")
                .and_then(Value::as_str)
                .map(clean_symbol_name)
                .unwrap_or_default();
            let short = symbol_short_name(&name);
            let container = raw
                .get("containerName")
                .and_then(Value::as_str)
                .unwrap_or("");
            let qualified = if container.is_empty() {
                name.clone()
            } else {
                format!("{container}::{name}")
            };
            if wanted.is_some_and(|wanted| {
                !name_matches(&wanted.replace('.', "::"), &short, &qualified, "::")
            }) {
                continue;
            }
            let Some(range) = raw.pointer("/location/range") else {
                continue;
            };
            if let Some(symbol) = symbol_from_range(path, text, range, kind, short, qualified) {
                out.push(symbol);
            }
        }
    }

    fn visit_positions(
        &self,
        uri: &str,
        symbols: &[Value],
        prefix: &mut Vec<String>,
        wanted: &str,
        out: &mut Vec<(String, usize, usize)>,
    ) {
        for raw in symbols {
            let Some(name) = raw
                .get("name")
                .and_then(Value::as_str)
                .map(clean_symbol_name)
            else {
                continue;
            };
            let qualified = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}::{name}", prefix.join("::"))
            };
            let short = symbol_short_name(&name);
            if raw
                .get("kind")
                .and_then(Value::as_u64)
                .and_then(lsp_kind)
                .is_some()
                && name_matches(&wanted.replace('.', "::"), &short, &qualified, "::")
            {
                let range = raw.get("selectionRange").or_else(|| raw.get("range"));
                if let Some(range) = range {
                    let line = range
                        .pointer("/start/line")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    let character = range
                        .pointer("/start/character")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    out.push((uri.to_string(), line, character));
                }
            }
            let container = raw
                .get("kind")
                .and_then(Value::as_u64)
                .is_some_and(|kind| matches!(kind, 3 | 5 | 10 | 23));
            if container {
                prefix.push(name);
            }
            if let Some(children) = raw.get("children").and_then(Value::as_array) {
                self.visit_positions(uri, children, prefix, wanted, out);
            }
            if container {
                prefix.pop();
            }
        }
    }

    fn open(&mut self, path: &Path, text: &str, uri: &str) -> anyhow::Result<()> {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id(path),
                    "version": 1,
                    "text": text
                }
            }),
        )
    }

    fn close(&mut self, uri: &str) -> anyhow::Result<()> {
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        )
    }

    fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        loop {
            let message = self.read_message()?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(anyhow!("clangd {method} failed: {error}"));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    fn request_collecting_diagnostics(
        &mut self,
        method: &str,
        params: Value,
        uri: &str,
    ) -> anyhow::Result<Vec<DiagnosticRecord>> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let mut out = Vec::new();
        loop {
            let message = self.read_message()?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(anyhow!("clangd {method} failed: {error}"));
                }
                return Ok(out);
            }
            if message.get("method").and_then(Value::as_str)
                == Some("textDocument/publishDiagnostics")
                && message.pointer("/params/uri").and_then(Value::as_str) == Some(uri)
            {
                out.extend(published_diagnostics_to_records(&message));
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    fn send(&mut self, payload: Value) -> anyhow::Result<()> {
        let body = serde_json::to_vec(&payload)?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        self.stdin.write_all(&body)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_message(&mut self) -> anyhow::Result<Value> {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            if self.stdout.read_line(&mut line)? == 0 {
                return Err(anyhow!("clangd exited while waiting for a response"));
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some(value) = trimmed.strip_prefix("Content-Length:") {
                content_length = Some(value.trim().parse::<usize>()?);
            }
        }
        let length = content_length.context("clangd response missing Content-Length")?;
        let mut body = vec![0; length];
        self.stdout.read_exact(&mut body)?;
        Ok(serde_json::from_slice(&body)?)
    }

    fn shutdown(&mut self) {
        let _ = self.request("shutdown", json!({}));
        let _ = self.notify("exit", json!({}));
        let _ = self.child.kill();
    }
}

fn symbol_from_lsp(
    path: &Path,
    text: &str,
    raw: &Value,
    kind: SymbolKind,
    name: String,
    qualified: String,
) -> Option<Symbol> {
    symbol_from_range(path, text, raw.get("range")?, kind, name, qualified)
}

fn symbol_from_range(
    path: &Path,
    text: &str,
    range: &Value,
    kind: SymbolKind,
    name: String,
    qualified: String,
) -> Option<Symbol> {
    let start_line = range.pointer("/start/line")?.as_u64()? as usize + 1;
    let end_line = range.pointer("/end/line")?.as_u64()? as usize + 1;
    Some(Symbol::new(
        path.to_path_buf(),
        language_for_path(path).unwrap_or(Language::Text),
        "clangd",
        kind,
        name,
        qualified,
        start_line,
        end_line,
        line_slice(text, start_line, end_line),
    ))
}

fn call_hierarchy_item_to_symbol(item: &Value) -> Option<Symbol> {
    let uri = item.get("uri").and_then(Value::as_str)?;
    let path = path_from_uri(uri)?;
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .map(clean_symbol_name)?;
    let detail = item.get("detail").and_then(Value::as_str).unwrap_or("");
    let qualified = if detail.is_empty() || name.contains("::") {
        name.clone()
    } else {
        let prefix = detail
            .trim()
            .trim_matches(|ch| matches!(ch, '(' | ')' | '[' | ']'));
        if prefix.is_empty() {
            name.clone()
        } else if prefix.split("::").last() == Some(name.as_str()) {
            prefix.to_string()
        } else {
            format!("{prefix}::{name}")
        }
    };
    let range = item.get("range")?;
    let kind = item
        .get("kind")
        .and_then(Value::as_u64)
        .and_then(lsp_kind)
        .unwrap_or(SymbolKind::Function);
    let text = read_text(&path).unwrap_or_default();
    symbol_from_range(
        &path,
        &text,
        range,
        kind,
        symbol_short_name(&name),
        qualified,
    )
}

fn published_diagnostics_to_records(message: &Value) -> Vec<DiagnosticRecord> {
    let Some(uri) = message.pointer("/params/uri").and_then(Value::as_str) else {
        return Vec::new();
    };
    let Some(path) = path_from_uri(uri) else {
        return Vec::new();
    };
    let language = language_for_path(&path).unwrap_or(Language::Text);
    let Some(diagnostics) = message
        .pointer("/params/diagnostics")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    diagnostics
        .iter()
        .filter_map(|raw| lsp_diagnostic_to_record(&path, language, raw))
        .collect()
}

fn lsp_diagnostic_to_record(
    path: &Path,
    language: Language,
    raw: &Value,
) -> Option<DiagnosticRecord> {
    let range = raw.get("range")?;
    let code = raw.get("code").and_then(|code| {
        code.as_str()
            .map(ToOwned::to_owned)
            .or_else(|| code.as_i64().map(|value| value.to_string()))
    });
    Some(DiagnosticRecord {
        path: path.to_path_buf(),
        language,
        backend: "lsp".to_string(),
        tool: "clangd".to_string(),
        severity: lsp_diagnostic_severity(raw.get("severity").and_then(Value::as_u64)),
        code,
        message: raw.get("message")?.as_str()?.to_string(),
        start_line: lsp_position(range.pointer("/start/line").and_then(Value::as_u64)),
        start_column: lsp_position(range.pointer("/start/character").and_then(Value::as_u64)),
        end_line: lsp_position(range.pointer("/end/line").and_then(Value::as_u64)),
        end_column: lsp_position(range.pointer("/end/character").and_then(Value::as_u64)),
        related: related_lsp_diagnostics(raw),
    })
}

fn related_lsp_diagnostics(raw: &Value) -> Vec<RelatedDiagnostic> {
    let Some(related) = raw.get("relatedInformation").and_then(Value::as_array) else {
        return Vec::new();
    };
    related
        .iter()
        .filter_map(|item| {
            let location = item.get("location")?;
            let path = path_from_uri(location.get("uri")?.as_str()?)?;
            let range = location.get("range")?;
            Some(RelatedDiagnostic {
                path,
                message: item
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                start_line: lsp_position(range.pointer("/start/line").and_then(Value::as_u64)),
                start_column: lsp_position(
                    range.pointer("/start/character").and_then(Value::as_u64),
                ),
            })
        })
        .collect()
}

fn lsp_diagnostic_severity(value: Option<u64>) -> DiagnosticSeverity {
    match value {
        Some(1) => DiagnosticSeverity::Error,
        Some(2) => DiagnosticSeverity::Warning,
        Some(3) => DiagnosticSeverity::Info,
        Some(4) => DiagnosticSeverity::Hint,
        _ => DiagnosticSeverity::Info,
    }
}

fn lsp_position(value: Option<u64>) -> usize {
    value.unwrap_or(0) as usize + 1
}

fn lsp_kind(kind: u64) -> Option<SymbolKind> {
    match kind {
        5 => Some(SymbolKind::Class),
        6 | 9 | 12 => Some(SymbolKind::Function),
        8 | 13 | 14 | 22 => Some(SymbolKind::Variable),
        10 => Some(SymbolKind::Enum),
        23 => Some(SymbolKind::Struct),
        _ => None,
    }
}

fn clean_symbol_name(name: &str) -> String {
    name.split('(').next().unwrap_or(name).trim().to_string()
}

fn symbol_short_name(name: &str) -> String {
    let clean = clean_symbol_name(name);
    if clean.starts_with("operator") {
        clean.replace(' ', "")
    } else {
        clean
            .split("::")
            .last()
            .unwrap_or(&clean)
            .trim_start_matches('~')
            .to_string()
    }
}

fn language_id(path: &Path) -> &'static str {
    match language_for_path(path).unwrap_or(Language::Cpp) {
        Language::C => "c",
        Language::Cuda => "cuda-cpp",
        _ => "cpp",
    }
}

fn uri_for_path(path: &Path) -> anyhow::Result<String> {
    Ok(Url::from_file_path(path.canonicalize()?)
        .map_err(|_| anyhow!("failed to convert path to file URI: {}", path.display()))?
        .to_string())
}

fn path_from_uri(uri: &str) -> Option<PathBuf> {
    Url::parse(uri).ok()?.to_file_path().ok()
}
