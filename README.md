# note

**Take notes without leaving the terminal — so you never have to open a GUI note app again.**

[![CI](https://github.com/LLawli/note/actions/workflows/ci.yml/badge.svg)](https://github.com/LLawli/note/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Capture a thought, search everything you've written, link notes together, and
tag them — all from the shell, in the time it would take a graphical app to
launch. `note` is a single self-contained binary: a scriptable **CLI** for
quick one-shot commands and an interactive **TUI** for browsing and editing,
both driving the same store.

Your notes live in **one SQLite file** you own and can back up or sync yourself.
There is no account, no server, no sync service, and no GUI.

---

## Why

Graphical note apps make you stop, switch windows, wait for a launch, and click
around — every time you have a thought worth keeping. If you already live in a
terminal, that context switch is friction with no payoff.

`note` removes it. Capturing a note is one command. Finding one is full-text
search that's instant even over thousands of notes. The result is a knowledge
base that keeps up with the speed of thinking, stays entirely on your machine,
and is trivial to script.

## Features

- **Fast full-text search** over every note, powered by SQLite **FTS5**
  (prefix-aware: `mensag` finds `mensagem`).
- **`[[wikilinks]]`** between notes — a Zettelkasten-style knowledge graph,
  with title/id/alias forms and dangling-link tracking.
- **Tags** for organizing and filtering.
- **Markdown or plain text**, your choice per note. Reading a note renders the
  markdown in the terminal.
- **Import / export**: bring in existing `.md` files; export to `.md` or
  `.json` with lossless round-trips.
- **A real TUI** for browsing, searching, reading, creating, and editing — and
  a **CLI** that does all of it without ever entering the TUI.
- **Local-first**: one SQLite file, single user, no network.

## Install

`note` is a single self-contained binary. Prebuilt binaries and packages are
published per [release](https://github.com/LLawli/note/releases).

### One-line install / update (picks the best method)

Runs anywhere and installs via the most appropriate route it finds — Homebrew,
your distro's `.deb`/`.rpm`, `mise`, `cargo`, or a prebuilt binary, in that
order. **Re-run it to update** — every route installs-or-upgrades in place:

```bash
curl -fsSL https://raw.githubusercontent.com/LLawli/note/master/install.sh | sh
```

Prefer a specific method? Pick one below.

### Prebuilt binary

Download the tarball/zip for your platform from the
[releases page](https://github.com/LLawli/note/releases/latest) — Linux
(`x86_64`, `aarch64`), macOS (Apple Silicon), Windows (`x86_64`) — verify it
against the published `.sha256`, and put `note` on your `$PATH`:

```bash
tar -xzf note-*-x86_64-unknown-linux-gnu.tar.gz
sudo install note-*/note /usr/local/bin/
```

### Homebrew (macOS / Linux)

```bash
brew install LLawli/tap/note
```

### Debian / Ubuntu · Fedora / RHEL

Grab the `.deb` or `.rpm` from the release and install it:

```bash
sudo dpkg -i note_*_amd64.deb     # Debian / Ubuntu
sudo rpm -U  note-*.x86_64.rpm    # Fedora / RHEL
```

### mise

[mise](https://mise.jdx.dev) fetches the prebuilt binary straight from the
release (via its `ubi` backend) and keeps it up to date:

```bash
mise use -g ubi:LLawli/note
```

### From source (Rust toolchain)

```bash
cargo install --git https://github.com/LLawli/note note-cli
# …or: git clone https://github.com/LLawli/note && cd note && cargo build --release
#       (binary at ./target/release/note)
```

The pinned toolchain (`rust-toolchain.toml`) is installed automatically by
`rustup` when you build.

## Quick start

```bash
# Capture a note (opens $EDITOR, or pass the body inline):
note new "Grocery list" -m "milk, eggs, coffee"
echo "piped body" | note new "From a pipe"

# Find it again (full-text, prefix-aware):
note search "coffee"

# List the most recently updated notes:
note list

# Read one (renders markdown; reference by title, short id, or a search):
note show "Grocery list"

# Edit it in $EDITOR:
note edit "Grocery list"

# Tags:
note tag "Grocery list" --add shopping
note list --tag shopping
note tags

# Links between notes (Zettelkasten):
note links "Grocery list"

# Re-resolve every note's links (e.g. after an upgrade or bulk import):
note reindex

# Import / export:
note import ./vault/*.md
note export ./backup --format md

# Where everything lives:
note status
```

Notes are referenced by a friendly handle, never a raw id: a full id, a short
id prefix (git-style), a case-insensitive title, or a full-text query — in that
order. Ambiguous references open an `fzf` picker when available, or print a
numbered list.

Output is TTY-aware: styled markdown on a terminal, raw text when piped or
redirected (so `note show x > out.md` stays clean). `--json` is available where
a machine-readable form helps.

## The TUI

Run `note` with no arguments to launch the interactive browser:

```bash
note
```

| Screen | Keys |
|---|---|
| **List**   | `↑`/`↓` move · `enter` open · `/` search · `n` new · `e` edit · `q` quit |
| **View**   | `↑`/`↓` scroll · `f` links · `e` edit · `esc` back · `q` quit |
| **Links**  | `↑`/`↓` move · `enter` follow · `esc` back · `q` quit |
| **Search** | type to filter · `enter` apply · `esc` cancel |
| **New**    | type the title · `enter` open `$EDITOR` · `esc` cancel |

Creating (`n`) and editing (`e`) hand off to `$EDITOR`, then drop you back in
the browser with your changes saved.

**Following links.** Press `f` while reading a note to open its links panel —
its outgoing `[[wikilinks]]` and its backlinks (notes that point to it).
Selecting one opens the target; `esc` walks back through where you came from
(browser-style history). Backlinks come from the resolved link graph; if you
linked a note before its target existed, run `note reindex` once to refresh it.

**Mouse.** The TUI also takes mouse input: click a note in the list or search
results to open it, click a link in the panel to follow it, and use the scroll
wheel to scroll a note.

## Configuration

| What | How |
|---|---|
| Data directory | Defaults to an absolute platform path (XDG / OS dirs), logged on startup. Override with `--data-dir <DIR>` or `NOTE_DATA_DIR`. |
| Editor | `$VISUAL` then `$EDITOR`, used by `new` / `edit` and the TUI. |
| Picker | Optional: `fzf` on `$PATH` is used for ambiguous references. |

All configuration is read once at startup; there are no hidden global settings.

## How it works

`note` is a Rust workspace of five crates with a strict dependency direction
(front-ends never touch SQLite directly):

```
note-cli ─┬─> note-tui ──> note-store ──┐
          ├─> note-store ───────────────┼─> note-core
          └─> note-md ──────────────────┘
```

- **`note-core`** — IO-free domain types, typed ids (ULID), errors.
- **`note-store`** — SQLite with a single-writer actor, a read pool, WAL, and
  FTS5; a note's text, FTS row, tags, and links all commit in one transaction.
- **`note-md`** — pure markdown parsing, `[[wikilink]]` extraction, and
  `md`/`json` conversion.
- **`note-tui`** — a `ratatui` Elm-style (Model/Msg/Cmd) interface.
- **`note-cli`** — the `note` binary (clap), which launches the TUI when run
  bare.

Design notes live in [`docs/`](docs/), and the full engineering rules — the
stack, invariants, and milestone plan — are in [`CLAUDE.md`](CLAUDE.md).

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md)
for the build/test gates, commit conventions, and the milestone-based workflow.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in this project by you, as defined in the
Apache-2.0 license, shall be dual-licensed as above, without any additional
terms or conditions.
