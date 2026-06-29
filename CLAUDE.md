# CLAUDE.md — `note` project directives

> Read this every session before touching code. This file is the operating
> rules: what the project is, the stack, the layout, the workflow, and the
> invariants that must never be violated.

## What this project is

`note` is a self-contained Rust binary for taking notes entirely from the
terminal — a **CLI + TUI** whose explicit goal is to **remove the need for a
graphical note-taking app**. Capture, search, link, and organize notes
without leaving the shell.

- **Source of truth:** a single **SQLite** database. Chosen primarily for
  **FTS** (full-text search) over notes.
- **Note content:** plain text and/or markdown — the user decides per note.
- **Interchange:** import existing `.md` files; export notes to `json`
  and/or `md`.
- **Core capabilities:**
  - Fast full-text search (SQLite FTS).
  - Links between notes (`[[wikilinks]]`, Zettelkasten-style knowledge graph).
  - Tags / organization.
  - Fast capture-and-edit from the terminal — speed is a feature.
- **Two front-ends, one core:** a scriptable **CLI** for quick one-shot
  commands, and an interactive **TUI** for browsing/editing. Both drive the
  same domain logic; neither owns business rules.

## UX principles (the CLI is first-class)

- **The CLI is fully usable without the TUI.** Every core operation — create,
  read, search, list, tag, link, import, export — is reachable as a CLI
  subcommand. The TUI is a convenience layer over the same operations, never a
  prerequisite: no data operation requires entering it. `note-cli` depends on
  `note-tui` only to launch it when run bare.
- **Reading a note renders its markdown.** `note show <ref>` renders markdown in
  the terminal via `termimad`, not as a raw dump.
- **Notes are referenced by a friendly handle, never a raw ULID.** The ULID is
  the canonical internal identity (storage, wikilinks, export); users never type
  it. A single `NoteRef` resolver — shared by `show` / `edit` / `tag` / `link`
  and by `[[wikilink]]` title resolution — accepts, in priority order: a full
  ULID, a short ULID prefix (git-style), a case-insensitive title, then an FTS
  query fallback.
- **Ambiguous references open a picker.** When a reference matches multiple
  notes on a TTY and `fzf` is available, launch
  `fzf --preview 'note show {ref} --raw'` (candidate list + first-lines
  preview); selecting prints the full note. When `fzf` is absent or output is
  piped, print a numbered candidate list and exit non-zero.
- **`note show` with no argument opens the most recently updated note.**
- **TTY-aware output.** Rendering auto-detects the output stream
  (`std::io::IsTerminal`): styled markdown when stdout is a terminal, raw
  text/markdown when piped or redirected (so `note show x > out.md` and
  `note show x | …` stay clean). `--raw` forces raw; `--render` forces styled.
- **No built-in pager.** Output is printed straight; users pipe to `less`
  themselves. (Revisit only if it proves painful.)
- **Scriptable by default.** Machine-readable output via `--json` where it
  helps; human-readable text otherwise. Errors go to stderr; exit codes are
  meaningful.

## Stack (do not deviate without updating this file)

- **Runtime:** Rust (edition 2024, resolver 3), pinned in
  `rust-toolchain.toml`. The whole happy path is **synchronous**: the
  single-writer SQLite actor is a dedicated OS thread driven by a
  `std::sync::mpsc` channel (no `tokio` in v1). Async is only revisited if a
  later milestone (e.g. the TUI) genuinely needs it.
- **Store:** `rusqlite` + `refinery` migrations. **One file**, one writer
  actor, one read pool. WAL mode. **FTS5** for full-text search — this is the
  reason SQLite was chosen.
- **TUI:** `ratatui` (0.30.x) with `ratatui-bubbletea` + `ratatui-tea`
  (Elm-style `Model` / `Msg` / `Cmd`, `update()` / `view()`, `Program`
  runner). Theme tokens via `ratatui-bubbletea-theme`. Markdown is rendered
  inside the frame via `tui-markdown` (no `highlight-code` feature, so no
  syntect/onig C dependency); `termimad` cannot draw into a ratatui frame.
- **CLI:** `clap` (derive) for argument parsing. Output is plain text by
  default, `--json` where a machine-readable form is useful.
- **Markdown / wikilinks:** `pulldown-cmark` for parsing and for extracting
  `[[wikilinks]]` to build the link graph.
