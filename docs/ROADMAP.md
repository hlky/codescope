# Roadmap

## Done

- Rust CLI scaffold on `main`.
- Tree-sitter Python backend for functions, classes, assignments, references, callers, and context.
- Tree-sitter C-family backend for functions, classes, structs, enums, and rough variables.
- Lexical C-family fallback for function extraction and references.
- clangd LSP document symbols and references, with one clangd session per invocation.
- JSON/plain output and strict exit codes.
- Integration tests for the CLI contract.
- Codex skill packaging.

## Next

- Improve C++ qualified names for nested namespaces and out-of-class method definitions when tree-sitter is used without clangd.
- Add richer C-family caller detection using clangd call hierarchy where available.
- Add release packaging for Windows, macOS, and Linux.
- Publish binaries and update the install script to fetch platform-specific releases.
