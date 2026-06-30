use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::SystemTime;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use clap::ValueEnum;
use serde::Serialize;
use serde_json::Value;

use crate::model::{Backend, Language, LanguageFilter};
use crate::path_display::display_path;
use crate::workspace::{language_allowed, language_for_path, read_text, source_files};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum DiagnosticTool {
    Auto,
    Clangd,
    Cargo,
    Ruff,
    Mypy,
    Pyright,
    Cmake,
}

impl fmt::Display for DiagnosticTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Auto => "auto",
            Self::Clangd => "clangd",
            Self::Cargo => "cargo",
            Self::Ruff => "ruff",
            Self::Mypy => "mypy",
            Self::Pyright => "pyright",
            Self::Cmake => "cmake",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Hint => "hint",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RelatedDiagnostic {
    #[serde(serialize_with = "crate::path_display::serialize")]
    pub path: PathBuf,
    pub message: String,
    pub start_line: usize,
    pub start_column: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiagnosticRecord {
    #[serde(serialize_with = "crate::path_display::serialize")]
    pub path: PathBuf,
    pub language: Language,
    pub backend: String,
    pub tool: String,
    pub severity: DiagnosticSeverity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedDiagnostic>,
}

#[derive(Clone, Debug)]
pub struct DiagnosticOptions {
    pub path: PathBuf,
    pub file: Option<PathBuf>,
    pub root: Option<PathBuf>,
    pub lang: Option<LanguageFilter>,
    pub backend: Backend,
    pub compile_commands_dir: Option<PathBuf>,
    pub tool: DiagnosticTool,
    pub max_matches: usize,
}

#[derive(Clone, Debug)]
pub struct DiagnosticRun {
    pub records: Vec<DiagnosticRecord>,
    pub backend_failed: bool,
}

const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

pub fn collect(options: &DiagnosticOptions) -> anyhow::Result<DiagnosticRun> {
    let root = resolve_root(options)?;
    let mut out = Vec::new();
    let mut backend_failed = false;
    match options.tool {
        DiagnosticTool::Auto => {
            if should_run_cargo(&root, options)
                && tool_available("cargo")
                && let Ok(records) = cargo_check(&root, options.max_matches - out.len())
            {
                out.extend(records);
            }
            if out.len() < options.max_matches
                && should_run_clangd(&root, options)
                && tool_available("clangd")
                && let Ok(records) = clangd(&root, options, options.max_matches - out.len())
            {
                out.extend(records);
            }
            if out.len() < options.max_matches
                && should_run_python(&root, options)
                && tool_available("ruff")
                && let Ok(records) = ruff_check(&root, options, options.max_matches - out.len())
            {
                out.extend(records);
            }
            if out.len() < options.max_matches
                && should_run_python(&root, options)
                && tool_available("mypy")
                && let Ok(records) = mypy_check(&root, options, options.max_matches - out.len())
            {
                out.extend(records);
            }
            if out.len() < options.max_matches
                && should_run_python(&root, options)
                && tool_available("pyright")
                && let Ok(records) = pyright_check(&root, options, options.max_matches - out.len())
            {
                out.extend(records);
            }
            if out.len() < options.max_matches
                && should_run_cmake(&root, options)
                && tool_available("cmake")
                && let Ok(records) = cmake_check(&root, options.max_matches - out.len())
            {
                out.extend(records);
            }
        }
        DiagnosticTool::Cargo => {
            match require_tool("cargo").and_then(|_| cargo_check(&root, options.max_matches)) {
                Ok(records) => out.extend(records),
                Err(error) => {
                    out.push(backend_error_record(&root, "cargo", error));
                    backend_failed = true;
                }
            }
        }
        DiagnosticTool::Clangd => {
            match require_tool("clangd").and_then(|_| clangd(&root, options, options.max_matches)) {
                Ok(records) => out.extend(records),
                Err(error) => {
                    out.push(backend_error_record(&root, "clangd", error));
                    backend_failed = true;
                }
            }
        }
        DiagnosticTool::Ruff => {
            match require_tool("ruff").and_then(|_| ruff_check(&root, options, options.max_matches))
            {
                Ok(records) => out.extend(records),
                Err(error) => {
                    out.push(backend_error_record(&root, "ruff", error));
                    backend_failed = true;
                }
            }
        }
        DiagnosticTool::Mypy => {
            match require_tool("mypy").and_then(|_| mypy_check(&root, options, options.max_matches))
            {
                Ok(records) => out.extend(records),
                Err(error) => {
                    out.push(backend_error_record(&root, "mypy", error));
                    backend_failed = true;
                }
            }
        }
        DiagnosticTool::Pyright => {
            match require_tool("pyright")
                .and_then(|_| pyright_check(&root, options, options.max_matches))
            {
                Ok(records) => out.extend(records),
                Err(error) => {
                    out.push(backend_error_record(&root, "pyright", error));
                    backend_failed = true;
                }
            }
        }
        DiagnosticTool::Cmake => {
            match require_tool("cmake").and_then(|_| cmake_check(&root, options.max_matches)) {
                Ok(records) => out.extend(records),
                Err(error) => {
                    out.push(backend_error_record(&root, "cmake", error));
                    backend_failed = true;
                }
            }
        }
    }
    out.truncate(options.max_matches);
    Ok(DiagnosticRun {
        records: out,
        backend_failed,
    })
}

pub fn render_plain(records: &[DiagnosticRecord]) -> String {
    let mut lines = Vec::new();
    let mut current: Option<&Path> = None;
    for record in records {
        if current != Some(record.path.as_path()) {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(display_path(&record.path));
            current = Some(&record.path);
        }
        let code = record
            .code
            .as_ref()
            .map(|code| format!(" [{code}]"))
            .unwrap_or_default();
        lines.push(format!(
            "  {}:{} {} {}{}: {}",
            record.start_line,
            record.start_column,
            record.severity,
            record.tool,
            code,
            record.message
        ));
    }
    lines.join("\n")
}

fn resolve_root(options: &DiagnosticOptions) -> anyhow::Result<PathBuf> {
    if let Some(root) = &options.root {
        return root
            .canonicalize()
            .with_context(|| format!("failed to resolve --root {}", root.display()));
    }
    let base = options.file.as_ref().unwrap_or(&options.path);
    let resolved = base
        .canonicalize()
        .with_context(|| format!("failed to resolve path {}", base.display()))?;
    Ok(if resolved.is_file() {
        resolved.parent().map(Path::to_path_buf).unwrap_or(resolved)
    } else {
        resolved
    })
}

fn should_run_cargo(root: &Path, options: &DiagnosticOptions) -> bool {
    options
        .lang
        .is_none_or(|lang| language_allowed(Language::Rust, Some(lang)))
        && find_manifest_root(root).is_some()
}

fn should_run_clangd(root: &Path, options: &DiagnosticOptions) -> bool {
    !c_family_files(root, options).is_empty()
}

fn should_run_python(root: &Path, options: &DiagnosticOptions) -> bool {
    options
        .lang
        .is_none_or(|lang| language_allowed(Language::Python, Some(lang)))
        && !python_files(root, options).is_empty()
}

fn should_run_cmake(root: &Path, options: &DiagnosticOptions) -> bool {
    options
        .lang
        .is_none_or(|lang| language_allowed(Language::Cmake, Some(lang)))
        && (root.join("CMakeLists.txt").is_file()
            || options.file.as_ref().is_some_and(|file| {
                language_for_path(file) == Some(Language::Cmake)
                    && file
                        .file_name()
                        .is_some_and(|name| name.eq_ignore_ascii_case("CMakeLists.txt"))
            }))
}

fn tool_available(name: &str) -> bool {
    which::which(name).is_ok()
}

fn require_tool(name: &str) -> anyhow::Result<()> {
    which::which(name)
        .map(|_| ())
        .with_context(|| format!("{name} was not found on PATH"))
}

fn backend_error_record(root: &Path, tool: &str, error: anyhow::Error) -> DiagnosticRecord {
    DiagnosticRecord {
        path: root.to_path_buf(),
        language: Language::Text,
        backend: "process".to_string(),
        tool: tool.to_string(),
        severity: DiagnosticSeverity::Error,
        code: Some("backend-error".to_string()),
        message: format!("{error:#}"),
        start_line: 1,
        start_column: 1,
        end_line: 1,
        end_column: 1,
        related: Vec::new(),
    }
}

fn run_with_timeout(mut command: Command, tool: &str) -> anyhow::Result<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start {tool}"))?;
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child
                .wait_with_output()
                .with_context(|| format!("failed to read {tool} output"));
        }
        if start.elapsed() >= TOOL_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!(
                "{tool} timed out after {}s",
                TOOL_TIMEOUT.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn diagnostic_target_arg(root: &Path, options: &DiagnosticOptions) -> PathBuf {
    options
        .file
        .as_ref()
        .and_then(|file| file.strip_prefix(root).ok())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn python_files(root: &Path, options: &DiagnosticOptions) -> Vec<PathBuf> {
    let files = if let Some(file) = &options.file {
        vec![file.clone()]
    } else {
        source_files(root, options.lang)
    };
    files
        .into_iter()
        .filter(|path| language_for_path(path) == Some(Language::Python))
        .collect()
}

fn c_family_files(root: &Path, options: &DiagnosticOptions) -> Vec<(PathBuf, String)> {
    let files = if let Some(file) = &options.file {
        vec![file.clone()]
    } else {
        source_files(root, options.lang)
    };
    files
        .into_iter()
        .filter(|path| {
            matches!(
                language_for_path(path),
                Some(Language::C | Language::Cpp | Language::Cuda | Language::Hip)
            )
        })
        .filter_map(|path| read_text(&path).map(|text| (path, text)))
        .collect()
}

fn clangd(
    root: &Path,
    options: &DiagnosticOptions,
    max_matches: usize,
) -> anyhow::Result<Vec<DiagnosticRecord>> {
    let files = c_family_files(root, options);
    if files.is_empty() {
        return Ok(Vec::new());
    }
    if options.backend != Backend::Auto && options.backend != Backend::Lsp {
        return Err(anyhow!(
            "clangd diagnostics require --backend auto or --backend lsp"
        ));
    }
    let lsp_options = crate::lsp::ClangdOptions {
        root: root.to_path_buf(),
        compile_commands_dir: options.compile_commands_dir.clone(),
    };
    crate::lsp::diagnostics(&files, &lsp_options, max_matches)
}

fn cargo_check(root: &Path, max_matches: usize) -> anyhow::Result<Vec<DiagnosticRecord>> {
    let manifest_root = find_manifest_root(root)
        .ok_or_else(|| anyhow!("could not find Cargo.toml at or above {}", root.display()))?;
    let cargo = which::which("cargo").context("cargo was not found on PATH")?;
    let mut command = Command::new(cargo);
    command
        .arg("check")
        .arg("--message-format=json")
        .current_dir(&manifest_root);
    let output = run_with_timeout(command, "cargo")?;

    let mut records =
        parse_cargo_json_lines(&String::from_utf8_lossy(&output.stdout), &manifest_root);
    records.truncate(max_matches);
    if !output.status.success() && records.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "cargo check failed without JSON diagnostics: {}",
            stderr.trim()
        ));
    }
    Ok(records)
}