- **CLI markdown rendering:** `termimad` renders markdown to the terminal for
  CLI read commands. It is **independent of `ratatui`** — the CLI never depends
  on the TUI crate. TTY detection uses `std::io::IsTerminal` (no extra crate).
- **Interactive picker (optional):** for ambiguous note references the CLI
  shells out to `fzf` (if found on `$PATH`) with `--preview`. `fzf` is an
  OPTIONAL runtime integration, never a hard dependency; absence degrades to a
  numbered list. The CLI never embeds `ratatui` for this.
- **Serialization:** `serde` + `serde_json` for `json` import/export.
- **Config:** `figment`, read once at startup, passed by `&Arc<Config>`.
- **Errors:** `thiserror` in library crates, `anyhow` at the binary edge.
- **Logging:** `tracing` with module filters; the TUI must route logs to a
  file, never to the terminal it is drawing on.

## Repository layout

```
crates/
  note-core/   # domain types (Note, Tag, Link, typed ids), errors. NO IO.
  note-store/  # SQLite, single-writer actor, FTS5, refinery migrations, read pool.
  note-md/     # markdown parse, [[wikilink]] extraction, import (.md) / export (md + json).
  note-tui/    # ratatui-tea app: Model / Msg / Cmd, screens, widgets, theme.
  note-cli/    # `note` binary: clap subcommands; launches the TUI when run bare.
tests/         # workspace integration tests.
docs/          # design notes (DO NOT delete).
migrations/    # SQL migration files consumed by refinery (or embedded in note-store).
```

Dependency direction (never inverted):

```
note-cli ─┬─> note-tui ──> note-store ──┐
          ├─> note-store ───────────────┼─> note-core
          └─> note-md ──────────────────┘
```

- `note-core` depends on nothing in the workspace and does no IO.
- `note-store` depends only on `note-core`.
- `note-md` is **pure**: it depends only on `note-core` (parse, `[[wikilink]]`
  extraction, `Note` <-> md/json conversion). It never touches SQLite.
- `note-tui` and `note-cli` never touch SQLite directly — they go through
  `note-store`. Import/export wiring (read/write via `note-store`, convert via
  `note-md`) lives in `note-cli`.
- Business rules live in `note-core` / `note-store`, never in a front-end.

## Workflow rules

1. **Milestone by milestone.** Do not start M(n+1) until every "Done when"
   bullet in M(n) passes. No mixing milestones in one change.
2. **No dead code, no half-built features.** If a feature is not finished, it
   does not land. If you must stub something, document it as `M(n) TODO` in the
   module's doc-comment with the milestone number.
3. **Tests before claiming done.** Every milestone requires:
   - `cargo fmt --all -- --check` (no diffs)
   - `cargo clippy --workspace --all-targets -- -D warnings` (no warnings)
   - `cargo test --workspace` (all green)
   - Manual exercise of the new feature against the real `note` binary
     (CLI subcommand and/or TUI screen) when applicable.
4. **Document the why in code, not the what.** No comments restating the line
   above; only comments explaining a constraint, an incident, or a non-obvious
   invariant.
5. **Add a unit test before the implementation, not after.** Especially for
   parsers (`[[wikilink]]` extraction, markdown), ID derivation, FTS query
   building, and import/export round-trips.
6. **CLI/TUI surface changes must stay in sync.** When adding, removing, or
   renaming a CLI subcommand/flag or a TUI keybinding/action, update the
   `--help` text, the README/docs, and the regression tests that assert the
   command/keybinding surface.
7. **Don't refactor outside the milestone.** Touch only what the current
   milestone requires; resist scope creep.
8. **Migrations are append-only.** Never edit a shipped migration; add a new
   one. Schema changes always ship with a migration and a test that opens an
   old DB and migrates it forward.

## Cross-cutting invariants (carved in, never violated)

Treat any change that violates one of these as a blocking issue:

1. **One config-read path.** No `std::env::var` outside `Config::load()`.
   Config is read once at startup and passed by `&Arc<Config>`.
2. **Single-writer SQLite actor.** All writes go through one `mpsc` channel to
   one writer task. Reads use a separate read pool. No write happens off that
   actor.
3. **Indexes commit in the same transaction as the data.** When a note is
   written, its FTS row and its link/tag rows commit in the SAME transaction.
   No "index it later in a background task" — a crash must never leave FTS or
   the link graph out of sync with `notes`.
