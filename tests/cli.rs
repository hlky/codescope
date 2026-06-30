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

The helper function is used by Widget.build and caller.

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
    std::fs::create_dir(dir.path().join("tests")).unwrap();
    std::fs::write(
        dir.path().join("tests").join("test_sample.py"),
        r#"
from sample import helper

def test_helper():
    assert helper() == 1
"#
        .trim_start(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("CMakeLists.txt"),
        r#"
cmake_minimum_required(VERSION 3.20)
project(Sample)

set(SAMPLE_OPS
    alpha
    beta
)

list(APPEND SAMPLE_OPS
    gamma
)

if(ENABLE_ACCELERATOR)
    add_library(sample_core STATIC sample.cpp)
    target_link_libraries(sample_core PRIVATE dependency)
    list(APPEND SAMPLE_TARGETS sample_core)
    foreach(item IN LISTS SAMPLE_OPS)
        list(APPEND SAMPLE_TARGETS "${item}")
    endforeach()
endif()

foreach(item IN LISTS SAMPLE_OPS)
    set(sample_generated_target "sample_generated_${item}")
    add_library(${sample_generated_target} STATIC generated.cpp)
    target_include_directories(${sample_generated_target} PRIVATE include)
    set_target_properties(${sample_generated_target} PROPERTIES OUTPUT_NAME generated)
    add_custom_command(TARGET ${sample_generated_target} POST_BUILD COMMAND echo done)
    add_executable(sample_tool ${item}.cpp)
    target_link_libraries(sample_tool PRIVATE $<TARGET_LINKER_FILE:sample_core>)
endforeach()

enable_testing()
add_test(NAME sample_helper COMMAND sample_tool --case helper)
"#
        .trim_start(),
    )
    .unwrap();
    dir
}

#[test]
fn version_exits_successfully() {
    Command::cargo_bin("codescope")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("codescope"));
}

#[test]
fn workspace_map_json_reports_languages_and_cmake_targets() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let output = Command::cargo_bin("codescope")
        .unwrap()
        .args(["workspace-map", "--json", "--path"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["language"] == "python" && entry["files"] == 2)
    );
    assert!(
        value["targets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "sample_core")
    );
    assert!(
        value["build_systems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"] == "Cargo.toml")
    );
}

#[test]
fn workspace_map_plain_is_concise() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["workspace-map", "--max-targets", "1", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("# Workspace Map"))
        .stdout(predicate::str::contains("languages:"))
        .stdout(predicate::str::contains("targets:"))
        .stdout(predicate::str::contains("target list truncated"));
}

#[test]
fn replace_text_previews_diff_without_modifying_files() {
    let dir = fixture();
    let file = dir.path().join("sample.py");
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "replace-text",
            "--find",
            "helper",
            "--replace",
            "assist",
            "--lang",
            "python",
            "--preview",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("preview:"))
        .stdout(predicate::str::contains("-def helper():"))
        .stdout(predicate::str::contains("+def assist():"));

    let text = std::fs::read_to_string(file).unwrap();
    assert!(text.contains("def helper():"));
    assert!(!text.contains("def assist():"));
}

#[test]
fn replace_text_apply_respects_include_exclude_and_max_files() {
    let dir = fixture();
    std::fs::write(dir.path().join("notes.md"), "old\n").unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "replace-text",
            "--find",
            "old",
            "--replace",
            "new",
            "--include",
            "*.md",
            "--exclude",
            "README.md",
            "--max-files",
            "1",
            "--apply",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "applied: 1 replacements across 1 files",
        ));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.md")).unwrap(),
        "new\n"
    );
    assert!(
        std::fs::read_to_string(dir.path().join("README.md"))
            .unwrap()
            .contains("overview")
    );
}

#[test]
fn replace_regex_supports_capture_expansion() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("sample.py"), "value_12 = 1\n").unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "replace-regex",
            "--find",
            "value_(\\d+)",
            "--replace",
            "item_${1}",
            "--apply",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(dir.path().join("sample.py")).unwrap(),
        "item_12 = 1\n"
    );
}

