# AGENTS.md

This repository's authoritative instructions for AI coding agents and human
contributors live in **[CLAUDE.md](CLAUDE.md)** — read it before touching code.
It covers the project's purpose, the stack, the crate layout and dependency
direction, the workflow rules, and the cross-cutting invariants that must never
be violated.

`AGENTS.md` exists so agents that look for this filename (Codex, Cursor, Gemini
CLI, …) find the same rules; `CLAUDE.md` is the canonical copy.

Quick reminders (the full rules are in `CLAUDE.md`):

- `note-core` is IO-free; the front-ends (`note-cli`, `note-tui`) never touch
  SQLite directly — everything routes through `note-store`.
- Every change passes `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, and `cargo test --workspace`.
- Conventional commits; keep the CLI/TUI surface in sync with `--help`, the
  README, and the regression tests.
- Migrations are append-only; the FTS row, tags and links commit in the same
  transaction as the note.