4. **`note-core` is IO-free and front-end-free.** No SQLite, no filesystem, no
   `ratatui`, no `clap` in `note-core`. Domain types and rules only.
5. **Front-ends never own business rules.** `note-cli` and `note-tui` parse
   input and render output; every mutation/query routes through `note-store`.
6. **The TUI never writes to its own terminal stream for logs.** `tracing`
   output goes to a file (or stderr only when the TUI is not active). The
   alternate screen is restored on every exit path, including panic.
7. **Atomic, recoverable imports.** Importing a batch of `.md` files happens in
   a transaction (or a clearly-resumable batch); a failure mid-import never
   leaves half-imported, FTS-desynced state. Import is idempotent on a stable
   note identity.
8. **Export round-trips.** `note` exported to `md`/`json` and re-imported must
   reproduce the same logical note (content, tags, links). A round-trip test
   guards this.
9. **Typed identity everywhere.** A note is referenced by a typed id
   (e.g. `NoteId`), never a bare integer/string passed around untyped across
   layers. Wikilinks resolve through that identity, not raw titles at the
   storage layer.
10. **Atomic file writes for export.** Write to a temp file, then rename; never
    truncate-in-place a user's export target.
11. **Default data dir is an absolute, canonical platform path** (XDG /
    platform dirs). Logged loudly on startup. Overridable via config/env read
    only through `Config::load()`.
12. **No global singletons / `lazy_static` configs.** State is constructed at
    startup and passed down explicitly.
13. **`{schema_version}` is known and checked.** The store refuses to open a
    DB newer than the binary understands, and migrates forward one known DB.

## Quick commands

```bash
# Build everything.
cargo build --workspace

# Lint + format + test (run before every commit).
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Auto-format.
cargo fmt --all

# Exercise the binary.
./target/debug/note --version
./target/debug/note                       # launch the TUI
./target/debug/note new "title" -m "body" # quick capture from the CLI
./target/debug/note edit <ref>            # edit a note in $EDITOR
./target/debug/note delete <ref>          # delete a note (alias: rm; confirms)
./target/debug/note search "query"        # FTS search
./target/debug/note tags                  # list tags with counts
./target/debug/note list --tag work       # filter by tag
./target/debug/note import ./path/*.md       # import markdown / json
./target/debug/note export ./out --format md # export notes (md or json)
NOTE_DATA_DIR=/tmp/x ./target/debug/note status
```

> Subcommand names above are the intended surface; keep this block in sync with
> the actual `clap` definitions as they land (workflow rule 6).

## What this project is NOT (v1 non-goals)

- **No GUI.** The entire point is to avoid a graphical app; CLI + TUI only.
- **No sync server / multi-device sync.** The SQLite file is the boundary;
  back it up or sync the file yourself if you want.
- **No multi-user / auth / sharing.** Single-user, local-first.
- **No web UI, no HTTP server.**
- **No alternative storage backends** (no Postgres, no remote DB). SQLite only.
- **No rich-media notes** (images/attachments) in v1 — text and markdown only.
- **No plugin system** in v1.

## Milestones

Build strictly in order (workflow rule 1). A milestone is done only when every
"Done when" bullet passes, including the three CI gates (fmt, clippy, test).

### M0 — Workspace skeleton + `note-core`
- Cargo workspace with all five crates created and building (four are clean
  compiling stubs; only `note-core` carries real code).
- `note-core`: `Note`, `Tag`, typed ids (`NoteId` = ULID), `Timestamp`,
  `ContentKind`, the wikilink target/value types (`WikiTarget`, `WikiLink`) with
  `FromStr`/`Display`, `derive_title`, and the `thiserror` error taxonomy. No IO,
  no ambient clock/RNG (ids and timestamps are injected at the edge).
- The link-index struct (`Link`, with resolution state) is **M1**, not M0 — it
  only makes sense once storage exists.
- `rust-toolchain.toml` pinned; shared `[workspace.lints]`; CI gates run clean.
- **Done when:** `cargo build/clippy/test --workspace` are green; `note-core`
  has unit tests for the four mandated areas — `NoteId` parse↔display
  round-trip, tag normalization, title derivation (H1 / first-line / empty), and
  wikilink-target parsing (id / title / alias) — plus a durable source-level
  guard proving `note-core` never touches an ambient clock/RNG/IO.
- Full step-by-step plan: [`docs/m0-plan.md`](docs/m0-plan.md).

