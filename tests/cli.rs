use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

#[cfg(windows)]
#[test]
fn windows_cli_returns_normalized_paths_in_text_and_json() {
    let dir = fixture();
    let path = dir.path().join("sample.py").canonicalize().unwrap();
    for args in [
        vec!["list-functions"],
        vec!["extract-function", "--name", "helper"],
        vec!["extract-function", "--name", "helper", "--json"],
    ] {
        let json = args.contains(&"--json");
        let extraction = args[0] == "extract-function";
        let result = Command::cargo_bin("codescope")
            .unwrap()
            .args(args)
            .args(["--backend", "tree-sitter", "--path"])
            .arg(&path)
            .assert()
            .success();
        let stdout = std::str::from_utf8(&result.get_output().stdout).unwrap();
        let output_path = if json {
            let values: serde_json::Value = serde_json::from_str(stdout).unwrap();
            values[0]["path"].as_str().unwrap().to_owned()
        } else {
            let header = stdout.lines().next().unwrap();
            let header = if extraction {
                header.strip_prefix("// ").unwrap()
            } else {
                header
            };
            header.split_once(".py:").unwrap().0.to_owned() + ".py"
        };
        assert!(!output_path.contains('\\'), "{output_path}");
        assert!(!output_path.starts_with("//?/"), "{output_path}");
        assert!(output_path.ends_with("/sample.py"), "{output_path}");
        assert_eq!(std::fs::canonicalize(output_path).unwrap(), path);
    }
}

fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sample.py"),
        r#"
import os
from pathlib import Path

CONFIG = 1

def helper():
    return CONFIG

class Widget:
    VALUE: int = 2

    @classmethod
    async def build(cls):
        return helper()

def caller():
    return helper()

def unrelated_build():
    build = 3
    return build
"#
        .trim_start(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("sample.cpp"),
        r#"
#include <vector>

namespace Demo {
class Thing {
public:
    int method() const {
        return helper();
    }
};
}

__global__ void kernel(int *out) {
    out[0] = 1;
}

int helper() {
    return 42;
}

int GLOBAL_COUNT = 7;
"#
        .trim_start(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("README.md"),
        r#"
# Project
overview

```markdown
## Not a heading
```

## Usage
steps

### Details
more

## API
reference
"#
        .trim_start(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("sample.rs"),
        r#"
use std::path::Path;

const GLOBAL_LIMIT: usize = 8;

mod engine {
    pub type Count = usize;

    pub union Value {
        pub integer: u64,
    }

    macro_rules! measure {
        ($value:expr) => { $value };
    }

    pub trait Runner {
        fn run(&self, path: &Path) -> usize;
    }

    pub struct Worker;

    impl Worker {
        pub const LIMIT: usize = 4;

        #[inline]
        pub async fn run(&self, path: &Path) -> usize {
            helper(path)
        }
    }

    fn helper(_path: &Path) -> usize {
        let value = super::GLOBAL_LIMIT;
        value
    }

    pub fn caller(path: &Path) -> usize {
        helper(path)
    }
}
"#
        .trim_start(),
    )
    .unwrap();
    dir
}

fn rust_lsp_fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        r#"[package]
name = "codescope-rust-lsp-fixture"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src").join("lib.rs"),
        r#"pub fn helper() -> usize {
    1
}

pub fn caller() -> usize {
    helper()
}

pub struct Worker;

impl Worker {
    pub fn run(&self) -> usize {
        helper()
    }
}
"#,
    )
    .unwrap();
    dir
}

#[test]
fn rust_functions_include_module_and_impl_scope() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-function",
            "--backend",
            "tree-sitter",
            "--name",
            "engine::Worker::run",
            "--lang",
            "rust",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("engine::Worker::run"))
        .stdout(predicate::str::contains("#[inline]"))
        .stdout(predicate::str::contains("pub async fn run"));
}

#[test]
fn rust_traits_can_be_extracted_by_kind() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-symbol",
            "--backend",
            "tree-sitter",
            "--name",
            "engine::Runner",
            "--kind",
            "trait",
            "--lang",
            "rust",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("trait, engine::Runner"));
}

