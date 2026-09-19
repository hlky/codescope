---
name: codescope
description: Inspect Python, Rust, C, C++, CUDA, HIP, and Markdown symbols or sections before opening large files.
---

# Codescope

List to discover names, then extract only the needed source. Set `--path` to the narrowest known file or directory.

```bash
codescope list-functions --path src --query parse
codescope extract-function --path src --name parse_config
codescope extract-symbol --path src --name Widget --kind class
codescope extract-variable --path src --name DEFAULT_LIMIT
codescope list-headings --path docs --query install
codescope extract-section --path README.md --name Usage.Installation
```

- Omit `--query` to list all functions or headings.
- `extract-function`: functions, methods, constructors, destructors, and GPU kernels.
- `extract-symbol`: types, classes, structs, enums, traits, modules, aliases, macros, or mixed symbol lookup; omit `--kind` when unsure.
- `extract-variable`: constants, globals, fields, and local bindings; use `--scope` to disambiguate.
- Use qualified names from list output: Python `Class.method`, Rust `module::Type::method`, Markdown `Parent.Child`.
- `extract-section` includes the heading through the next heading of equal or higher level; fenced-code headings are ignored.
- Narrow results with `--lang` or `--max-matches`; use `--json` for structured output.
- Default `--backend auto` uses available language servers with parser fallback. Use `codescope <command> --help` for backend options and other flags.
