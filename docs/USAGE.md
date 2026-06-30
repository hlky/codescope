# Usage

`codescope` searches source files under `--path` and emits either compact plain text or JSON records.

## Commands

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
codescope context --name foo --path .
codescope diagnostics --path .
codescope diagnostics --tool cargo --json --path .
codescope diagnostics --tool clangd --backend lsp --lang cpp --path .
codescope replace-text --find "old" --replace "new" --path . --preview
codescope replace-regex --find "old_(\\w+)" --replace "new_${1}" --path . --preview
codescope replace --name OldSymbol --with NewSymbol --kind function --path . --preview
codescope rename-symbol --from Foo --to Bar --path . --preview
codescope rewrite-import --from old.module --to new.module --path . --preview
codescope rewrite-markdown --heading-from "Old Title" --heading-to "New Title" --path docs --preview
codescope rewrite-markdown --link-from docs/old.md --link-to docs/new.md --path docs --preview
```

## Common Flags

- `--json`: emit stable JSON records.
- `--max-matches N`: stop after `N` matches.
- `--lang python|c|cpp|c++|cuda|hip|cmake|markdown`: limit language search.
- `--kind function|class|struct|enum|variable|target|block|heading|all`: limit symbol kind where supported.
- `--backend auto|lsp|tree-sitter|lexical`: choose backend behavior.
- `--root PATH`: set project root for clangd.
- `--compile-commands-dir PATH`: pass a compilation database directory to clangd.

## Diagnostics

`diagnostics` emits IDE-style errors and warnings as normalized records:

```bash
codescope diagnostics --path .
codescope diagnostics --file src/foo.cpp --json
codescope diagnostics --tool cargo --json --path .
codescope diagnostics --tool clangd --backend lsp --lang cpp --path .
codescope diagnostics --tool ruff --path .
codescope diagnostics --tool mypy --path .
codescope diagnostics --tool pyright --path .
codescope diagnostics --tool cmake --path .
```

Auto mode runs available relevant sources deterministically: Rust projects use `cargo check --message-format=json`; C-family files use clangd when available; Python files use available Ruff, mypy, and Pyright; CMake projects use `cmake -S/-B` plus `cmake --build` in a temporary build directory. Missing tools are skipped in auto mode. Explicit tool mode emits a `backend-error` diagnostic and exits with code `3` when the selected backend cannot run or times out.

Plain output is grouped by file. JSON diagnostics include `path`, `language`, `backend`, `tool`, `severity`, optional `code`, `message`, `start_line`, `start_column`, `end_line`, `end_column`, and optional `related` entries.

Each external diagnostics command has a 30 second timeout. CMake configure/build diagnostics parse CMake error/warning records plus common GCC/Clang and MSVC compiler diagnostics from build output.

## Edit Flags

All edit commands are previewable and diff-aware:

- `--preview`: print the planned edits without writing files. This is the default.
- `--apply`: write matching edits to disk.
- `--confirm`: with `--apply`, require `--path` to be in a clean Git worktree before writing.
- `--include GLOB`: only edit matching paths, for example `--include "*.py"`.
- `--exclude GLOB`: skip matching paths, for example `--exclude "vendor/*"`.
- `--max-files N`: fail instead of editing more than `N` files.
- `--lang python|c|cpp|c++|cuda|hip|cmake|markdown`: limit edits by file type.

## Edit Commands

`replace-text` performs literal text replacement:

```bash
codescope replace-text --find "old" --replace "new" --path . --preview
codescope replace-text --find "old" --replace "new" --path . --apply --confirm
```

`replace-regex` performs regex replacement with capture expansion:

```bash
codescope replace-regex --find "old_(\\w+)" --replace "new_${1}" --path src --preview
```

`replace` and `rename-symbol` perform identifier-boundary rewrites. When `--kind` is provided, `codescope` first verifies that a matching symbol of that kind exists before editing.

```bash
codescope replace --name OldSymbol --with NewSymbol --kind function --path . --preview
codescope rename-symbol --from Foo --to Bar --kind class --path . --preview
codescope rename-symbol --from OldNamespace --to NewNamespace --lang cpp --path include --preview
```

`rewrite-import` rewrites Python import/module paths while preserving `import` and `from` syntax:

```bash
codescope rewrite-import --from old.module --to new.module --path src --preview
```

`rewrite-markdown` updates Markdown headings or link targets:

```bash
codescope rewrite-markdown --heading-from "Old Title" --heading-to "New Title" --path docs --preview
codescope rewrite-markdown --link-from docs/old.md --link-to docs/new.md --path docs --preview
```

## Backends

Python uses tree-sitter for tolerant structural parsing.

C-family files use clangd in `auto` when available, then fall back to tree-sitter. `--backend lsp` requires clangd and exits non-zero if clangd cannot run. `--backend lexical` is a rough fallback for functions, types, variables, and references.

Markdown uses tree-sitter for block parsing. `list-headings` returns heading records with qualified names based on heading nesting. `extract-section` returns the heading and its content until the next heading at the same or higher level. Fenced code headings are ignored by the parser.

CMake uses lexical command parsing for `CMakeLists.txt` and `*.cmake`. `extract-variable` returns full `set(...)`, `option(...)`, `unset(...)`, and mutating `list(...)` commands for a variable. `extract-block` returns matched `if/foreach/function/macro/while` blocks by command name, full header, or argument token. Add `--contains TEXT` to narrow to nested blocks containing text, `--around-line N` to narrow to blocks containing a line, and `--largest` or `--smallest` to choose one block from the narrowed result. `extract-symbol --kind target` returns an `add_library(...)`, `add_executable(...)`, or `pybind11_add_module(...)` target with related `target_*`, `add_dependencies(...)`, `set_property(...)`, `install(TARGETS ...)`, and generator-expression references such as `$<TARGET_LINKER_FILE:target>`. `references` finds variable and target tokens, including `${VAR}` references.

## Exit Codes

- `0`: found at least one match.
- `1`: no matches.
- `2`: CLI or configuration error.
- `3`: explicitly required backend failed.

## Release Assets

Tagged releases publish platform archives plus `SHA256SUMS`. The Windows install script verifies the downloaded archive when checksums are available, then falls back to a local source build if release installation fails.