#[test]
fn rust_extended_symbol_kinds_can_be_filtered() {
    let dir = fixture();
    for (kind, name) in [
        ("module", "engine"),
        ("type-alias", "engine::Count"),
        ("union", "engine::Value"),
        ("macro", "engine::measure"),
    ] {
        Command::cargo_bin("codescope")
            .unwrap()
            .args([
                "extract-symbol",
                "--backend",
                "tree-sitter",
                "--name",
                name,
                "--kind",
                kind,
                "--lang",
                "rust",
                "--path",
            ])
            .arg(dir.path())
            .assert()
            .success()
            .stdout(predicate::str::contains(format!("{kind}, {name}")));
    }
}

#[test]
fn rust_associated_constants_support_scope_filtering() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-variable",
            "--backend",
            "tree-sitter",
            "--name",
            "LIMIT",
            "--scope",
            "engine::Worker",
            "--lang",
            "rust",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("engine::Worker::LIMIT"));
}

#[test]
fn rust_local_bindings_support_scope_filtering() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-variable",
            "--backend",
            "tree-sitter",
            "--name",
            "value",
            "--scope",
            "engine::helper",
            "--lang",
            "rust",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("engine::helper::value"));
}

#[test]
fn rust_references_and_callers_use_structural_fallback() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "references",
            "--backend",
            "tree-sitter",
            "--name",
            "helper",
            "--lang",
            "rust",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("reference"));

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "callers",
            "--backend",
            "tree-sitter",
            "--name",
            "helper",
            "--lang",
            "rust",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("engine::caller"));
}

#[test]
fn rust_context_includes_use_declarations() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "context",
            "--backend",
            "tree-sitter",
            "--name",
            "engine::helper",
            "--lang",
            "rust",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("use std::path::Path;"));
}

#[test]
fn rust_lsp_backend_requires_rust_analyzer() {
    let dir = fixture();
    let mut command = Command::cargo_bin("codescope").unwrap();
    command.env("PATH", "");
    command
        .args([
            "list-functions",
            "--backend",
            "lsp",
            "--lang",
            "rust",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains("rust-analyzer"));
}

#[test]
fn rust_auto_backend_falls_back_when_rust_analyzer_is_unavailable() {
    let dir = fixture();
    let mut command = Command::cargo_bin("codescope").unwrap();
    command.env("PATH", "");
    command
        .args([
            "extract-function",
            "--backend",
            "auto",
            "--lang",
            "rust",
            "--name",
            "engine::helper",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "tree-sitter, function, engine::helper",
        ));
}

#[test]
fn rust_lsp_backend_lists_document_symbols_when_available() {
    if !codescope::lsp::rust_analyzer_available() {
        return;
    }
    let dir = rust_lsp_fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "list-functions",
            "--backend",
            "lsp",
            "--lang",
            "rust",
            "--path",
        ])
        .arg(dir.path().join("src"))
        .assert()
        .success()
        .stdout(predicate::str::contains("rust-analyzer, function, helper"))
        .stdout(predicate::str::contains(
            "rust, rust-analyzer, function, Worker::run)",
        ));
}

#[test]
fn rust_lsp_backend_resolves_references_when_available() {
    if !codescope::lsp::rust_analyzer_available() {
        return;
    }
    let dir = rust_lsp_fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "references",
            "--backend",
            "lsp",
            "--lang",
            "rust",
            "--name",
            "helper",
            "--path",
        ])
        .arg(dir.path().join("src"))
        .assert()
        .success()
        .stdout(predicate::str::contains("rust-analyzer, reference, helper"));
}

#[test]
fn rust_lsp_backend_resolves_callers_when_available() {
    if !codescope::lsp::rust_analyzer_available() {
        return;
    }
    let dir = rust_lsp_fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "callers",
            "--backend",
            "lsp",
            "--lang",
            "rust",
            "--name",
            "helper",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("rust-analyzer, function, caller"));
}

