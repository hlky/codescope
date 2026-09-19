# Usage

`codescope` searches source files under `--path` and emits either compact plain text or JSON records.

Windows paths in both formats use `/` separators without the extended-path prefix: `H:/directory/file.py` or `//server/share/file.py` for UNC paths.

## Commands

```bash
codescope list-functions --path .
codescope extract-function --name Namespace::Class::method --path src
codescope extract-symbol --name Foo --kind class --path .
codescope extract-variable --name CONFIG --scope Foo --path .
codescope list-headings --path docs
codescope extract-section --name Usage --path README.md
codescope references --name foo --path .
codescope callers --name foo --path .
codescope context --name foo --path .
```

## Common Flags

- `--json`: emit stable JSON records.
- `--max-matches N`: stop after `N` matches.
- `--lang python|rust|c|cpp|c++|cuda|hip|markdown`: limit language search.
- `--kind function|class|struct|enum|union|trait|module|type-alias|macro|variable|heading|all`: limit symbol kind where supported.
- `--backend auto|lsp|tree-sitter|lexical`: choose backend behavior.
- `--root PATH`: set the project root for clangd or rust-analyzer.
- `--compile-commands-dir PATH`: pass a compilation database directory to clangd.

## Backends

Python uses tree-sitter for tolerant structural parsing.

Rust uses rust-analyzer in `auto` when the executable is available, then falls back to tree-sitter. `--backend lsp` requires rust-analyzer and exits non-zero if it cannot run. Semantic references and callers resolve definitions across a Cargo workspace; codescope discovers the nearest parent `Cargo.toml` unless `--root` is provided. The tree-sitter fallback covers functions and methods, structs, enums, unions, traits, modules, type aliases, macros, constants, statics, fields, local bindings, and structural reference/caller matches. Rust qualified names use `::`, such as `module::Type::method`.

C-family files use clangd in `auto` when available, then fall back to tree-sitter. `--backend lsp` requires clangd and exits non-zero if clangd cannot run. `--backend lexical` is a rough fallback for functions, types, variables, and references.

Markdown uses tree-sitter for block parsing. `list-headings` returns heading records with qualified names based on heading nesting. `extract-section` returns the heading and its content until the next heading at the same or higher level. Fenced code headings are ignored by the parser.

## Exit Codes

- `0`: found at least one match.
- `1`: no matches.
- `2`: CLI or configuration error.
- `3`: explicitly required backend failed.

## Release Assets

Tagged releases publish platform archives plus `SHA256SUMS`. The Windows install script verifies the downloaded archive when checksums are available, then falls back to a local source build if release installation fails.
