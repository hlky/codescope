use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use serde_json::{Value, json};
use url::Url;

use crate::model::{Language, Symbol, SymbolKind, SymbolKindFilter, kind_matches, name_matches};
use crate::workspace::{language_for_path, line_slice, read_text};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LspServer {
    Clangd,
    RustAnalyzer,
}

impl LspServer {
    fn executable(self) -> &'static str {
        match self {
            Self::Clangd => "clangd",
            Self::RustAnalyzer => "rust-analyzer",
        }
    }

    fn backend(self) -> &'static str {
        self.executable()
    }
}

pub struct LspOptions {
    pub root: PathBuf,
    pub compile_commands_dir: Option<PathBuf>,
    pub server: LspServer,
}

pub fn clangd_available() -> bool {
    server_available(LspServer::Clangd)
}

pub fn rust_analyzer_available() -> bool {
    server_available(LspServer::RustAnalyzer)
}

fn server_available(server: LspServer) -> bool {
    let Ok(executable) = which::which(server.executable()) else {
        return false;
    };
    Command::new(executable)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn document_symbols(
    files: &[(PathBuf, String)],
    options: &LspOptions,
    kind_filter: Option<SymbolKindFilter>,
    wanted: Option<&str>,
    max_matches: usize,
) -> anyhow::Result<Vec<Symbol>> {
    let mut client = LspClient::start(options, false)?;
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
    options: &LspOptions,
    wanted: &str,
    max_matches: usize,
) -> anyhow::Result<Vec<Symbol>> {
    let mut client = LspClient::start(options, true)?;
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
        let result = client.request_nonempty_array(
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
                    client.server.backend(),
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
    options: &LspOptions,
    wanted: &str,
    max_matches: usize,
) -> anyhow::Result<Vec<Symbol>> {
    let mut client = LspClient::start(options, true)?;
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
        let prepared = client.request_nonempty_array(
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
                    let Some(symbol) = call_hierarchy_item_to_symbol(from, client.server) else {
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

struct LspClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    server: LspServer,
    root_uri: String,
    server_quiescent: bool,
    server_error: Option<String>,
}

impl LspClient {
    fn start(options: &LspOptions, wait_for_workspace: bool) -> anyhow::Result<Self> {
        let server_name = options.server.executable();
        let executable = which::which(server_name)
            .with_context(|| format!("{server_name} was not found on PATH"))?;
        let mut command = Command::new(executable);
        if options.server == LspServer::Clangd {
            command.arg("--background-index").arg("--log=error");
            if let Some(dir) = &options.compile_commands_dir {
                command.arg(format!("--compile-commands-dir={}", dir.display()));
            }
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to start {server_name}"))?;
        let stdin = child
            .stdin
            .take()
            .with_context(|| format!("failed to capture {server_name} stdin"))?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .with_context(|| format!("failed to capture {server_name} stdout"))?,
        );
        let root_uri = uri_for_path(&options.root)?;
        let mut client = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
            server: options.server,
            root_uri: root_uri.clone(),
            server_quiescent: false,
            server_error: None,
        };
        client.request(
            "initialize",
            json!({
                "processId": null,
                "clientInfo": { "name": "codescope" },
                "rootUri": root_uri,
                "workspaceFolders": [{
                    "uri": client.root_uri.clone(),
                    "name": options.root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace")
                }],
                "capabilities": {
                    "workspace": {
                        "configuration": true,
                        "workspaceFolders": true
                    },
                    "textDocument": {
                        "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                        "callHierarchy": {}
                    },
                    "experimental": {
                        "serverStatusNotification": true
                    }
                }
            }),
        )?;
        client.notify("initialized", json!({}))?;
        if wait_for_workspace
            && options.server == LspServer::RustAnalyzer
            && options.root.join("Cargo.toml").is_file()
        {
            client.wait_for_rust_workspace()?;
        }
        Ok(client)
    }

    fn wait_for_rust_workspace(&mut self) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut workspace_loaded_at = None;
        loop {
            let status = self.request(
                "rust-analyzer/analyzerStatus",
                json!({ "textDocument": null }),
            )?;
            if workspace_loaded_at.is_none()
                && status.as_str().is_some_and(|status| {
                    !status.is_empty() && !status.starts_with("No workspaces")
                })
            {
                workspace_loaded_at = Some(Instant::now());
            }
            if let Some(error) = self.server_error.take() {
                return Err(anyhow!(
                    "rust-analyzer failed to load the workspace: {error}"
                ));
            }
            if workspace_loaded_at.is_some_and(|loaded_at| {
                self.server_quiescent || loaded_at.elapsed() >= Duration::from_secs(3)
            }) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "rust-analyzer did not finish loading the Cargo workspace within 15 seconds"
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
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
                    && let Some(symbol) = symbol_from_lsp(
                        path,
                        text,
                        raw,
                        self.server,
                        kind,
                        short,
                        qualified.clone(),
                    )
                {
                    out.push(symbol);
                }
            }
            let container_kind = raw.get("kind").and_then(Value::as_u64);
            let container = container_kind.is_some_and(is_container_kind);
            if let Some(kind) = container_kind.filter(|kind| is_container_kind(*kind)) {
                prefix.push(container_scope_name(self.server, kind, &name));
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
            if let Some(symbol) =
                symbol_from_range(path, text, range, self.server, kind, short, qualified)
            {
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
            let container_kind = raw.get("kind").and_then(Value::as_u64);
            let container = container_kind.is_some_and(is_container_kind);
            if let Some(kind) = container_kind.filter(|kind| is_container_kind(*kind)) {
                prefix.push(container_scope_name(self.server, kind, &name));
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
            self.process_server_message(&message)?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(anyhow!(
                        "{} {method} failed: {error}",
                        self.server.backend()
                    ));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    fn request_nonempty_array(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let result = self.request(method, params.clone())?;
            if self.server != LspServer::RustAnalyzer
                || result.as_array().is_some_and(|items| !items.is_empty())
                || Instant::now() >= deadline
            {
                return Ok(result);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn process_server_message(&mut self, message: &Value) -> anyhow::Result<()> {
        if message.get("method").and_then(Value::as_str) == Some("experimental/serverStatus") {
            self.server_quiescent = message
                .pointer("/params/quiescent")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if message.pointer("/params/health").and_then(Value::as_str) == Some("error") {
                self.server_error = Some(
                    message
                        .pointer("/params/message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown rust-analyzer workspace error")
                        .to_string(),
                );
            }
        }
        let (Some(id), Some(method)) = (message.get("id"), message.get("method")) else {
            return Ok(());
        };
        let result = match method.as_str().unwrap_or_default() {
            "workspace/configuration" => {
                let count = message
                    .pointer("/params/items")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                Value::Array(vec![Value::Null; count])
            }
            "workspace/workspaceFolders" => json!([{
                "uri": self.root_uri,
                "name": "workspace"
            }]),
            _ => Value::Null,
        };
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
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
                return Err(anyhow!(
                    "{} exited while waiting for a response",
                    self.server.backend()
                ));
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some(value) = trimmed.strip_prefix("Content-Length:") {
                content_length = Some(value.trim().parse::<usize>()?);
            }
        }
        let length = content_length.with_context(|| {
            format!("{} response missing Content-Length", self.server.backend())
        })?;
        let mut body = vec![0; length];
        self.stdout.read_exact(&mut body)?;
        Ok(serde_json::from_slice(&body)?)
    }

    fn shutdown(&mut self) {
        let _ = self.request("shutdown", Value::Null);
        let _ = self.notify("exit", json!({}));
        self.terminate();
    }

    fn terminate(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn symbol_from_lsp(
    path: &Path,
    text: &str,
    raw: &Value,
    server: LspServer,
    kind: SymbolKind,
    name: String,
    qualified: String,
) -> Option<Symbol> {
    symbol_from_range(path, text, raw.get("range")?, server, kind, name, qualified)
}

fn symbol_from_range(
    path: &Path,
    text: &str,
    range: &Value,
    server: LspServer,
    kind: SymbolKind,
    name: String,
    qualified: String,
) -> Option<Symbol> {
    let start_line = range.pointer("/start/line")?.as_u64()? as usize + 1;
    let end_line = range.pointer("/end/line")?.as_u64()? as usize + 1;
    Some(Symbol::new(
        path.to_path_buf(),
        language_for_path(path).unwrap_or(Language::Text),
        server.backend(),
        kind,
        name,
        qualified,
        start_line,
        end_line,
        line_slice(text, start_line, end_line),
    ))
}

fn call_hierarchy_item_to_symbol(item: &Value, server: LspServer) -> Option<Symbol> {
    let uri = item.get("uri").and_then(Value::as_str)?;
    let path = path_from_uri(uri)?;
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .map(clean_symbol_name)?;
    let detail = item.get("detail").and_then(Value::as_str).unwrap_or("");
    let qualified = if server == LspServer::RustAnalyzer || detail.is_empty() || name.contains("::")
    {
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
        server,
        kind,
        symbol_short_name(&name),
        qualified,
    )
}

fn is_container_kind(kind: u64) -> bool {
    matches!(kind, 2 | 3 | 5 | 10 | 11 | 19 | 23)
}

fn container_scope_name(server: LspServer, kind: u64, name: &str) -> String {
    if server != LspServer::RustAnalyzer || kind != 19 {
        return name.to_string();
    }
    let mut implementation = name.strip_prefix("impl ").unwrap_or(name).trim();
    if implementation.starts_with('<')
        && let Some(end) = matching_angle_bracket(implementation)
    {
        implementation = implementation[end + 1..].trim();
    }
    if let Some((_, implemented_type)) = implementation.rsplit_once(" for ") {
        implementation = implemented_type.trim();
    }
    strip_angle_arguments(implementation)
}

fn matching_angle_bracket(value: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn strip_angle_arguments(value: &str) -> String {
    let mut result = String::new();
    let mut depth = 0usize;
    for character in value.chars() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => result.push(character),
            _ => {}
        }
    }
    result.trim().to_string()
}

fn lsp_kind(kind: u64) -> Option<SymbolKind> {
    match kind {
        2 => Some(SymbolKind::Module),
        5 => Some(SymbolKind::Class),
        6 | 9 | 12 => Some(SymbolKind::Function),
        8 | 13 | 14 | 22 => Some(SymbolKind::Variable),
        10 => Some(SymbolKind::Enum),
        11 => Some(SymbolKind::Trait),
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
        Language::Rust => "rust",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_impl_container_uses_implemented_type() {
        assert_eq!(
            container_scope_name(LspServer::RustAnalyzer, 19, "impl<T> Runner for Worker<T>"),
            "Worker"
        );
    }

    #[test]
    fn dropping_client_terminates_and_reaps_child() {
        let mut command = long_running_command();
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("long-running test child should start");
        let process_id = child.id();
        let stdin = child
            .stdin
            .take()
            .expect("test child stdin should be piped");
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .expect("test child stdout should be piped"),
        );

        let client = LspClient {
            child,
            stdin,
            stdout,
            next_id: 1,
            server: LspServer::Clangd,
            root_uri: "file:///workspace".to_string(),
            server_quiescent: false,
            server_error: None,
        };
        drop(client);

        assert!(!process_exists(process_id));
    }

    #[cfg(unix)]
    fn long_running_command() -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", "while :; do sleep 60; done"]);
        command
    }

    #[cfg(windows)]
    fn long_running_command() -> Command {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "while ($true) { Start-Sleep -Seconds 60 }",
        ]);
        command
    }

    #[cfg(unix)]
    fn process_exists(process_id: u32) -> bool {
        Command::new("kill")
            .args(["-0", &process_id.to_string()])
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(windows)]
    fn process_exists(process_id: u32) -> bool {
        Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!("Get-Process -Id {process_id} -ErrorAction SilentlyContinue"),
            ])
            .status()
            .is_ok_and(|status| status.success())
    }
}
