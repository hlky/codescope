# Changelog

## v0.1.20 - 2026-07-14

- Restored the focused v0.1.1 command surface after later features made agent usage less predictable.
- Ensured clangd child processes are terminated and reaped when LSP operations fail or are interrupted.

## v0.1.19 - 2026-06-30

- Fixed `list-headings` to list all Markdown headings by default instead of silently stopping at the shared default match cap, while still honoring explicit `--max-matches`.
- Added numbered Markdown heading shorthand for `extract-section`, such as `--name 14` matching `## 14. ...`.
- Fixed `list-functions` to list all functions by default and to apply `--query` before `--max-matches`, so late matches are not hidden by earlier functions.

## v0.1.18 - 2026-06-30

- Fixed project traversal to skip common vendored dependency and agent metadata roots such as `.codex`, `.agents`, `third_party`, `vendor`, `external`, and `_deps`, reducing noisy and slow `context-pack` scans on real projects.
- Added support for multiple roots in symbol/search-style `--path` handling, including `context-pack --path src tests`.

## v0.1.17 - 2026-06-30

- Added `related` for ranked next-file discovery by file or symbol, covering definitions, references, tests, docs, C-family header/implementation pairs, CMake build links, Markdown links/backlinks, JSON output, docs, skill guidance, and CLI coverage.

## v0.1.16 - 2026-06-30

- Added `callees`, bounded `callgraph`, and `dataflow` graph slices with JSON node/edge output, plain grouped edge rendering, Python/CMake/C-family best-effort extraction, CLI coverage, docs, and skill guidance.

## v0.1.15 - 2026-06-30

- Added `workspace-map` for compact project overviews with language counts, source/test/doc roots, build files, CMake targets, tool availability, Git status, JSON output, docs, skill guidance, and CLI coverage.

## v0.1.14 - 2026-06-30

- Added `impact` reports for symbol, file, and changed-line subjects, combining definitions, references, callers, best-effort callees, related tests, Markdown docs, CMake target associations, confidence notes, JSON output, docs, skill guidance, and CLI coverage.

## v0.1.13 - 2026-06-30

- Added `rename-symbol --semantic` with tree-sitter Python definition/reference edits, clangd-backed C-family rename plans, skipped textual match reporting, JSON plan output, conflict detection, tests, docs, and skill guidance.

## v0.1.12 - 2026-06-30

- Fixed `tests-for` Python discovery to avoid reporting ordinary implementation symbols as tests when only the subject text matches.

## v0.1.11 - 2026-06-30

- Added `tests-for` heuristic test discovery by symbol or file, with scored reasons, JSON output, Python/C-family/CMake coverage, tests, docs, and skill guidance.

## v0.1.10 - 2026-06-30

- Added `definition`, `type-of`, and `hover` navigation commands with clangd-backed C-family position lookup, Python structural definition fallback, JSON column ranges, tests, docs, and skill guidance.

## v0.1.9 - 2026-06-30

- Added `context-pack` for ranked symbol or line-based editing context with JSON output, budget-based whole-item omission, tests/docs/build/diagnostics hooks, and skill guidance.

## v0.1.8 - 2026-06-30

- Fixed diagnostics tool timeout handling to drain child process output while waiting, avoiding blocked cargo/other verbose diagnostics runs.

## v0.1.7 - 2026-06-30

- Added normalized `diagnostics` output for cargo, clangd, Ruff, mypy, Pyright, and CMake configure/build diagnostics.
- Added deterministic diagnostics auto mode, JSON backend-error records, tool timeouts, documentation, skill guidance, and CLI coverage.

## v0.1.6 - 2026-06-30

- Normalized displayed Windows paths by removing verbatim `\\?\` prefixes and using `/` separators in plain, JSON, and edit preview output.

## v0.1.5 - 2026-06-30

- Fixed `--help` and `--version` to exit successfully after printing display output.

## v0.1.4 - 2026-06-30

- Added previewable, diff-aware edit commands for literal text replacement, regex replacement, symbol renames, import/module path rewrites, and Markdown heading/link rewrites.
- Added scoped edit safety flags: `--preview`, `--apply`, `--include`, `--exclude`, `--max-files`, and `--confirm`.
- Added CLI, documentation, and skill metadata coverage for agent IDE-style replacement workflows.

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