#[test]
fn rename_symbol_uses_identifier_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sample.py"),
        "def Foo():\n    return Foo()\n\ndef Foobar():\n    return 1\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "rename-symbol",
            "--from",
            "Foo",
            "--to",
            "Bar",
            "--kind",
            "function",
            "--apply",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success();

    let text = std::fs::read_to_string(dir.path().join("sample.py")).unwrap();
    assert!(text.contains("def Bar():"));
    assert!(text.contains("return Bar()"));
    assert!(text.contains("def Foobar():"));
}

#[test]
fn semantic_python_rename_changes_definition_and_call_sites() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sample.py"),
        "def Foo():\n    return 1\n\nvalue = Foo()\ntext = \"Foo\"\n# Foo stays visible\n\ndef Foobar():\n    return Foo()\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "rename-symbol",
            "--from",
            "Foo",
            "--to",
            "Bar",
            "--semantic",
            "--apply",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("definitions changed: 1"))
        .stdout(predicate::str::contains("skipped matches: 2"));

    let text = std::fs::read_to_string(dir.path().join("sample.py")).unwrap();
    assert!(text.contains("def Bar():"));
    assert!(text.contains("value = Bar()"));
    assert!(text.contains("return Bar()"));
    assert!(text.contains("def Foobar():"));
    assert!(text.contains("text = \"Foo\""));
    assert!(text.contains("# Foo stays visible"));
}

#[test]
fn semantic_python_rename_json_reports_skipped_matches() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sample.py"),
        "def Foo():\n    return Foo()\n\nnote = 'Foo'\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "rename-symbol",
            "--from",
            "Foo",
            "--to",
            "Bar",
            "--semantic",
            "--json",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""backend": "tree-sitter-python""#,
        ))
        .stdout(predicate::str::contains(r#""definitions_changed": 1"#))
        .stdout(predicate::str::contains(r#""skipped_matches""#))
        .stdout(predicate::str::contains(
            "textual identifier match outside semantic edit set",
        ));
}

#[test]
fn semantic_cfamily_rename_fails_when_clangd_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("sample.cpp"), "int Foo() { return 1; }\n").unwrap();

    let mut command = Command::cargo_bin("codescope").unwrap();
    command.env("PATH", "");
    command
        .args([
            "rename-symbol",
            "--from",
            "Foo",
            "--to",
            "Bar",
            "--semantic",
            "--lang",
            "cpp",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .code(3);
}

#[test]
fn semantic_cfamily_rename_uses_clangd_when_available() {
    if which::which("clangd").is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sample.cpp"),
        "int Foo() { return 1; }\nint caller() { return Foo(); }\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "rename-symbol",
            "--from",
            "Foo",
            "--to",
            "Bar",
            "--semantic",
            "--lang",
            "cpp",
            "--preview",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("backend: clangd"))
        .stdout(
            predicate::str::contains("+int Bar()")
                .or(predicate::str::contains("+int caller() { return Bar(); }")),
        );
}

#[test]
fn rewrite_import_preserves_import_syntax() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sample.py"),
        "import old.module\nfrom old.module import thing\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "rewrite-import",
            "--from",
            "old.module",
            "--to",
            "new.module",
            "--apply",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(dir.path().join("sample.py")).unwrap(),
        "import new.module\nfrom new.module import thing\n"
    );
}

#[test]
fn rewrite_markdown_updates_headings_and_links() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("README.md"),
        "# Old Title\n[guide](docs/old.md#section)\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "rewrite-markdown",
            "--heading-from",
            "Old Title",
            "--heading-to",
            "New Title",
            "--apply",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "rewrite-markdown",
            "--link-from",
            "docs/old.md",
            "--link-to",
            "docs/new.md",
            "--apply",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "# New Title\n[guide](docs/new.md#section)\n"
    );
}