fn ruff_check(
    root: &Path,
    options: &DiagnosticOptions,
    max_matches: usize,
) -> anyhow::Result<Vec<DiagnosticRecord>> {
    let ruff = which::which("ruff").context("ruff was not found on PATH")?;
    let mut command = Command::new(ruff);
    command
        .arg("check")
        .arg("--output-format=json")
        .arg(diagnostic_target_arg(root, options))
        .current_dir(root);
    let output = run_with_timeout(command, "ruff")?;
    let mut records = parse_ruff_json(&String::from_utf8_lossy(&output.stdout), root);
    records.truncate(max_matches);
    if !output.status.success() && records.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "ruff check failed without JSON diagnostics: {}",
            stderr.trim()
        ));
    }
    Ok(records)
}

fn mypy_check(
    root: &Path,
    options: &DiagnosticOptions,
    max_matches: usize,
) -> anyhow::Result<Vec<DiagnosticRecord>> {
    let mypy = which::which("mypy").context("mypy was not found on PATH")?;
    let mut command = Command::new(mypy);
    command
        .arg("--show-column-numbers")
        .arg("--show-error-codes")
        .arg("--no-error-summary")
        .arg(diagnostic_target_arg(root, options))
        .current_dir(root);
    let output = run_with_timeout(command, "mypy")?;
    let mut records = parse_mypy_output(&String::from_utf8_lossy(&output.stdout), root);
    records.truncate(max_matches);
    if !output.status.success() && records.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "mypy failed without parseable diagnostics: {}",
            stderr.trim()
        ));
    }
    Ok(records)
}

