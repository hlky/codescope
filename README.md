# codescope

`codescope` is a purpose-built Rust CLI for listing and extracting source symbols without opening large files.

Initial commands:

```bash
codescope list-functions --path .
codescope extract-function --name Namespace::Class::method --path src
codescope extract-symbol --name Foo --kind class --path .
codescope extract-variable --name CONFIG --scope Foo --path .
codescope extract-variable --name MY_LIST --lang cmake --path CMakeLists.txt
codescope extract-block --name ENABLE_FEATURE --lang cmake --path CMakeLists.txt
codescope extract-block --name ENABLE_FEATURE --contains generated_target --smallest --lang cmake --path CMakeLists.txt
codescope extract-symbol --name my_target --kind target --lang cmake --path CMakeLists.txt
codescope list-headings --path docs
codescope extract-section --name Usage --path README.md
codescope references --name foo --path .
codescope callers --name foo --path .
codescope definition --name Foo --path .
codescope definition --file src/foo.cpp --line 42 --column 17 --backend lsp
codescope type-of --file src/foo.py --line 42 --column 12 --json
codescope hover --file src/foo.cpp --line 42 --column 17 --backend lsp --json
codescope tests-for --name foo --path .
codescope tests-for --file src/foo.py --path . --json
codescope impact --name foo --path .
codescope impact --file src/foo.cpp --path . --json
codescope impact --file src/foo.cpp --changed-lines 10-30 --path .
codescope context --name foo --path .
codescope context-pack --name foo --path .
codescope context-pack --file src/foo.py --around-line 80 --path .
codescope diagnostics --path .
codescope diagnostics --tool cargo --json --path .
codescope diagnostics --tool clangd --backend lsp --lang cpp --path .
codescope diagnostics --tool ruff --path .
codescope diagnostics --tool mypy --path .
codescope diagnostics --tool pyright --path .
codescope diagnostics --tool cmake --path .
codescope replace-text --find "old" --replace "new" --path . --preview
codescope replace-regex --find "old_(\\w+)" --replace "new_${1}" --path . --preview
codescope replace --name OldSymbol --with NewSymbol --kind function --path . --preview
codescope rename-symbol --from Foo --to Bar --path . --preview
codescope rename-symbol --from Foo --to Bar --semantic --path . --preview
codescope rewrite-import --from old.module --to new.module --path . --preview
codescope rewrite-markdown --heading-from "Old Title" --heading-to "New Title" --path docs --preview
codescope rewrite-markdown --link-from docs/old.md --link-to docs/new.md --path docs --preview
```

The first production slice supports tree-sitter-backed Python extraction, clangd-backed C-family symbols, references, and IDE-style navigation, heuristic test discovery and change impact reports, tree-sitter/lexical fallback for C, C++, CUDA, and HIP, ranked context packs, cargo/clangd diagnostics, lexical CMake command extraction, and tree-sitter-backed Markdown heading and section extraction.

Current implementation:

- Python structural parsing via tree-sitter.
- C-family semantic symbols/references plus definition, type, and hover navigation via clangd LSP when available.
- Python structural definition navigation for functions, classes, variables, and imports, with best-effort `type-of` and `hover` summaries.
- Heuristic `tests-for` discovery by symbol or file, including Python test symbols, C-family test macros, and CMake `add_test(...)` entries.
- `impact` reports for a symbol, file, or changed line range, combining definitions, references, callers, callees, tests, docs, CMake target associations, confidence, and notes.
- C-family structural fallback via tree-sitter and lexical scanning.
- Ranked `context-pack` output for a symbol or file line, combining definitions, imports/includes, callers, references, nearby tests, docs, CMake metadata, diagnostics, and notes under an approximate source-character budget.
- Normalized diagnostics from `cargo check --message-format=json`, clangd LSP, Ruff, mypy, Pyright, and CMake configure/build output.
- CMake variables, command blocks, narrowed block selection, targets, and references via lexical scanning.
- Markdown headings and sections via tree-sitter.
- Previewable, diff-aware edit operations for literal text, regexes, symbols, semantic renames, import/module paths, and Markdown headings/links.
- Codex skill packaging in `skill/SKILL.md`.

See [docs/USAGE.md](docs/USAGE.md) for command details.