#[test]
fn diagnostics_cargo_json_emits_normalized_records() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"bad_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src").join("lib.rs"),
        "pub fn bad() { missing }\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["diagnostics", "--tool", "cargo", "--json", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""language": "rust""#))
        .stdout(predicate::str::contains(r#""tool": "cargo""#))
        .stdout(predicate::str::contains(r#""severity": "error""#))
        .stdout(predicate::str::contains("cannot find value"));
}

#[test]
fn tests_for_name_finds_python_test_referencing_symbol() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["tests-for", "--name", "helper", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("tests/test_sample.py"))
        .stdout(predicate::str::contains("test_helper"))
        .stdout(predicate::str::contains("reason:"))
        .stdout(predicate::str::contains("/sample.py:").not());
}

#[test]
fn tests_for_file_finds_named_python_test_file() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["tests-for", "--file", "sample.py", "--json", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""test_name": "test_helper""#))
        .stdout(predicate::str::contains(
            "test filename matches subject file",
        ))
        .stdout(predicate::str::contains(r#""language": "python""#));
}

#[test]
fn tests_for_reports_cmake_add_test_mapping() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["tests-for", "--name", "helper", "--lang", "cmake", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("sample_helper"))
        .stdout(predicate::str::contains(
            "CMake add_test references subject",
        ));
}

#[test]
fn related_file_finds_c_family_header_source_pair() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("include")).unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("include").join("foo.hpp"), "int foo();\n").unwrap();
    std::fs::write(
        dir.path().join("src").join("foo.cpp"),
        "#include \"foo.hpp\"\nint foo() { return 1; }\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["related", "--file", "include/foo.hpp", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("src/foo.cpp"))
        .stdout(predicate::str::contains("implementation"))
        .stdout(predicate::str::contains(
            "C-family header/source basename pair",
        ));
}

#[test]
fn related_file_finds_python_tests_and_docs() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["related", "--file", "sample.py", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("tests/test_sample.py"))
        .stdout(predicate::str::contains("test"))
        .stdout(predicate::str::contains("README.md"))
        .stdout(predicate::str::contains("doc"));
}

#[test]
fn related_file_finds_cmake_build_links() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["related", "--file", "sample.cpp", "--json", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""relationship": "build""#))
        .stdout(predicate::str::contains("CMakeLists.txt"))
        .stdout(predicate::str::contains(
            "CMake target references subject file",
        ));
}

#[test]
fn related_file_finds_markdown_links_and_backlinks() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "[Usage](docs/usage.md)\n").unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(
        dir.path().join("docs").join("usage.md"),
        "[Home](../README.md)\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["related", "--file", "README.md", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("docs/usage.md"))
        .stdout(predicate::str::contains("linked"));
}

#[test]
fn related_name_includes_definition_references_and_tests() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args(["related", "--name", "helper", "--json", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""relationship": "definition""#))
        .stdout(predicate::str::contains(r#""relationship": "reference""#))
        .stdout(predicate::str::contains(r#""relationship": "test""#));
}

#[test]
fn impact_name_reports_callers_references_tests_and_docs() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "impact",
            "--backend",
            "tree-sitter",
            "--name",
            "helper",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("# Impact: helper"))
        .stdout(predicate::str::contains("## references"))
        .stdout(predicate::str::contains("## callers"))
        .stdout(predicate::str::contains("caller"))
        .stdout(predicate::str::contains("## tests"))
        .stdout(predicate::str::contains("test_helper"))
        .stdout(predicate::str::contains("## docs"))
        .stdout(predicate::str::contains("README.md"));
}

#[test]
fn impact_file_reports_symbols_and_cmake_target_association() {
    let dir = fixture();
    let output = Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "impact",
            "--file",
            "sample.cpp",
            "--lang",
            "cpp",
            "--json",
            "--path",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["definitions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["qualified_name"].as_str().unwrap().contains("helper"))
    );
    assert!(
        value["build_targets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["qualified_name"].as_str() == Some("sample_core"))
    );
}

#[test]
fn impact_changed_lines_uses_enclosing_symbol() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "impact",
            "--file",
            "sample.py",
            "--changed-lines",
            "16-18",
            "--lang",
            "python",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("function, caller"))
        .stdout(predicate::str::contains("enclosing symbol"));
}

#[test]
fn diagnostics_auto_uses_available_cargo_source() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"bad_auto_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src").join("lib.rs"),
        "pub fn bad() { missing }\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["diagnostics", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("cargo"))
        .stdout(predicate::str::contains("cannot find value"));
}

