# Changelog

## v0.1.3 - 2026-06-30

- Added CMake block result narrowing with `--contains`, `--around-line`, `--largest`, and `--smallest`.
- Expanded CMake target extraction to include generator-expression target references such as `$<TARGET_LINKER_FILE:target>`.

## v0.1.2 - 2026-06-30

- Added CMake file discovery for `CMakeLists.txt` and `*.cmake`.
- Added CMake variable extraction for `set`, `option`, `unset`, and mutating `list` commands.
- Added CMake block extraction for condition and loop regions with `extract-block`.
- Added CMake target extraction for target definitions and related target setup commands.
- Added CMake reference lookup for bare names and `${NAME}` references.
- Updated documentation, skill metadata, and tests for CMake workflows.

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
