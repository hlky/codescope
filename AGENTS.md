# Agent Notes

## Release Flow

When asked to ship a change, follow the full project release loop:

1. Implement and test locally.
2. Run:
   - `cargo fmt --check`
   - `cargo test`
   - `cargo clippy --all-targets -- -D warnings`
3. If releasing, bump patch version in:
   - `Cargo.toml`
   - `Cargo.lock` via `cargo check`
   - `CHANGELOG.md`
4. Commit on `main` with a concise message.
5. Push `main`.
6. Watch CI:
   - `gh run list --branch main --limit 5`
   - `gh run watch <run-id> --exit-status`
7. If CI passes, create annotated tag:
   - `git tag -a vX.Y.Z -m "Release vX.Y.Z"`
   - `git push origin vX.Y.Z`
8. Watch release workflow:
   - `gh run list --limit 5`
   - `gh run watch <run-id> --exit-status`
9. Install exact released version locally:
   - `./scripts/install-skill.ps1 -Version vX.Y.Z`
10. Verify:
   - `~/.codex/bin/codescope.exe --version`
   - relevant smoke command against a real file.

## Local Install

Use the release installer by explicit tag, not `latest`, to avoid GitHub cache timing:

```powershell
./scripts/install-skill.ps1 -Version vX.Y.Z
```

The installer updates both:
- `~/.codex/bin/codescope.exe`
- `~/.codex/skills/codescope/SKILL.md`

## Feature Checklist

For new CLI features, update all of:

- `src/*` implementation
- `tests/cli.rs`
- `README.md`
- `docs/USAGE.md`
- `skill/SKILL.md`
- `CHANGELOG.md` if release-bound

Keep examples generic; do not bake downstream project-specific names that you may be given as an example into docs or tests.
