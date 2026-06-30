---
name: codescope
description: Inspect Python, C, C++, CUDA, HIP, CMake, and Markdown symbols or sections before opening large files.
---

# Codescope

Use `codescope` when a task needs focused context for known or fuzzy Python, C, C++, CUDA, HIP, CMake, or Markdown symbols and sections.

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
codescope extract-variable --name MY_LIST --lang cmake --path CMakeLists.txt
codescope extract-block --name ENABLE_FEATURE --lang cmake --path CMakeLists.txt
codescope extract-block --name ENABLE_FEATURE --contains generated_target --smallest --lang cmake --path CMakeLists.txt
codescope extract-symbol --name my_target --kind target --lang cmake --path CMakeLists.txt
codescope list-headings --path docs --query install
codescope extract-section --name Usage.Installation --path README.md
codescope references --name parse_config --path src
codescope callers --name parse_config --path src
codescope context --name parse_config --path src
codescope replace-text --find "old" --replace "new" --path src --preview
codescope replace-regex --find "old_(\\w+)" --replace "new_${1}" --path src --preview
codescope replace --name OldSymbol --with NewSymbol --kind function --path src --preview
codescope rename-symbol --from Foo --to Bar --path src --preview
codescope rewrite-import --from old.module --to new.module --path src --preview
codescope rewrite-markdown --heading-from "Old Title" --heading-to "New Title" --path docs --preview
codescope rewrite-markdown --link-from docs/old.md --link-to docs/new.md --path docs --preview
```

## Behavior

- Python extraction uses tree-sitter and returns decorators plus the full `def` or `async def` body.
- Python names may be unqualified (`foo`) or qualified (`ClassName.foo`, `Outer.Inner.foo`).
- Python variables include module constants, class attributes, and local assignments.
- C-family extraction covers C, C++, CUDA (`.cu`, `.cuh`), and HIP (`.hip`) sources.
- C-family symbol, reference, and caller discovery uses clangd in `--backend auto` when available, with tree-sitter or lexical fallback.
- Use `--backend lsp` to require semantic C-family results, and pass `--compile-commands-dir` when the project has a non-default compilation database.
- Use `--root` when the clangd project root differs from the search `--path`.
- CMake extraction covers `CMakeLists.txt` and `*.cmake` files with `--lang cmake`.
- CMake variables include full `set(...)`, `option(...)`, `unset(...)`, and mutating `list(...)` commands.
- CMake blocks include matched `if`, `foreach`, `function`, `macro`, and `while` regions; `extract-block --name NAME` may match the command name, full header, or an argument token.
- CMake block extraction supports `--contains TEXT`, `--around-line N`, `--largest`, and `--smallest` to narrow broad condition matches to the relevant nested region.
- CMake targets include `add_library(...)`, `add_executable(...)`, and `pybind11_add_module(...)` definitions plus related `target_*`, `add_dependencies(...)`, `set_property(...)`, `install(TARGETS ...)`, and `$<TARGET_...:name>` generator-expression references.
- CMake references find bare names and `${NAME}` references.
- Markdown heading discovery uses tree-sitter and ignores fenced-code headings.
- Markdown headings have nested qualified names like `Usage.Installation`; `extract-section` returns the heading and content until the next heading at the same or higher level.
- Use `--lang markdown` to limit search to Markdown and `--kind heading` for heading symbols.
- Edit commands default to preview mode and print contextual diffs. Use `--apply` to write files and `--confirm` with `--apply` to require a clean Git worktree before editing.
- Edit commands support `--include`, `--exclude`, `--max-files`, and `--lang` for scoped, filetype-aware changes.
- Use `replace-text` for literal replacement and `replace-regex` for regex replacement with capture expansion.
- Use `replace --kind function|class|struct|enum|variable|target|block|heading` or `rename-symbol --kind ...` when a symbol should be verified before rewriting identifier-boundary matches.
- Use `rewrite-import` for Python import/module path changes.
- Use `rewrite-markdown` for Markdown heading text or link target rewrites.
- Use `--json` when stable fields are needed: `path`, `language`, `backend`, `kind`, `name`, `qualified_name`, `start_line`, `end_line`, and `source`.

## Agent Workflow

1. Use `codescope list-functions` when the exact function name is unknown or fuzzy.
2. Use `codescope extract-function` for a known function, method, constructor, destructor, CUDA kernel, or HIP kernel.
3. Use `codescope extract-symbol` for classes, structs, enums, and mixed symbol lookup.
4. Use `codescope extract-variable` for constants, globals, fields, and Python assignments; add `--scope` for class/function-scoped variables.
5. Use `codescope extract-variable --lang cmake` for focused CMake list or option definitions.
6. Use `codescope extract-block --lang cmake` for CMake condition or loop regions; add `--contains`, `--around-line`, `--largest`, or `--smallest` when a broad condition has many nested blocks.
7. Use `codescope extract-symbol --kind target --lang cmake` for CMake target setup.
8. Use `codescope list-headings` when the exact Markdown heading is unknown or fuzzy.
9. Use `codescope extract-section` for focused Markdown documentation context.
10. Use `codescope references` or `codescope callers` before opening broad call-site regions.
11. Use `codescope context` when a symbol plus imports/includes is enough context for reasoning.
12. Use edit commands with `--preview` first, use `--apply` to write files or `--confirm` with `--apply` to require a clean Git worktree before editing.
13. If `--backend lsp` fails, retry with `--backend auto` unless semantic clangd behavior is required.
