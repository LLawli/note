# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

The first development cycle toward `0.1.0`. The CLI and TUI are usable; the
items below describe what is implemented today.

### Added

- **CLI** (`note`): `new`, `edit`, `delete` (alias `rm`), `show`, `search`,
  `list`, `tags`, `tag`, `links`, `import`, `export`, and `status`.
  - Prefix-aware full-text search (`mensag` finds `mensagem`).
  - Friendly note references: full id, short id prefix, case-insensitive title,
    or a full-text query; ambiguous references open an `fzf` picker or print a
    numbered list.
  - TTY-aware rendering (styled markdown on a terminal, raw when piped) with
    `--raw` / `--render` overrides, and `--json` where useful.
- **TUI** (`ratatui`, Elm-style): browse the note list, live full-text search,
  read a note with markdown rendered in-frame, **create** a note from a title
  prompt (`n`), and **edit** a note (`e`) via `$EDITOR`.
  - In-frame markdown rendering strips `#` heading markers, differentiates
    heading levels, and draws fenced code blocks as a labelled box.
- **`[[wikilinks]]`**: extraction and a resolved/dangling link graph that is
  rewritten on every write, in the same transaction as the note.
- **Tags**: per-note tags with listing, counts, and tag-filtered listing.
- **Import / export**: idempotent `.md` / `.json` import (upsert on note id)
  and atomic `.md` / `.json` export with export↔import round-trips.
- **Storage**: a single SQLite file with a single-writer actor, a read pool,
  WAL mode, FTS5, and `refinery` migrations.
- **Project**: dual `MIT OR Apache-2.0` license, README, contributing guide,
  versioned git hooks (`pre-commit`: fmt + clippy; `pre-push`: tests), and a CI
  workflow running format, lint, and tests.

### Roadmap

- Prebuilt binaries and OS packages (tarballs, Homebrew, AUR, RPM/DEB).
- A `crates.io` release so `cargo install note` works.
- A tagged-release pipeline (checksums, GitHub Releases).

[Unreleased]: https://github.com/LLawli/note/commits/master