### M1 — `note-store`: schema + single-writer + FTS
- refinery migrations create the `notes`, `tags`, `links`, and FTS5 tables.
- Single-writer actor (one `mpsc`), separate read pool, WAL on.
- The link-index struct (`Link` with resolution state) lands here, alongside
  the storage shape that gives it meaning.
- CRUD for notes; FTS/tags/links rows commit in the same transaction.
- **Done when:** integration tests prove create/read/update/delete, an FTS
  query returns expected hits, and a crash mid-write leaves no desync.

### M2 — `note-cli`: core commands
- `clap` subcommands: `new`, `edit`, `delete` (alias `rm`), `search`, `list`,
  `show`, `status`.
- `note search` is **prefix-aware**: each whitespace term is matched as an FTS5
  prefix (`"term"*`), so `mensag` finds `mensagem`. (`ReaderPool::search` stays
  raw for internal exact-ish resolution; `search_prefix` builds the prefix query.)
- `note edit <ref>` resolves via the same `NoteRef` resolver, then edits the body
  in `$EDITOR` (pre-filled with the current body) or non-interactively via
  `--message` / `--title` / `--tag` / `--plain` / `--markdown`; a metadata-only
  edit never touches the body. No reference = the most recent note.
- `note delete <ref>` (alias `rm`) resolves via `NoteRef` and confirms before
  deleting: an interactive `[y/N]` prompt on a terminal, or `--yes`/`--force`;
  it refuses (non-zero) rather than delete when stdin is not a terminal and
  `--yes` was not given.
- `NoteRef` resolver: resolves a user reference to a `NoteId` in priority order
  — full ULID, short ULID prefix (git-style), case-insensitive title, FTS query
  fallback. (Pure classification of the reference string may live in
  `note-core`; resolution to a `NoteId` is a `note-store` lookup.)
- `note show <ref>` resolves via `NoteRef`; with no argument it opens the most
  recently updated note. Renders markdown via `termimad`, TTY-aware (styled on a
  terminal, raw when piped/redirected); `--raw` / `--render` override.
- Ambiguous references on a TTY open an optional `fzf` picker
  (`--preview 'note show {ref} --raw'`, selection prints the full note); when
  `fzf` is absent or output is piped, print a numbered list and exit non-zero.
  `fzf` is optional and the CLI never embeds `ratatui`.
- No subcommand requires the TUI (the CLI is fully usable standalone); the
  `note-tui` dependency exists only so a bare `note` can launch it (M6).
- Plain-text output; `--json` where useful.
- **Done when:** each subcommand is exercised against a real DB and covered by
  a CLI-surface regression test, including (a) a piped-vs-TTY render check for
  `show`, (b) `NoteRef` resolution tests for each reference kind, and (c) the
  ambiguity fallback (numbered list, non-zero exit) when not on a TTY.

### M3 — `note-md`: parse + wikilink graph
- `note-md::extract_wikilinks` parses markdown (pulldown-cmark) and extracts
  `[[wikilinks]]` (id / title / `Title|alias` forms), ignoring code blocks and
  inline code, de-duplicated in document order.
- `note-store` resolves `ByTitle` targets to a `NoteId` by unique
  case-insensitive effective-title match (ambiguous/absent stays dangling).
- `note-cli` re-extracts and replaces a note's link graph on every `new`/`edit`
  (in the write transaction); `note links <ref>` lists the outgoing links.
- **Done when:** unit tests cover wikilink extraction edge cases and the graph
  reflects added/removed links (CLI e2e).

### M4 — Import / export
- `note-md` converts `Note` <-> markdown (frontmatter: id/title/tags/content_kind/
  created/updated + body) and `Note` <-> json.
- `note import <files>…` is idempotent on the note id (upsert preserving
  timestamps), per-file atomic (the store transaction), resumable across a batch,
  and re-extracts each note's links; `note export <dir> --format md|json` writes
  one file per note via atomic temp-then-rename.
- **Done when:** a round-trip test (export → import) reproduces the same
  logical notes (content, tags, links), CLI e2e.

### M5 — Tags / organization
- `note tags` lists every tag with its note count; `note list --tag <t>` filters
  by tag; `note tag <ref> [--add <t>]… [--remove <t>]…` shows or edits a note's
  tags (no flags = read-only view).
- **Done when:** tag operations are covered by tests and exposed in `--help`
  (CLI e2e).

