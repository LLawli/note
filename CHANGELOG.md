# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Exact-title and `[[wikilink]]` resolution is deterministic again: a note whose
  title matches exactly is found even when 50+ notes share a common word.
- Importing CRLF (Windows) markdown with frontmatter no longer loses the note's
  id, tags, and timestamps into the body.
- A title containing a newline or a `---` no longer breaks or silently truncates
  an exported note's frontmatter, so `md`/`json` round-trips hold.
- Markdown that merely opens with a `---` horizontal rule imports as body rather
  than erroring.
- `note edit` no longer clobbers a concurrent tag/metadata change made while
  `$EDITOR` was open, and parses an `$EDITOR` whose path contains spaces.
- Showing a note raw on a terminal neutralizes embedded control/escape sequences.
- Export writes via a randomized temp file (no fixed-name symlink/TOCTOU, cleaned
  up on failure); a deleted note's inbound links degrade to dangling instead of
  pointing at a dead id; the store refuses to open a database newer than the
  binary understands.
- Deterministic list/search ordering under same-millisecond ties; the TUI no
  longer opens a hidden row when its bottom border is clicked, surfaces read
  errors in every screen, and can't overflow the scroll offset.
- `install.sh` verifies the download checksum, pins the release tag for the
  `cargo` path, and (like the Homebrew tap) builds from source on Intel macOS.

### Roadmap

- A `crates.io` release so `cargo install note` works.
- An AUR `-bin` package.

## [0.1.0] - 2026-07-01

The initial release: terminal note-taking (CLI + TUI) over a single SQLite file.

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
  - **Follow links** from a panel (`f`): outgoing `[[wikilinks]]` and backlinks,
    with browser-style `esc` history.
  - **Mouse support**: click a list/search row to open it, click a link to
    follow it, and scroll with the wheel.
- **`[[wikilinks]]`**: extraction and a resolved/dangling link graph that is
  rewritten on every write, in the same transaction as the note. Targets resolve
  by unique id prefix too, so a short id like `[[01ARZ…]]` resolves like
  `note show`. `note links` opens an `fzf` picker to follow a link, and
  `note reindex` re-resolves the whole graph (refreshing backlinks for links
  written before their target existed).
- **Tags**: per-note tags with listing, counts, and tag-filtered listing.
- **Import / export**: idempotent `.md` / `.json` import (upsert on note id)
  and atomic `.md` / `.json` export with export↔import round-trips.
- **Storage**: a single SQLite file with a single-writer actor, a read pool,
  WAL mode, FTS5, and `refinery` migrations.
- **Project**: dual `MIT OR Apache-2.0` license, README, contributing guide,
  code of conduct, security policy, issue/PR templates, and versioned git hooks
  (`pre-commit`: fmt + clippy; `pre-push`: tests).
- **CI/CD**: a check workflow (format, lint, tests, audit), a performance gate
  over a large RSS-seeded corpus, and a tagged-release pipeline producing
  multi-platform prebuilt binaries (Linux x86_64/aarch64, macOS Apple Silicon,
  Windows x86_64), `.deb`/`.rpm` packages, a Homebrew tap formula, and a GitHub
  Release with checksums and the CHANGELOG section.
