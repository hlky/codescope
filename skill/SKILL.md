---
name: codescope
description: Inspect Python, C, C++, CUDA, and HIP symbols before opening large source files.
---

# Codescope

Use `codescope` when a task needs focused source context for known or fuzzy Python, C, C++, CUDA, or HIP symbols.

## Quick Start

```bash
codescope extract-function --name FUNCTION_NAME --path .
```

Command selection:

```bash
codescope list-functions --path src --query parse
codescope extract-function --name ClassName.method_name --path src
codescope extract-symbol --name Widget --kind class --path src
codescope extract-variable --name DEFAULT_LIMIT --path src
codescope references --name parse_config --path src
codescope callers --name parse_config --path src
codescope context --name parse_config --path src
```

## Behavior

- Python extraction uses tree-sitter and returns decorators plus the full `def` or `async def` body.
- Python names may be unqualified (`foo`) or qualified (`ClassName.foo`, `Outer.Inner.foo`).
- Python variables include module constants, class attributes, and local assignments.
- C-family extraction covers C, C++, CUDA (`.cu`, `.cuh`), and HIP (`.hip`) sources.
- C-family symbol, reference, and caller discovery uses clangd in `--backend auto` when available, with tree-sitter or lexical fallback.
- Use `--backend lsp` to require semantic C-family results, and pass `--compile-commands-dir` when the project has a non-default compilation database.
- Use `--root` when the clangd project root differs from the search `--path`.
- Use `--json` when stable fields are needed: `path`, `language`, `backend`, `kind`, `name`, `qualified_name`, `start_line`, `end_line`, and `source`.

## Agent Workflow

1. Use `codescope list-functions` when the exact function name is unknown or fuzzy.
2. Use `codescope extract-function` for a known function, method, constructor, destructor, CUDA kernel, or HIP kernel.
3. Use `codescope extract-symbol` for classes, structs, enums, and mixed symbol lookup.
4. Use `codescope extract-variable` for constants, globals, fields, and Python assignments; add `--scope` for class/function-scoped variables.
5. Use `codescope references` or `codescope callers` before opening broad call-site regions.
6. Use `codescope context` when a symbol plus imports/includes is enough context for reasoning.
7. If `--backend lsp` fails, retry with `--backend auto` unless semantic clangd behavior is required.