fn pyright_check(
    root: &Path,
    options: &DiagnosticOptions,
    max_matches: usize,
) -> anyhow::Result<Vec<DiagnosticRecord>> {
    let pyright = which::which("pyright").context("pyright was not found on PATH")?;
    let mut command = Command::new(pyright);
    command
        .arg("--outputjson")
        .arg(diagnostic_target_arg(root, options))
        .current_dir(root);
    let output = run_with_timeout(command, "pyright")?;
    let mut records = parse_pyright_json(&String::from_utf8_lossy(&output.stdout), root);
    records.truncate(max_matches);
    if !output.status.success() && records.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "pyright failed without JSON diagnostics: {}",
            stderr.trim()
        ));
    }
    Ok(records)
}

fn cmake_check(root: &Path, max_matches: usize) -> anyhow::Result<Vec<DiagnosticRecord>> {
    let cmake = which::which("cmake").context("cmake was not found on PATH")?;
    let manifest_root = find_cmake_root(root).ok_or_else(|| {
        anyhow!(
            "could not find CMakeLists.txt at or below {}",
            root.display()
        )
    })?;
    let build_dir = std::env::temp_dir().join(format!(
        "codescope-cmake-{}-{}",
        std::process::id(),
        chrono_like_millis()
    ));
    std::fs::create_dir_all(&build_dir)
        .with_context(|| format!("failed to create {}", build_dir.display()))?;

    let mut configure = Command::new(&cmake);
    configure
        .arg("-S")
        .arg(&manifest_root)
        .arg("-B")
        .arg(&build_dir)
        .current_dir(&manifest_root);
    let configure_output = run_with_timeout(configure, "cmake")?;
    let mut records = parse_cmake_output(
        &format!(
            "{}\n{}",
            String::from_utf8_lossy(&configure_output.stdout),
            String::from_utf8_lossy(&configure_output.stderr)
        ),
        &manifest_root,
    );

    if configure_output.status.success() {
        let mut build = Command::new(cmake);
        build
            .arg("--build")
            .arg(&build_dir)
            .current_dir(&manifest_root);
        let build_output = run_with_timeout(build, "cmake")?;
        let build_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&build_output.stdout),
            String::from_utf8_lossy(&build_output.stderr)
        );
        records.extend(parse_cmake_output(&build_text, &manifest_root));
        records.extend(parse_build_output(&build_text, &manifest_root));
        if !build_output.status.success() && records.is_empty() {
            let _ = std::fs::remove_dir_all(&build_dir);
            return Err(anyhow!("cmake build failed without parseable diagnostics"));
        }
    } else if records.is_empty() {
        let _ = std::fs::remove_dir_all(&build_dir);
        return Err(anyhow!(
            "cmake configure failed without parseable diagnostics"
        ));
    }
    let _ = std::fs::remove_dir_all(&build_dir);
    records.truncate(max_matches);
    Ok(records)
}