#[test]
fn diagnostics_explicit_clangd_reports_backend_failure_when_missing() {
    let dir = fixture();
    let mut command = Command::cargo_bin("codescope").unwrap();
    command.env("PATH", "");
    command
        .args([
            "diagnostics",
            "--tool",
            "clangd",
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
fn diagnostics_clangd_reports_content_when_available() {
    if which::which("clangd").is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("bad.cpp"),
        "int main() { return missing; }\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "diagnostics",
            "--tool",
            "clangd",
            "--backend",
            "lsp",
            "--lang",
            "cpp",
            "--json",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""tool": "clangd""#))
        .stdout(predicate::str::contains("missing"));
}

#[test]
fn diagnostics_ruff_runs_when_available() {
    if which::which("ruff").is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.py"), "print(missing)\n").unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["diagnostics", "--tool", "ruff", "--json", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""tool": "ruff""#))
        .stdout(predicate::str::contains("F821"));
}

#[test]
fn diagnostics_mypy_runs_when_available() {
    if which::which("mypy").is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("bad.py"),
        "def bad() -> int:\n    return 'text'\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["diagnostics", "--tool", "mypy", "--json", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""tool": "mypy""#))
        .stdout(predicate::str::contains("return-value"));
}

#[test]
fn diagnostics_pyright_runs_when_available() {
    if which::which("pyright").is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.py"), "print(missing)\n").unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["diagnostics", "--tool", "pyright", "--json", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""tool": "pyright""#))
        .stdout(predicate::str::contains("reportUndefinedVariable"));
}

#[test]
fn diagnostics_cmake_runs_when_available() {
    if which::which("cmake").is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.20)\nproject(Bad)\nbad_command()\n",
    )
    .unwrap();

    let output = Command::cargo_bin("codescope")
        .unwrap()
        .args(["diagnostics", "--tool", "cmake", "--json", "--path"])
        .arg(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    if output.status.success() {
        assert!(stdout.contains(r#""tool": "cmake""#));
        assert!(stdout.contains("bad_command"));
    } else {
        assert_eq!(output.status.code(), Some(3));
        assert!(stdout.contains(r#""tool": "cmake""#));
        assert!(stdout.contains(r#""code": "backend-error""#));
    }
}

#[test]
fn diagnostics_explicit_missing_tool_outputs_json_backend_error() {
    let dir = fixture();
    let mut command = Command::cargo_bin("codescope").unwrap();
    command.env("PATH", "");
    command
        .args(["diagnostics", "--tool", "pyright", "--json", "--path"])
        .arg(dir.path())
        .assert()
        .code(3)
        .stdout(predicate::str::contains(r#""code": "backend-error""#))
        .stdout(predicate::str::contains(r#""tool": "pyright""#));
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
fn list_functions_lists_all_functions_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let mut text = String::new();
    for idx in 1..=25 {
        text.push_str(&format!("def f{idx}():\n    return {idx}\n\n"));
    }
    std::fs::write(dir.path().join("sample.py"), text).unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["list-functions", "--backend", "tree-sitter", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("f25"));

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "list-functions",
            "--backend",
            "tree-sitter",
            "--max-matches",
            "20",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("f20"))
        .stdout(predicate::str::contains("f21").not());
}

#[test]
fn list_functions_query_scans_past_default_match_cap() {
    let dir = tempfile::tempdir().unwrap();
    let mut early = String::new();
    for idx in 1..=25 {
        early.push_str(&format!("def f{idx}():\n    return {idx}\n\n"));
    }
    std::fs::write(dir.path().join("a.py"), early).unwrap();
    std::fs::write(
        dir.path().join("z.py"),
        "def target_late():\n    return 1\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "list-functions",
            "--backend",
            "tree-sitter",
            "--query",
            "target_late",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("target_late"));
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
fn list_headings_lists_all_markdown_headings_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let mut text = String::from("# Project\n\n");
    for idx in 1..=25 {
        text.push_str(&format!("## {idx}. Section {idx}\nbody {idx}\n\n"));
    }
    std::fs::write(dir.path().join("README.md"), text).unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["list-headings", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("25. Section 25"));

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["list-headings", "--max-matches", "20", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("19. Section 19"))
        .stdout(predicate::str::contains("20. Section 20").not());
}

#[test]
fn extract_section_accepts_numbered_markdown_heading_shorthand() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("README.md"),
        "# Project\n\n## 14. Skip/defer list\nbody\n\n## 15. Final checklist\nnext\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args(["extract-section", "--name", "14", "--path"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("## 14. Skip/defer list"))
        .stdout(predicate::str::contains("## 15. Final checklist").not());
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
fn cmake_variables_blocks_targets_and_references_can_be_extracted() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-variable",
            "--name",
            "SAMPLE_OPS",
            "--lang",
            "cmake",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("set(SAMPLE_OPS"))
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("list(APPEND SAMPLE_OPS"));

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-block",
            "--name",
            "ENABLE_ACCELERATOR",
            "--lang",
            "cmake",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("if(ENABLE_ACCELERATOR)"))
        .stdout(predicate::str::contains("endif()"));

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-block",
            "--name",
            "ENABLE_ACCELERATOR",
            "--contains",
            "SAMPLE_TARGETS",
            "--smallest",
            "--lang",
            "cmake",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "foreach(item IN LISTS SAMPLE_OPS)",
        ))
        .stdout(predicate::str::contains("if(ENABLE_ACCELERATOR)").not());

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-block",
            "--name",
            "ENABLE_ACCELERATOR",
            "--around-line",
            "17",
            "--largest",
            "--lang",
            "cmake",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("if(ENABLE_ACCELERATOR)"));

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-symbol",
            "--kind",
            "target",
            "--name",
            "sample_core",
            "--lang",
            "cmake",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("add_library(sample_core"))
        .stdout(predicate::str::contains(
            "target_link_libraries(sample_core",
        ))
        .stdout(predicate::str::contains(
            "$<TARGET_LINKER_FILE:sample_core>",
        ));

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "references",
            "--name",
            "SAMPLE_OPS",
            "--lang",
            "cmake",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("reference"));

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "extract-symbol",
            "--kind",
            "target",
            "--name",
            "sample_generated_target",
            "--lang",
            "cmake",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "add_library(${sample_generated_target}",
        ))
        .stdout(predicate::str::contains(
            "target_include_directories(${sample_generated_target}",
        ))
        .stdout(predicate::str::contains(
            "set_target_properties(${sample_generated_target}",
        ))
        .stdout(predicate::str::contains(
            "add_custom_command(TARGET ${sample_generated_target}",
        ));
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
fn callees_finds_direct_python_call() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "callees",
            "--backend",
            "tree-sitter",
            "--name",
            "caller",
            "--lang",
            "python",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("helper"));
}

