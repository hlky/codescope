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
"#
        .trim_start(),
    )
    .unwrap();
    dir
}

#[test]
fn list_functions_outputs_plain_records() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["list-functions", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("helper"))
        .stdout(predicate::str::contains("Widget.build"));
}

#[test]
fn extract_python_decorated_async_function_outputs_source() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["extract-function", "--name", "Widget.build", "--path"])
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
        .args(["references", "--name", "helper", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("reference"));

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["callers", "--name", "helper", "--path"])
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
        .args(["context", "--name", "helper", "--lang", "python", "--path"])
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

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["list-functions", "--backend", "lsp", "--path"])
        .arg(dir.path())
        .assert()
        .code(3);
}