fn find_manifest_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?
    } else {
        start
    };
    loop {
        if current.join("Cargo.toml").is_file() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn find_cmake_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?
    } else {
        start
    };
    loop {
        if current.join("CMakeLists.txt").is_file() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn chrono_like_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

pub fn parse_cargo_json_lines(text: &str, root: &Path) -> Vec<DiagnosticRecord> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value.get("reason").and_then(Value::as_str) == Some("compiler-message"))
        .filter_map(|value| cargo_message_to_record(&value, root))
        .collect()
}

pub fn parse_ruff_json(text: &str, root: &Path) -> Vec<DiagnosticRecord> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let path = normalize_tool_path(root, item.get("filename")?.as_str()?);
            let start = item.get("location")?;
            let end = item.get("end_location").unwrap_or(start);
            Some(DiagnosticRecord {
                path,
                language: Language::Python,
                backend: "ruff".to_string(),
                tool: "ruff".to_string(),
                severity: DiagnosticSeverity::Warning,
                code: item
                    .get("code")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                message: item.get("message")?.as_str()?.to_string(),
                start_line: one_based(start.get("row").and_then(Value::as_u64)),
                start_column: one_based(start.get("column").and_then(Value::as_u64)),
                end_line: one_based(end.get("row").and_then(Value::as_u64)),
                end_column: one_based(end.get("column").and_then(Value::as_u64)),
                related: Vec::new(),
            })
        })
        .collect()
}