#[test]
fn callgraph_json_has_stable_nodes_edges_and_depth() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.py"),
        "def helper():\n    return 1\n\ndef service():\n    return helper()\n\ndef handler():\n    return service()\n\ndef route():\n    return handler()\n",
    )
    .unwrap();

    let output = Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "callgraph",
            "--name",
            "handler",
            "--depth",
            "2",
            "--direction",
            "both",
            "--json",
            "--path",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["qualified_name"] == "handler")
    );
    assert!(
        value["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "calls")
    );
    assert!(
        value["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "called_by")
    );
}

#[test]
fn callgraph_handles_recursive_cycle() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.py"),
        "def loop():\n    return loop()\n",
    )
    .unwrap();

    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "callgraph",
            "--name",
            "loop",
            "--depth",
            "3",
            "--json",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""kind": "calls""#));
}

#[test]
fn dataflow_cmake_reports_writes_mutations_and_reads() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "dataflow",
            "--name",
            "SAMPLE_OPS",
            "--lang",
            "cmake",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("## writes"))
        .stdout(predicate::str::contains("## mutates"))
        .stdout(predicate::str::contains("## reads"))
        .stdout(predicate::str::contains("SAMPLE_OPS"));
}

#[test]
fn definition_name_finds_python_structural_symbol() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "definition",
            "--name",
            "helper",
            "--lang",
            "python",
            "--json",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""kind": "definition""#))
        .stdout(predicate::str::contains("def helper"));
}