#[test]
fn tree_sitter_cfamily_qualified_names_include_scope() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-function",
            "--backend",
            "tree-sitter",
            "--name",
            "Demo::Thing::method",
            "--lang",
            "cpp",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Demo::Thing::method"));
}

#[test]
fn lexical_backend_extracts_types_and_variables() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-symbol",
            "--backend",
            "lexical",
            "--name",
            "Thing",
            "--kind",
            "class",
            "--lang",
            "cpp",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("class, Thing"));

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-variable",
            "--backend",
            "lexical",
            "--name",
            "GLOBAL_COUNT",
            "--lang",
            "cpp",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("GLOBAL_COUNT"));
}

#[test]
fn qualified_python_references_do_not_match_bare_identifiers_or_definitions() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "references",
            "--backend",
            "tree-sitter",
            "--name",
            "Widget.build",
            "--lang",
            "python",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .code(1);
}

#[test]
fn list_functions_outputs_plain_records() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["list-functions", "--backend", "tree-sitter", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("helper"))
        .stdout(predicate::str::contains("Widget.build"));
}

#[test]
fn markdown_headings_can_be_listed_and_sections_extracted() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["list-headings", "--lang", "markdown", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Project.Usage"))
        .stdout(predicate::str::contains("Not a heading").not());

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-section",
            "--name",
            "Project.Usage",
            "--lang",
            "markdown",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("### Details"))
        .stdout(predicate::str::contains("## API").not());
}

#[test]
fn extract_symbol_can_find_markdown_heading() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-symbol",
            "--kind",
            "heading",
            "--name",
            "Usage",
            "--lang",
            "markdown",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("heading, Project.Usage"));
}

#[test]
fn extract_python_decorated_async_function_outputs_source() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-function",
            "--backend",
            "tree-sitter",
            "--name",
            "Widget.build",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("@classmethod"))
        .stdout(predicate::str::contains("async def build"));
}

#[test]
fn extract_symbol_json_has_contract_fields() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-symbol",
            "--name",
            "Widget",
            "--kind",
            "class",
            "--backend",
            "tree-sitter",
            "--json",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""kind": "class""#))
        .stdout(predicate::str::contains(r#""backend": "tree-sitter""#));
}

#[test]
fn extract_variable_supports_scope_filter() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-variable",
            "--name",
            "VALUE",
            "--scope",
            "Widget",
            "--backend",
            "tree-sitter",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Widget.VALUE"));
}

#[test]
fn references_and_callers_work() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "references",
            "--backend",
            "tree-sitter",
            "--name",
            "helper",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("reference"));

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "callers",
            "--backend",
            "tree-sitter",
            "--name",
            "helper",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("caller"));
}

#[test]
fn context_includes_imports() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "context",
            "--backend",
            "tree-sitter",
            "--name",
            "helper",
            "--lang",
            "python",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("import os"))
        .stdout(predicate::str::contains("def helper"));
}

#[test]
fn no_match_exits_one_and_lsp_exits_three() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["extract-function", "--name", "missing_symbol", "--path"])
        .arg(dir.path())
        .assert()
        .code(1);

    let mut lsp = Command::cargo_bin("codescope").unwrap();
    lsp.env("PATH", "");
    lsp.args([
        "list-functions",
        "--backend",
        "lsp",
        "--lang",
        "cpp",
        "--path",
    ])
    .arg(dir.path())
    .assert()
    .code(3);
}

#[test]
fn lsp_backend_runs_when_clangd_is_available() {
    if which::which("clangd").is_err() {
        return;
    }
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "list-functions",
            "--backend",
            "lsp",
            "--lang",
            "cpp",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("helper"));
}

#[test]
fn lsp_callers_use_clangd_call_hierarchy_when_available() {
    if which::which("clangd").is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("simple.cpp"),
        r#"
namespace Demo {
int helper() { return 1; }
int caller() { return helper(); }
}
"#
        .trim_start(),
    )
    .unwrap();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "callers",
            "--backend",
            "lsp",
            "--lang",
            "cpp",
            "--name",
            "helper",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("clangd"))
        .stdout(predicate::str::contains("method").or(predicate::str::contains("caller")));
}
