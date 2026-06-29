# Changelog

## v0.1.1 - 2026-06-29

- Added tree-sitter backed Markdown heading discovery with `list-headings`.
- Added Markdown section extraction with `extract-section`.
- Added `markdown` language filtering and `heading` symbol kind support.
- Updated documentation and Codex skill metadata for Markdown workflows.

## v0.1.0 - 2026-06-29

Initial production release.

- Added `list-functions`, `extract-function`, `extract-symbol`, `extract-variable`, `references`, `callers`, and `context`.
- Added tree-sitter backed Python symbol extraction, references, callers, and context.
- Added clangd LSP backed C-family symbols, references, and callers.
- Added tree-sitter and lexical C-family fallback paths for C, C++, CUDA, and HIP files.
- Added stable JSON output, plain output, common filters, and strict exit codes.
- Added Codex `extract-function` skill packaging.
- Added CI, RustSec audit, release archives, and `SHA256SUMS`.
