# Contributing

`codescope` is a Rust CLI for source-symbol extraction. Keep changes small, tested, and aligned with the existing backend boundaries.

## Local Checks

Run the same checks as CI before pushing:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo audit
cargo package --no-verify
```

Use `--allow-dirty` only for local package checks before committing.

## Release Checks

The release workflow builds Linux, macOS x86_64, macOS arm64, and Windows archives. A non-tag workflow dispatch validates artifact packaging and `SHA256SUMS` generation without publishing. Tags matching `v*` publish a GitHub release.

## Backend Expectations

- Python behavior is structural and tree-sitter based.
- C-family `--backend lsp` failures must be visible and exit non-zero.
- C-family `--backend auto` should fall back per file where practical.
- JSON output fields are part of the CLI contract and should remain stable.
