# Contributing to `note`

Thanks for your interest! This project is small and opinionated; the rules
below keep it consistent. The complete engineering directives — the stack,
architectural invariants, and milestone plan — live in
[`CLAUDE.md`](CLAUDE.md). This file is the short version for contributors.

## Getting set up

```bash
git clone https://github.com/LLawli/note
cd note
cargo build --workspace
```

The toolchain is pinned in `rust-toolchain.toml` and installed automatically by
`rustup`.

### Enable the git hooks (once per clone)

Hooks live in the versioned `.githooks/` directory: `pre-commit` runs format +
lints, `pre-push` runs the test suite. Activate them with:

```bash
git config core.hooksPath .githooks
```

## The validation gates

Every change must pass all three gates before it lands — the same checks CI
runs (see [`ci.sh`](ci.sh) and `.github/workflows/ci.yml`):

```bash
cargo fmt --all -- --check                              # no formatting diffs
cargo clippy --workspace --all-targets -- -D warnings   # no warnings
cargo test --workspace                                  # all green
```

`cargo fmt --all` auto-formats. With the git hooks enabled, the first two run
on every commit and the test suite runs on every push.

### Performance probe

A larger-scale probe lives in `crates/note-store/tests/perf.rs` (and runs in the
`perf` workflow). It builds a realistic corpus at runtime from RSS feeds
(`crates/note-store/tests/feeds.json`) — so no big fixture is committed — and
times the write/search/backlinks/reindex paths. It is `#[ignore]`d (and skips if
no feed responds), so it never slows the normal suite:

```bash
# 50k notes, the per-PR probes (report-only):
cargo test -p note-store --release --test perf -- --ignored --nocapture
# full tier (resolve_ref / list / reindex), bigger corpus, asserting ceilings:
NOTE_PERF_N=200000 NOTE_PERF_FULL=1 NOTE_PERF_ENFORCE=1 \
  cargo test -p note-store --release --test perf -- --ignored --nocapture
```

It starts informational; the ceilings become a merge gate once calibrated from
CI-runner numbers.

## How we work

- **Tests come with the change.** New behavior — especially parsers, id
  derivation, FTS query building, and import/export round-trips — ships with
  unit tests. Add the test alongside (ideally before) the implementation.
- **Keep the surface in sync.** When you add, remove, or rename a CLI
  subcommand/flag or a TUI keybinding, update its `--help` text, the README, and
  the regression tests that assert the command/keybinding surface.
- **No dead code, no half-built features.** If a feature isn't finished, it
  doesn't land.
- **Document the *why*, not the *what*.** Comments explain a constraint, an
  incident, or a non-obvious invariant — not the line above them.
- **Respect the layering.** `note-core` is IO-free; front-ends (`note-cli`,
  `note-tui`) never touch SQLite directly — everything routes through
  `note-store`. See the invariants in [`CLAUDE.md`](CLAUDE.md).
- **Migrations are append-only.** Never edit a shipped migration; add a new one
  with a test that migrates an old DB forward.

## Commits and pull requests

- Use [Conventional Commits](https://www.conventionalcommits.org/): e.g.
  `feat(tui): …`, `fix(store): …`, `refactor(cli): …`, `docs: …`,
  `chore(hooks): …`. Split work into logically-grouped commits.
- Keep PRs focused. Describe the change and how you verified it; make sure the
  three gates are green.
- Note user-facing changes in [`CHANGELOG.md`](CHANGELOG.md) under
  `[Unreleased]`.

## Reporting issues

Open a GitHub issue with steps to reproduce, what you expected, what happened,
and your OS / `note --version`. For anything security-sensitive, please contact
the maintainer privately rather than filing a public issue.

## License

By contributing, you agree that your contributions are dual-licensed under
`MIT OR Apache-2.0`, matching the project (see [README](README.md#license)).