pub fn parse_mypy_output(text: &str, root: &Path) -> Vec<DiagnosticRecord> {
    let pattern = regex::Regex::new(
        r"^(?P<file>.+?):(?P<line>\d+)(?::(?P<col>\d+))?: (?P<level>error|note|warning): (?P<message>.*?)(?:\s+\[(?P<code>[A-Za-z0-9_-]+)\])?$",
    )
    .expect("valid mypy diagnostic regex");
    text.lines()
        .filter_map(|line| {
            let captures = pattern.captures(line)?;
            let path = normalize_tool_path(root, captures.name("file")?.as_str());
            let start_line = captures.name("line")?.as_str().parse::<usize>().ok()?;
            let start_column = captures
                .name("col")
                .and_then(|value| value.as_str().parse::<usize>().ok())
                .unwrap_or(1);
            let message = captures.name("message")?.as_str().to_string();
            Some(DiagnosticRecord {
                path,
                language: Language::Python,
                backend: "mypy".to_string(),
                tool: "mypy".to_string(),
                severity: severity_from_text(captures.name("level")?.as_str()),
                code: captures
                    .name("code")
                    .map(|value| value.as_str().to_string()),
                message,
                start_line,
                start_column,
                end_line: start_line,
                end_column: start_column,
                related: Vec::new(),
            })
        })
        .collect()
}

pub fn parse_pyright_json(text: &str, root: &Path) -> Vec<DiagnosticRecord> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(items) = value.get("generalDiagnostics").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let path = normalize_tool_path(root, item.get("file")?.as_str()?);
            let range = item.get("range")?;
            let start = range.get("start")?;
            let end = range.get("end").unwrap_or(start);
            Some(DiagnosticRecord {
                path,
                language: Language::Python,
                backend: "pyright".to_string(),
                tool: "pyright".to_string(),
                severity: pyright_severity(item.get("severity").and_then(Value::as_str)),
                code: item
                    .get("rule")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                message: item.get("message")?.as_str()?.to_string(),
                start_line: lsp_one_based(start.get("line").and_then(Value::as_u64)),
                start_column: lsp_one_based(start.get("character").and_then(Value::as_u64)),
                end_line: lsp_one_based(end.get("line").and_then(Value::as_u64)),
                end_column: lsp_one_based(end.get("character").and_then(Value::as_u64)),
                related: Vec::new(),
            })
        })
        .collect()
}

pub fn parse_cmake_output(text: &str, root: &Path) -> Vec<DiagnosticRecord> {
    let pattern = regex::Regex::new(
        r"^CMake (?P<level>Error|Warning|Deprecation Warning)(?: \(dev\))? at (?P<file>[^:]+):(?P<line>\d+)(?: \((?P<code>[^)]+)\))?:",
    )
    .expect("valid cmake diagnostic regex");
    let lines: Vec<&str> = text.lines().collect();
    let mut records = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let Some(captures) = pattern.captures(line) else {
            continue;
        };
        let message = lines
            .iter()
            .skip(idx + 1)
            .take_while(|candidate| !candidate.starts_with("CMake "))
            .map(|candidate| candidate.trim())
            .filter(|candidate| !candidate.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let start_line = captures
            .name("line")
            .and_then(|value| value.as_str().parse::<usize>().ok())
            .unwrap_or(1);
        records.push(DiagnosticRecord {
            path: normalize_tool_path(root, captures.name("file").map_or("", |v| v.as_str())),
            language: Language::Cmake,
            backend: "cmake".to_string(),
            tool: "cmake".to_string(),
            severity: cmake_severity(captures.name("level").map(|value| value.as_str())),
            code: captures
                .name("code")
                .map(|value| value.as_str().to_string()),
            message,
            start_line,
            start_column: 1,
            end_line: start_line,
            end_column: 1,
            related: Vec::new(),
        });
    }
    records
}

