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
```

The first production slice supports tree-sitter-backed Python extraction, clangd-backed C-family symbols and references, tree-sitter/lexical fallback for C, C++, CUDA, and HIP, lexical CMake command extraction, and tree-sitter-backed Markdown heading and section extraction.

Current implementation:

- Python structural parsing via tree-sitter.
- C-family semantic symbols/references via clangd LSP when available.
- C-family structural fallback via tree-sitter and lexical scanning.
- CMake variables, command blocks, narrowed block selection, targets, and references via lexical scanning.
- Markdown headings and sections via tree-sitter.
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