## Install Locally

After a tagged release exists, install the Windows binary and skill metadata with:

```powershell
.\scripts\install-skill.ps1
```

To build from the current checkout instead:

```powershell
.\scripts\install-skill.ps1 -FromSource
```

The install script copies `codescope.exe` into `%USERPROFILE%\.codex\bin` and installs the skill metadata into `%USERPROFILE%\.codex\skills\codescope`. Release downloads are verified against `SHA256SUMS` when available.

## Exit Codes

- `0`: found at least one match
- `1`: no matches
- `2`: CLI or configuration error
- `3`: explicitly required backend failed

Edit commands default to preview mode. Add `--apply` to write changes, and add `--confirm` with `--apply` to require a clean Git worktree before writing so changes can be undone through Git. All edit commands support `--include`, `--exclude`, `--lang`, `--max-files`, and `--json`.

`rename-symbol` preserves its identifier-boundary rewrite behavior by default. Add `--semantic` for a stricter refactor preview: Python uses tree-sitter identifier nodes for definitions and references, C-family files use clangd rename, and comments/strings or other textual matches outside the safe edit set are reported as skipped. C-family semantic rename exits with code `3` when clangd cannot run or cannot produce a safe rename.

## JSON Output

Use `--json` for stable machine-readable records:

```json
{
  "path": "src/example.cpp",
  "language": "cpp",
  "backend": "lexical",
  "kind": "function",
  "name": "method",
  "qualified_name": "Namespace::Class::method",
  "start_line": 10,
  "end_line": 42,
  "source": "..."
}
```

```json
{
  "path": "CMakeLists.txt",
  "language": "cmake",
  "backend": "lexical",
  "kind": "variable",
  "name": "MY_LIST",
  "qualified_name": "MY_LIST",
  "start_line": 10,
  "end_line": 18,
  "source": "set(MY_LIST ...)"
}
```

```json
{
  "path": "README.md",
  "language": "markdown",
  "backend": "tree-sitter",
  "kind": "heading",
  "name": "Installation",
  "qualified_name": "Usage.Installation",
  "start_line": 10,
  "end_line": 42,
  "source": "..."
}
```

Diagnostics records use a separate normalized shape:

```json
{
  "path": "src/lib.rs",
  "language": "rust",
  "backend": "cargo",
  "tool": "cargo",
  "severity": "error",
  "code": "E0425",
  "message": "cannot find value `missing` in this scope",
  "start_line": 1,
  "start_column": 16,
  "end_line": 1,
  "end_column": 23
}
```

Explicit diagnostics tool failures, including missing tools and timeouts, are emitted as `backend-error` records and exit with code `3`.

Related test records from `tests-for` include the candidate test location, heuristic reason, score, and source snippet:

```json
{
  "path": "tests/test_example.py",
  "language": "python",
  "backend": "tree-sitter",
  "test_name": "test_helper",
  "qualified_name": "test_helper",
  "start_line": 3,
  "end_line": 4,
  "reason": "test source references subject",
  "score": 90,
  "source": "def test_helper():\n    assert helper() == 1\n"
}
```

`tests-for` is heuristic. Verify the reported matches before treating them as exhaustive.

`impact --json` emits a grouped report with `subject`, `definitions`, `references`, `callers`, `callees`, `tests`, `docs`, `build_targets`, `diagnostics`, `confidence`, and `notes`. Each entry includes `path`, `start_line`, `end_line`, `language`, `backend`, `kind`, `name`, `qualified_name`, `reason`, and `source`.

Navigation records from `definition`, `type-of`, and `hover` include line and column ranges:

```json
{
  "path": "src/example.cpp",
  "language": "cpp",
  "backend": "clangd",
  "kind": "definition",
  "name": "helper",
  "qualified_name": "helper",
  "start_line": 12,
  "start_column": 5,
  "end_line": 14,
  "end_column": 2,
  "source": "int helper() { ... }",
  "detail": "hover or best-effort type text"
}
```

`context-pack --json` emits a pack with `subject`, `budget`, ranked `items`, whole-item `omitted` entries, and `notes`. Each item includes `role`, `path`, `start_line`, `end_line`, `language`, `backend`, `score`, `reason`, and `source`.
