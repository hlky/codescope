# codescope

`codescope` is a purpose-built Rust CLI for listing and extracting source symbols without opening large files.

Initial commands:

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
codescope replace-text --find "old" --replace "new" --path . --preview
codescope replace-regex --find "old_(\\w+)" --replace "new_${1}" --path . --preview
codescope replace --name OldSymbol --with NewSymbol --kind function --path . --preview
codescope rename-symbol --from Foo --to Bar --path . --preview
codescope rewrite-import --from old.module --to new.module --path . --preview
codescope rewrite-markdown --heading-from "Old Title" --heading-to "New Title" --path docs --preview
codescope rewrite-markdown --link-from docs/old.md --link-to docs/new.md --path docs --preview
```

The first production slice supports tree-sitter-backed Python extraction, clangd-backed C-family symbols and references, tree-sitter/lexical fallback for C, C++, CUDA, and HIP, lexical CMake command extraction, and tree-sitter-backed Markdown heading and section extraction.

Current implementation:

- Python structural parsing via tree-sitter.
- C-family semantic symbols/references via clangd LSP when available.
- C-family structural fallback via tree-sitter and lexical scanning.
- CMake variables, command blocks, narrowed block selection, targets, and references via lexical scanning.
- Markdown headings and sections via tree-sitter.
- Previewable, diff-aware edit operations for literal text, regexes, symbols, import/module paths, and Markdown headings/links.
- Codex skill packaging in `skill/SKILL.md`.

See [docs/USAGE.md](docs/USAGE.md) for command details.

## Install Locally

After a tagged release exists, install the Windows binary and skill metadata with:

```powershell
.\scripts\install-skill.ps1
```

To build from the current checkout instead:

```powershell
.\scripts\install-skill.ps1 -FromSource
```

The install script copies `codescope.exe` into `%USERPROFILE%\.codex\bin` and installs the skill metadata into `%USERPROFILE%\.codex\skills\codescope`. Release downloads are verified against `SHA256SUMS` when available.

## Exit Codes

- `0`: found at least one match
- `1`: no matches
- `2`: CLI or configuration error
- `3`: explicitly required backend failed

Edit commands default to preview mode. Add `--apply` to write changes, and add `--confirm` with `--apply` to require a clean Git worktree before writing so changes can be undone through Git. All edit commands support `--include`, `--exclude`, `--lang`, and `--max-files`.

## JSON Output

Use `--json` for stable machine-readable records:

```json
{
  "path": "src/example.cpp",
  "language": "cpp",
  "backend": "lexical",
  "kind": "function",
  "name": "method",
  "qualified_name": "Namespace::Class::method",
  "start_line": 10,
  "end_line": 42,
  "source": "..."
}
```

```json
{
  "path": "CMakeLists.txt",
  "language": "cmake",
  "backend": "lexical",
  "kind": "variable",
  "name": "MY_LIST",
  "qualified_name": "MY_LIST",
  "start_line": 10,
  "end_line": 18,
  "source": "set(MY_LIST ...)"
}
```

```json
{
  "path": "README.md",
  "language": "markdown",
  "backend": "tree-sitter",
  "kind": "heading",
  "name": "Installation",
  "qualified_name": "Usage.Installation",
  "start_line": 10,
  "end_line": 42,
  "source": "..."
}
```
