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
```

## Common Flags

- `--json`: emit stable JSON records.
- `--max-matches N`: stop after `N` matches.
- `--lang python|c|cpp|c++|cuda|hip|cmake|markdown`: limit language search.
- `--kind function|class|struct|enum|variable|target|block|heading|all`: limit symbol kind where supported.
- `--backend auto|lsp|tree-sitter|lexical`: choose backend behavior.
- `--root PATH`: set project root for clangd.
- `--compile-commands-dir PATH`: pass a compilation database directory to clangd.

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
