# Usage

`codescope` searches source files under `--path` and emits either compact plain text or JSON records.

## Commands

```bash
codescope list-functions --path .
codescope extract-function --name Namespace::Class::method --path src
codescope extract-symbol --name Foo --kind class --path .
codescope extract-variable --name CONFIG --scope Foo --path .
codescope references --name foo --path .
codescope callers --name foo --path .
codescope context --name foo --path .
```

## Common Flags

- `--json`: emit stable JSON records.
- `--max-matches N`: stop after `N` matches.
- `--lang python|c|cpp|c++|cuda|hip`: limit language search.
- `--kind function|class|struct|enum|variable|all`: limit symbol kind where supported.
- `--backend auto|lsp|tree-sitter|lexical`: choose backend behavior.
- `--root PATH`: set project root for clangd.
- `--compile-commands-dir PATH`: pass a compilation database directory to clangd.

## Backends

Python uses tree-sitter for tolerant structural parsing.

C-family files use clangd in `auto` when available, then fall back to tree-sitter. `--backend lsp` requires clangd and exits non-zero if clangd cannot run. `--backend lexical` is a rough fallback for function extraction and references.

## Exit Codes

- `0`: found at least one match.
- `1`: no matches.
- `2`: CLI or configuration error.
- `3`: explicitly required backend failed.