pub fn parse_build_output(text: &str, root: &Path) -> Vec<DiagnosticRecord> {
    let gcc = regex::Regex::new(
        r"^(?P<file>[^:\r\n]+):(?P<line>\d+):(?P<col>\d+): (?P<level>fatal error|error|warning|note): (?P<message>.+)$",
    )
    .expect("valid gcc-style diagnostic regex");
    let msvc = regex::Regex::new(
        r"^(?P<file>.+?)\((?P<line>\d+)(?:,(?P<col>\d+))?\): (?P<level>fatal error|error|warning) (?P<code>[A-Z]+\d+): (?P<message>.+)$",
    )
    .expect("valid msvc-style diagnostic regex");
    text.lines()
        .filter_map(|line| {
            if let Some(captures) = gcc.captures(line) {
                let path = normalize_tool_path(root, captures.name("file")?.as_str());
                let start_line = captures.name("line")?.as_str().parse::<usize>().ok()?;
                let start_column = captures.name("col")?.as_str().parse::<usize>().ok()?;
                return Some(DiagnosticRecord {
                    language: language_for_path(&path).unwrap_or(Language::Text),
                    path,
                    backend: "cmake".to_string(),
                    tool: "cmake".to_string(),
                    severity: severity_from_text(captures.name("level")?.as_str()),
                    code: None,
                    message: captures.name("message")?.as_str().to_string(),
                    start_line,
                    start_column,
                    end_line: start_line,
                    end_column: start_column,
                    related: Vec::new(),
                });
            }
            let captures = msvc.captures(line)?;
            let path = normalize_tool_path(root, captures.name("file")?.as_str());
            let start_line = captures.name("line")?.as_str().parse::<usize>().ok()?;
            let start_column = captures
                .name("col")
                .and_then(|value| value.as_str().parse::<usize>().ok())
                .unwrap_or(1);
            Some(DiagnosticRecord {
                language: language_for_path(&path).unwrap_or(Language::Text),
                path,
                backend: "cmake".to_string(),
                tool: "cmake".to_string(),
                severity: severity_from_text(captures.name("level")?.as_str()),
                code: captures
                    .name("code")
                    .map(|value| value.as_str().to_string()),
                message: captures.name("message")?.as_str().to_string(),
                start_line,
                start_column,
                end_line: start_line,
                end_column: start_column,
                related: Vec::new(),
            })
        })
        .collect()
}

fn pyright_severity(value: Option<&str>) -> DiagnosticSeverity {
    match value {
        Some("error") => DiagnosticSeverity::Error,
        Some("warning") => DiagnosticSeverity::Warning,
        Some("information") => DiagnosticSeverity::Info,
        _ => DiagnosticSeverity::Hint,
    }
}

fn cmake_severity(value: Option<&str>) -> DiagnosticSeverity {
    match value {
        Some("Error") => DiagnosticSeverity::Error,
        Some("Warning" | "Deprecation Warning") => DiagnosticSeverity::Warning,
        _ => DiagnosticSeverity::Info,
    }
}

fn lsp_one_based(value: Option<u64>) -> usize {
    value.unwrap_or(0) as usize + 1
}

