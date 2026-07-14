use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

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
    dir
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
