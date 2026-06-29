# Roadmap

## Done

- Rust CLI scaffold on `main`.
- Tree-sitter Python backend for functions, classes, assignments, references, callers, and context.
- Tree-sitter C-family backend for functions, classes, structs, enums, and rough variables.
- Lexical C-family fallback for function extraction and references.
- clangd LSP document symbols and references, with one clangd session per invocation.
- clangd call hierarchy for C-family callers when available.
- JSON/plain output and strict exit codes.
- Integration tests for the CLI contract.
- Codex skill packaging.
- Release workflow for Windows, macOS, and Linux binaries.

## Next

- Add broader real-world fixtures for templates, overloads, CUDA/HIP attributes, syntax-error files, and macro-heavy C++.
- Publish the first tagged release and update the install script to fetch platform-specific release artifacts by default.
- Add optional Python semantic resolution for imports and aliases when tree-sitter structural matching is too broad.