#[test]
fn definition_position_finds_python_structural_symbol() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "definition",
            "--file",
            "sample.py",
            "--line",
            "17",
            "--column",
            "12",
            "--lang",
            "python",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("def helper"));
}

#[test]
fn definition_name_finds_python_import() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "definition",
            "--name",
            "Path",
            "--lang",
            "python",
            "--json",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""kind": "definition""#))
        .stdout(predicate::str::contains("from pathlib import Path"));
}

#[test]
fn type_of_python_is_best_effort() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "type-of", "--name", "CONFIG", "--lang", "python", "--json", "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""kind": "type""#))
        .stdout(predicate::str::contains("best-effort"));
}

#[test]
fn navigation_position_requires_column() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "definition",
            "--file",
            "sample.py",
            "--line",
            "17",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--file, --line, and --column"));
}

#[test]
fn explicit_lsp_definition_exits_three_when_clangd_missing() {
    let dir = fixture();
    let mut command = Command::cargo_bin("codescope").unwrap();
    command.env("PATH", "");
    command
        .args([
            "definition",
            "--file",
            "sample.cpp",
            "--line",
            "18",
            "--column",
            "5",
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
fn lsp_definition_runs_when_clangd_is_available() {
    if which::which("clangd").is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("simple.cpp"),
        "int helper() { return 1; }\nint caller() { return helper(); }\n",
    )
    .unwrap();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "definition",
            "--file",
            "simple.cpp",
            "--line",
            "2",
            "--column",
            "23",
            "--backend",
            "lsp",
            "--lang",
            "cpp",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("clangd"));
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
fn context_pack_ranks_definition_imports_callers_and_references() {
    let dir = fixture();
    let output = Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "context-pack",
            "--backend",
            "tree-sitter",
            "--name",
            "helper",
            "--lang",
            "python",
            "--path",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let definition = stdout.find("## definition").unwrap();
    let imports = stdout.find("## imports").unwrap();
    let caller = stdout.find("## caller").unwrap();
    let reference = stdout.find("## reference").unwrap();
    assert!(definition < imports);
    assert!(imports < caller);
    assert!(caller < reference);
    assert!(stdout.contains("import os"));
    assert!(stdout.contains("def helper"));
}

#[test]
fn context_pack_budget_omits_lower_ranked_items() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "context-pack",
            "--backend",
            "tree-sitter",
            "--name",
            "helper",
            "--lang",
            "python",
            "--budget",
            "80",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("## definition"))
        .stdout(predicate::str::contains("## imports"))
        .stdout(predicate::str::contains("## Omitted"))
        .stdout(predicate::str::contains("caller"));
}

#[test]
fn context_pack_json_has_stable_roles() {
    let dir = fixture();
    let output = Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "context-pack",
            "--backend",
            "tree-sitter",
            "--name",
            "helper",
            "--lang",
            "python",
            "--json",
            "--path",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let roles = value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["role"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(roles.first().copied(), Some("definition"));
    assert!(roles.contains(&"imports"));
    assert!(roles.contains(&"caller"));
    assert!(roles.contains(&"reference"));
}

#[test]
fn context_pack_accepts_multiple_path_roots() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::write(
        dir.path().join("src").join("signal.py"),
        "def stft(x):\n    return x\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests").join("test_signal.py"),
        "from signal import stft\n\ndef test_stft():\n    assert stft(1) == 1\n",
    )
    .unwrap();

    let output = Command::cargo_bin("codescope")
        .unwrap()
        .current_dir(dir.path())
        .args([
            "context-pack",
            "--backend",
            "tree-sitter",
            "--name",
            "stft",
            "--json",
            "--path",
            "src",
            "tests",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let roles = value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["role"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(roles.contains(&"definition"));
    assert!(roles.contains(&"caller"));
}

#[test]
fn context_pack_file_around_line_uses_enclosing_symbol() {
    let dir = fixture();
    Command::cargo_bin("codescope")
        .unwrap()
        .args([
            "context-pack",
            "--backend",
            "tree-sitter",
            "--file",
            "sample.py",
            "--around-line",
            "16",
            "--lang",
            "python",
            "--path",
        ])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("## enclosing-symbol"))
        .stdout(predicate::str::contains("def caller"));
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