fn cargo_message_to_record(value: &Value, root: &Path) -> Option<DiagnosticRecord> {
    let message = value.get("message")?;
    let severity = severity_from_text(message.get("level")?.as_str()?);
    let code = message
        .pointer("/code/code")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let primary = message
        .get("spans")?
        .as_array()?
        .iter()
        .find(|span| {
            span.get("is_primary")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .or_else(|| message.get("spans")?.as_array()?.first())?;
    let path = normalize_tool_path(root, primary.get("file_name")?.as_str()?);
    Some(DiagnosticRecord {
        path,
        language: Language::Rust,
        backend: "cargo".to_string(),
        tool: "cargo".to_string(),
        severity,
        code,
        message: message.get("message")?.as_str()?.to_string(),
        start_line: one_based(primary.get("line_start").and_then(Value::as_u64)),
        start_column: one_based(primary.get("column_start").and_then(Value::as_u64)),
        end_line: one_based(primary.get("line_end").and_then(Value::as_u64)),
        end_column: one_based(primary.get("column_end").and_then(Value::as_u64)),
        related: related_cargo_spans(message, root, primary),
    })
}

fn related_cargo_spans(message: &Value, root: &Path, primary: &Value) -> Vec<RelatedDiagnostic> {
    let Some(spans) = message.get("spans").and_then(Value::as_array) else {
        return Vec::new();
    };
    spans
        .iter()
        .filter(|span| !std::ptr::eq(*span, primary))
        .filter_map(|span| {
            let path = normalize_tool_path(root, span.get("file_name")?.as_str()?);
            Some(RelatedDiagnostic {
                path,
                message: span
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                start_line: one_based(span.get("line_start").and_then(Value::as_u64)),
                start_column: one_based(span.get("column_start").and_then(Value::as_u64)),
            })
        })
        .collect()
}

pub fn normalize_tool_path(root: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

pub fn severity_from_text(level: &str) -> DiagnosticSeverity {
    match level {
        "error" | "fatal error" => DiagnosticSeverity::Error,
        "warning" | "warn" => DiagnosticSeverity::Warning,
        "help" | "note" => DiagnosticSeverity::Info,
        _ => DiagnosticSeverity::Hint,
    }
}

fn one_based(value: Option<u64>) -> usize {
    value.unwrap_or(1).max(1) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cargo_compiler_message() {
        let root = Path::new("repo");
        let input = r#"{"reason":"compiler-message","message":{"message":"cannot find value `x` in this scope","code":{"code":"E0425"},"level":"error","spans":[{"file_name":"src/lib.rs","line_start":3,"line_end":3,"column_start":5,"column_end":6,"is_primary":true,"label":"not found in this scope"}]}}"#;
        let records = parse_cargo_json_lines(input, root);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].language, Language::Rust);
        assert_eq!(records[0].severity, DiagnosticSeverity::Error);
        assert_eq!(records[0].code.as_deref(), Some("E0425"));
        assert_eq!(records[0].start_line, 3);
    }

    #[test]
    fn parses_ruff_json() {
        let input = r#"[{"cell":null,"code":"F821","end_location":{"row":1,"column":22},"filename":"src/bad.py","fix":null,"location":{"row":1,"column":16},"message":"Undefined name `missing`","noqa_row":1,"url":"https://docs.astral.sh/ruff/rules/undefined-name"}]"#;
        let records = parse_ruff_json(input, Path::new("repo"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, "ruff");
        assert_eq!(records[0].code.as_deref(), Some("F821"));
        assert_eq!(records[0].start_line, 1);
    }

    #[test]
    fn parses_mypy_output() {
        let input = "src/bad.py:2:5: error: Incompatible return value type (got \"str\", expected \"int\")  [return-value]\n";
        let records = parse_mypy_output(input, Path::new("repo"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, "mypy");
        assert_eq!(records[0].severity, DiagnosticSeverity::Error);
        assert_eq!(records[0].code.as_deref(), Some("return-value"));
        assert_eq!(records[0].start_column, 5);
    }

    #[test]
    fn parses_pyright_json() {
        let input = r#"{"version":"1.1.402","time":"0","generalDiagnostics":[{"file":"src/bad.py","severity":"error","message":"\"missing\" is not defined","range":{"start":{"line":0,"character":15},"end":{"line":0,"character":22}},"rule":"reportUndefinedVariable"}],"summary":{"filesAnalyzed":1,"errorCount":1,"warningCount":0,"informationCount":0,"timeInSec":1}}"#;
        let records = parse_pyright_json(input, Path::new("repo"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, "pyright");
        assert_eq!(records[0].code.as_deref(), Some("reportUndefinedVariable"));
        assert_eq!(records[0].start_line, 1);
    }

    #[test]
    fn parses_cmake_output() {
        let input = "CMake Error at CMakeLists.txt:3 (bad_command):\n  Unknown CMake command \"bad_command\".\n";
        let records = parse_cmake_output(input, Path::new("repo"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, "cmake");
        assert_eq!(records[0].severity, DiagnosticSeverity::Error);
        assert_eq!(records[0].code.as_deref(), Some("bad_command"));
        assert_eq!(records[0].start_line, 3);
    }

    #[test]
    fn parses_cmake_build_compiler_output() {
        let input = "src/main.cpp:4:9: error: use of undeclared identifier 'missing'\n";
        let records = parse_build_output(input, Path::new("repo"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, "cmake");
        assert_eq!(records[0].language, Language::Cpp);
        assert_eq!(records[0].severity, DiagnosticSeverity::Error);
        assert_eq!(records[0].start_column, 9);
    }
}