### M6 — `note-tui` (ratatui-tea)
- Elm-style app (`Model`/`Msg`/`Cmd`) over `note-store`: browse the note list,
  open a note (markdown rendered via `tui-markdown`, plain shown verbatim,
  scrollable), live FTS search, and request an edit. `update()` is driven by
  semantic `Msg`s (not raw keys) so it is unit-testable; the event loop +
  `App::map_key` are the only crossterm-aware parts.
- Terminal restored on every exit path including panic (`ratatui::init`'s panic
  hook); the TUI never logs to the terminal it draws on.
- A bare `note` launches it; pressing `e` returns an edit request that the CLI
  fulfils with `$EDITOR`, then re-enters the TUI.
- **Done when:** core `update()` transitions have unit tests, a frame renders via
  `TestBackend`, and the binary launches (bare `note`).

<!-- ai-memory:start -->
## Long-term memory (ai-memory)

This project uses [ai-memory](https://github.com/akitaonrails/ai-memory)
for cross-session continuity.

**Default to the current project — always.** Every ai-memory tool
auto-scopes to the project resolved from your session's working
directory. **Do NOT pass `project`, `workspace`, or `cwd` arguments unless the user
explicitly references a *different* project by name** (e.g. "what did we
decide in the `other-app` project?"). Phrases like "this project",
"here", "we", "our work", "where did we leave off" all mean the *current*
project — call the tool with no scoping args. If the user asks about a
handoff and the SessionStart auto-fetched block is already in your
context, just answer from it; do not re-call the tool to "find it again"
in another project.

**Lifecycle hooks already capture every prompt + tool call
automatically.** You never need to manually write routine notes; the
SessionStart hook auto-fetches pending handoffs, and on session end
ai-memory writes a session-summary page and a handoff.
LLM consolidation (compiling observations into topical wiki pages) runs
on PreCompact, on demand via `memory_consolidate`, and at session end
only when the server sets `AI_MEMORY_CONSOLIDATE_ON_SESSION_END`. Only
write a durable wiki page when the user explicitly asks to remember or
annotate something permanently.

### When to reach for each tool

The user can express any of the intents below in plain English —
match the intent to the tool. They do not need to name the tool.

| User says / situation | Tool |
|---|---|
| "have we discussed X?" / "search memory for Y" / before proposing architecture | `memory_query` (current project; `scopes` for named siblings; `global=true` to search every project) |
| "what's been going on" / "show recent activity" (light) | `memory_recent` |
| "is ai-memory healthy?" / "how big is the wiki?" | `memory_status` |
| "give me the stats" / structured snapshot for the agent to consume | `memory_briefing` (read-only; never creates handoffs) |
| "catch me up" / "I've been away" / "what's important right now?" / open-ended exploration | `memory_explore` |
| "where did we leave off?" — and you see a `📥 ai-memory: pending handoff` block in your context | already done — answer from that block; do NOT re-call `memory_handoff_accept` |
| "where did we leave off?" — and no such block is visible | `memory_handoff_accept` (rare; the SessionStart hook usually got there first; pass `workspace` + `project` together only for a named sibling workspace/project) |
| "save context for the next session" / wrapping up / ending this session | `memory_handoff_begin` (session-end only; do **not** use for status/briefing; single-use handoff; terse summary; put detail in `open_questions` + `next_steps` bullets; pass `workspace` + `project` together only for a named sibling workspace/project) |
| "discard that handoff" / "I created a handoff by mistake" | `memory_handoff_cancel` (requires exact `handoff_id` from `memory_handoff_begin`; marks it expired before the next session sees it) |
| "consolidate this session" / "compile what we learned" (also runs on PreCompact; at session end only if `AI_MEMORY_CONSOLIDATE_ON_SESSION_END` is set) | `memory_consolidate` |
| "what did we learn from this session?" / "what memory should we add?" / explicit wrap-up learning review | `memory_auto_improve` (manual learning review for a completed session; omit `session_id` for latest completed session; the server also schedules background review for newly completed sessions in every project when configured) |
| "remember this permanently" / "save a note" / "add an annotation" / durable project knowledge | `memory_write_page` (write a wiki page; do **not** use handoff for permanent notes; put the title as a `# H1` on the first line of `body` and omit the `title` arg — ai-memory derives it from the H1) |
| "read the page about X" / "show me the full content of Y" / "open the page on Z" | `memory_read_page` (full body; pass a query to search or `path` for a direct lookup; pass `workspace` + `project` together only for a named sibling workspace/project) |
| "delete the page X" / "remove that note" | `memory_delete_page` (by exact `path`; idempotent; pass `workspace` + `project` together only for a named sibling workspace/project) |
| "audit the wiki" / "find contradictions" / "what rules should we add?" | `memory_lint` |
| "prune old pages" / "memory cleanup" | `memory_forget_sweep` |

`memory_explore` is the right default for the "I want to know what's
going on" use case — it returns a prose digest whose verbosity
scales automatically to how long it's been since the last activity
(< 1 h → one line; > 30 days → full catchup).

### When the current project comes up empty — broaden the search

`memory_query` searches only the **current** project by default. If a
search comes back empty or thin, the knowledge may live in a **sibling
project** — shared `infra`, `ops`, or a related app. Don't conclude
"we never recorded it" after a single project misses; broaden instead:

- **Know which projects to check?** Re-run with explicit `scopes`, e.g.
  `scopes: [{ "workspace": "default", "project": "infra" }]`.
- **Don't know where it lives?** Pass `global=true` to search every
  project in every workspace at once. Each hit is annotated with its
  workspace + project so you can tell where it came from. `global=true`
  cannot be combined with `scopes`/`project`/`workspace`.

`memory_query` returns **snippets, not full page bodies** — an empty or
short snippet does **not** mean the page is empty (a large page can
match outside the snippet window). To read the whole page, use
`memory_read_page` (by `path`, or pass a `query` to fetch the top hit's
full body; add `workspace` + `project` together only when the user names
a sibling workspace/project).

### Use Retrieved Memory As Operating Guidance

When `memory_query` or `memory_recent` returns `_rules/`, `gotchas/`,
`procedures/`, or `decisions/` pages that match the current task, treat
them as actionable context, not trivia:

- Read full pages with `memory_read_page` when the snippet looks relevant.
- Apply `_rules/` as constraints.
- Check `gotchas/` as preflight warnings before editing the same subsystem.
- Follow `procedures/` as checklists for releases, PR reviews, deploys,
  migrations, and other repeatable workflows.
- Use `decisions/` as prior architecture unless the user explicitly asks
  to revisit them.

Before non-trivial coding, debugging, deployment, release, auth, scope,
migration, PR-review, or data-preservation work, search memory for the
subsystem and task type first. If the first query is thin, broaden or
query specific error/subsystem terms before designing a fix.

### Learning Review

The server schedules background auto-improvement for newly completed sessions in
every project when an LLM provider is configured. `memory_auto_improve` is the manual version:
use it when the user asks what durable lessons this session suggests, or at
explicit wrap-up when reviewing proposed memory would be useful. Scheduled and
manual runs apply or stage validated edits through the auto-improvement approval
path. Admins can turn off scheduling with `[auto_improve.scheduler] enabled =
false`, or opt into manual proposal approval with `[auto_improve]
require_approval = true`, in which case scheduled and manual proposals stay in
pending-writes until approved.

### When you write a project rule, write it here

If you're about to write a durable project rule ("always X", "never
Y", "all PRs must …"), write it in the project's canonical agent
instruction file. Many projects use CLAUDE.md for Claude Code and
AGENTS.md for Codex / OpenCode / Cursor / Gemini CLI, but if the
project says one file is canonical, use that file. ai-memory's lint
pass surfaces the same hint automatically when a `kind: rule` page
lands in `_rules/`.

### Refreshing this snippet

This block is maintained by ai-memory. Two ways to refresh it with
the latest binary's recommended copy:

- **From the agent** (no terminal needed): ask "refresh the ai-memory
  routing in this project" — the agent calls
  `memory_install_self_routing`, picks the right filename for itself
  (Claude Code → `CLAUDE.md`; Codex / OpenCode / Cursor / Gemini →
  `AGENTS.md`), and uses its Write / Edit tool to land the block.
- **From the CLI**: `ai-memory install-instructions` (defaults to
  `CLAUDE.md`; pass `--target AGENTS.md` for non-Claude agents or
  projects that use `AGENTS.md` as the canonical instruction file).

Both are idempotent: re-runs replace the block bracketed by
`<!-- ai-memory:start -->` / `<!-- ai-memory:end -->` markers
without disturbing the rest of the file.
<!-- ai-memory:end -->
