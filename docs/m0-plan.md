# M0 Implementation Plan — `note` workspace (FINAL, execution-ready)

## Overview
M0 delivers a five-crate Cargo workspace (edition 2024, resolver 3, toolchain pinned to 1.95.0) where `note-core` is the only crate with real code: IO-free domain types (`NoteId`, `Tag`, `WikiTarget`, `WikiLink`, `Note`, `Timestamp`, `ContentKind`), a `thiserror` error taxonomy, the `derive_title` function, and `FromStr`/`Display` for the wikilink target. The other four crates are clean compiling stubs. The milestone is done when `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test` all pass workspace-wide, with unit tests covering the four mandated areas (NoteId round-trip, tag normalization, title derivation, wikilink-target parsing) plus a durable source-level guard proving core never touches an ambient clock/RNG/IO.

---

## Critic findings: adjudication summary
All findings were re-verified empirically against ulid 1.2.1 on the installed 1.95.0 toolchain. Accepted unless noted.

ACCEPTED (with the fix baked into the plan below):
- **title.rs fence blocker (#61) [blocker]** — confirmed `first_non_empty_line` returns "```". Fix: drop the fence state machine and closing-hash trimming entirely; M0 H1 detection is a deliberately trivial ATX scan (`# ` = one hash + whitespace). Real fence/setext/ATX-edge handling is M3 (pulldown-cmark). Tests #58 (closing-hash) and #61 (fenced) removed.
- **IO-free not structural [major]** — confirmed: `cargo build --workspace` with an edge crate that enables ulid default features unifies `std`+`rand` into core's ulid build, after which core *could* call `Ulid::new()`. Fix: keep `default-features=false` in core (documents intent + blocks ambient ctors while M0 is the sole consumer) but stop calling it "structural"; enforce no-ambient-minting with a **durable source-level guard test** (`tests/io_free_guard.rs`).
- **`cargo tree -p note-core` is a false-assurance probe [major]** — confirmed it stays empty even when unification links rand. Fix: the done-when uses `cargo tree -p note-core --edges normal` (verified clean) as an *M0-only* sanity check, and the cross-milestone guarantee is the source guard test, not the tree command.
- **clippy::pedantic + `--all-targets -D warnings` denies pedantic in tests [major]** — confirmed `unreadable_literal` fails the build. Fix: add `unreadable_literal`, `similar_names`, `items_after_statements`, `cast_possible_truncation` to the workspace clippy allow-list, plus underscore-group numeric literals in tests.
- **ulid overflow round-trip hole [minor]** — confirmed `"80000000000000000000000000"` parses OK but Display emits `"00000000000000000000000000"`. Fix: `NoteId::from_str` re-checks canonical form (`id.to_string() == s.to_ascii_uppercase()`) and returns new `IdError::NonCanonical`. Verified this still accepts lowercase canonical input (ulid normalizes to uppercase).
- **decode-error ordering (#8) [minor]** — confirmed length is checked before chars; `"I"` → `InvalidLength`. Fix: #8 uses a 26-char fixture ending in `I` (`"0123456789ABCDEFGHJKMNPQRI"`).
- **from_parts masking (#3) [minor]** — confirmed ts masked to 48 bits. Fix: tests constrain `ts < 2^48`.
- **MSRV 1.85 untested + resolver pulls older deps + let-chain contortion [minor/minor]** — Fix: set `rust-version = "1.95"` to match the pin (true by construction); this removes the nested-if/clippy-MSRV juggling. let-chains are legal but the title impl uses plain nested `if let` for clarity.
- **`Link` index struct is M1 [minor + major scope]** — confirmed untested, `source`/`resolved` only meaningful post-storage. Fix: dropped from M0; reintroduced in note-store/M1. `WikiTarget` + `WikiLink` fully satisfy the brief's M0 "wikilink target type + parser".
- **CI gold-plating: deny.toml/cargo-deny/cargo-audit/license reconciliation/justfile [major scope]** — Fix: all cut. M0 CI = exactly the three mandated gates via a thin `ci.sh`.
- **MAX_TAG_LEN / TooLong / hierarchical `/` invented [minor scope]** — Fix: dropped `TooLong` and the length cap. (UPDATE: the hierarchical-`/` open question was later RESOLVED with the user — `/` IS allowed; see §8. Char policy is alphanumeric + `-` + `_` + `/`.)
- **NoteId surface gold-plating [minor scope]** — Fix: removed `nil()`; kept the `From<Ulid>`/`From<NoteId>` idiom only (dropped `from_ulid`/`as_ulid`); kept `from_parts` + `timestamp_ms` (both verified `const fn`).
- **ulid version landmine on exhaustive From match [nit]** — Fix: keep the exhaustive 2-arm match (DecodeError is NOT `#[non_exhaustive]` in 1.2.1, so a future variant forces a major bump and a compile error here — desirable); add a comment.
- **placeholder repository/authors [nit + open item]** — Fix: omit `repository` and `authors` from the manifest until confirmed (build does not need them).
- **WikiLink Display↔FromStr not total over padded/empty ByTitle [major rust-idiom]** — confirmed. Fix: FromStr trims and rejects empty; round-trip is contractually guaranteed only for values produced via FromStr (always trimmed, non-empty, pipe-free). Documented on the type; example tests use trimmed inputs.

REJECTED (with reason):
- **"manual serde is likely avoidable / use ulid's `serde` feature" [scope major, option (b)]** — REJECTED, empirically false: with `default-features=false`, ulid's `serde` feature fails to compile (`E0433: cannot find type String` — `ulid/src/serde.rs` uses `String` without an alloc/std prelude import). Manual `Serialize`/`Deserialize` on `NoteId` stays.
- **"accept ulid default features in core" [scope major, option (a)]** — REJECTED in favor of keeping `default-features=false`: it documents the no-ambient-clock intent and blocks accidental `Ulid::new()`/wall-clock in core while M0 is the sole consumer; the cost (manual serde + a 4-line custom `From`) is tiny and already verified. (The critic's *underlying* point — that this is convention, not structure — is ACCEPTED via the source guard.)
- **"defer proptest" [scope minor, C14]** — ACCEPTED (proptest dropped); consequently the proptest reword nit and the dev-graph rand contamination are moot.
- **"trim all per-type serde tests to M4" [scope nit]** — PARTIALLY REJECTED: the brief makes export round-trip and "the id scheme must support this" an explicit M0 concern, so a *lean* serde test set (NoteId canonical string, Note JSON round-trip + defaults, Tag re-normalize-on-load, ContentKind/Timestamp wire format) is kept as forward-looking design locks. The broad exhaustive suite and all proptests are cut.
- **GitHub workflow [scope nit]** — KEPT but marked OPTIONAL convenience (three gates only), not an M0 done-when requirement.

---

## 1. Ordered implementation steps (top-to-bottom; tests-before-impl per module)

1. **Toolchain** — already verified (`cargo 1.95.0`, `rustfmt 1.9.0-stable`, `clippy 0.1.95`). No action.
2. **Root manifest** — `/var/home/luka/Projetos/note/Cargo.toml` (virtual workspace, `resolver="3"`, `[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`). §3.0.
3. **Root tooling** — `/var/home/luka/Projetos/note/rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`, `ci.sh` (chmod +x). §3.1.
4. **Stub crates** — `crates/note-store/{Cargo.toml,src/lib.rs}`, `crates/note-md/{Cargo.toml,src/lib.rs}`, `crates/note-tui/{Cargo.toml,src/lib.rs}`, `crates/note-cli/{Cargo.toml,src/main.rs}` (binary `note`, `fn main() {}`). §3.1b.
5. **Smoke build** — `cargo build --workspace` to confirm the 5-crate skeleton compiles before domain code.
6. **note-core manifest** — `crates/note-core/Cargo.toml`: deps `ulid {default-features=false}`, `serde {derive}`, `thiserror`; dev-dep `serde_json`. §3.1c.
7. **note-core root** — `crates/note-core/src/lib.rs` (`#![forbid(unsafe_code)]`, module wiring, `pub use`). Create remaining module files as empty stubs so the crate compiles. §3.2.
8. **error.rs** — implement the full taxonomy first (every parser's `FromStr::Err` references it). §3.3.
9. **id.rs** — tests (red) then `NoteId` impl (green), incl. the canonical guard. §3.4 / §4.
10. **time.rs** — tests then `Timestamp`. §3.5 / §4.
11. **content.rs** — tests then `ContentKind`. §3.6 / §4.
12. **tag.rs** — tests then `Tag` + canonicalization. §3.7 / §4.
13. **link.rs** — tests then `WikiTarget`/`WikiLink` (+ FromStr/Display). §3.8 / §4.
14. **title.rs** — table-driven tests then `derive_title`/`first_h1`/`first_non_empty_line`. §3.9 / §4.
15. **note.rs** — tests then `Note` + `effective_title`. §3.10 / §4.
16. **tests/public_api.rs** — black-box surface + typed-identity lock. §4.
17. **tests/io_free_guard.rs** — durable source-level guard (core never names `Ulid::new`/`Ulid::from_datetime`/`SystemTime`/`Instant::now`/`std::time`/`std::fs`/`std::net`). §3.11 / §4.
18. **Format once** — `cargo fmt --all` so the first commit passes gate 1.
19. **Run gates** — `./ci.sh` (the three commands in §5). Relax any newly-added workspace lint that fires on the verified domain code, then re-run.
20. **(Optional) CI workflow** — `.github/workflows/ci.yml` running the three gates.
21. **Commit** — `git init`, branch off, logically-grouped conventional commits, **commit `Cargo.lock`** (workspace ships the `note` binary).

---

## 2. M0 file tree
```
/var/home/luka/Projetos/note/
├── Cargo.toml                  # virtual workspace
├── Cargo.lock                  # COMMITTED
├── rust-toolchain.toml         # channel = "1.95.0"
├── rustfmt.toml                # style_edition = "2024"
├── .gitignore
├── ci.sh                       # the 3 gates, fail-fast
├── .github/workflows/ci.yml    # OPTIONAL: same 3 gates
└── crates/
    ├── note-core/              # ONLY crate with real code in M0
    │   ├── Cargo.toml
    │   ├── src/{lib,error,id,time,content,tag,link,title,note}.rs
    │   └── tests/{public_api,io_free_guard}.rs
    ├── note-store/{Cargo.toml,src/lib.rs}   # STUB (M1)
    ├── note-md/{Cargo.toml,src/lib.rs}      # STUB (M3/M4)
    ├── note-tui/{Cargo.toml,src/lib.rs}     # STUB (M6)
    └── note-cli/{Cargo.toml,src/main.rs}    # STUB BINARY `note` (M2)
```
No `Link` struct, no `deny.toml`, no `justfile`, no `proptest-regressions/`.

---

## 3. Concrete configuration + final Rust signatures

### 3.0 Root `Cargo.toml`
```toml
[workspace]
resolver = "3"
members = [
  "crates/note-core", "crates/note-store", "crates/note-md",
  "crates/note-tui", "crates/note-cli",
]

[workspace.package]
version      = "0.1.0"
edition      = "2024"
rust-version = "1.95"            # matches the pinned toolchain (true by construction)
license      = "MIT OR Apache-2.0"   # OPEN QUESTION: confirm
description  = "Terminal note-taking (CLI + TUI) over a single SQLite file"
# repository / authors intentionally omitted until confirmed (not needed to build)

[workspace.dependencies]
note-core  = { path = "crates/note-core",  version = "0.1.0" }
note-store = { path = "crates/note-store", version = "0.1.0" }
note-md    = { path = "crates/note-md",    version = "0.1.0" }
note-tui   = { path = "crates/note-tui",   version = "0.1.0" }
note-cli   = { path = "crates/note-cli",   version = "0.1.0" }
# default-features=false => no std/rand/getrandom => no ambient Ulid::new()/clock in core.
# NOTE: this blocks ambient ctors only while M0 is the sole ulid consumer; in M1+ feature
# unification can turn std on. The DURABLE guard is tests/io_free_guard.rs, not this flag.
ulid       = { version = "1.2", default-features = false }
thiserror  = { version = "2.0" }
serde      = { version = "1", features = ["derive"] }
serde_json = { version = "1" }   # note-core dev-dep only (round-trip tests); note-md M3/M4
# FORWARD REFERENCE ONLY — do NOT add in M0: rusqlite/refinery (M1), clap (M2),
# pulldown-cmark (M3), figment/anyhow/tracing (cli edge), ratatui/ratatui-tea (M6).
# Edge crates that MINT ids (note-store M1, note-cli M2) depend on ulid WITH default features.

[workspace.lints.rust]
unsafe_code                   = "forbid"
missing_debug_implementations = "warn"
unused_qualifications         = "warn"
unreachable_pub               = "warn"
rust_2018_idioms              = { level = "warn", priority = -1 }

[workspace.lints.clippy]
all      = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
# pedantic de-noise for an early skeleton:
module_name_repetitions = "allow"
missing_errors_doc      = "allow"
missing_panics_doc      = "allow"
must_use_candidate      = "allow"
doc_markdown            = "allow"
# pedantic de-noise for TEST code (verified to fire under --all-targets -D warnings):
unreadable_literal      = "allow"
similar_names           = "allow"
items_after_statements  = "allow"
cast_possible_truncation = "allow"
# CI runs `clippy ... -- -D warnings`, so every `warn` above is a HARD error in CI.
# After scaffolding, relax any rust-lint that fires on the verified domain code and re-run.
```

### 3.1 Root tooling
`rust-toolchain.toml`
```toml
[toolchain]
channel    = "1.95.0"
components = ["rustfmt", "clippy"]
profile    = "minimal"
```
`rustfmt.toml`
```toml
style_edition = "2024"
max_width     = 100
newline_style = "Unix"
use_small_heuristics = "Default"
# group_imports / imports_granularity / wrap_comments are nightly-only -> omitted.
# `edition` not set: cargo fmt uses each crate's edition.
```
`.gitignore`
```gitignore
/target
**/*.rs.bk
.DS_Store
*.swp
*.swo
.idea/
.vscode/
# forward-looking (M1+) local SQLite scratch DBs
*.sqlite
*.sqlite-shm
*.sqlite-wal
*.db
*.db-shm
*.db-wal
# Cargo.lock IS committed (ships the `note` binary). Do NOT ignore it.
```
`ci.sh`
```bash
#!/usr/bin/env bash
set -euo pipefail
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### 3.1b Stub crates
All four use `version.workspace=true`, `edition.workspace=true`, `rust-version.workspace=true`, `license.workspace=true`, a unique `description`, `[lints] workspace=true`, and NO dependencies in M0. `note-cli` adds `[[bin]] name = "note"` / `path = "src/main.rs"`. Stub sources are a `//! …` module-doc only; `note-cli/src/main.rs` is `//! note binary entry point. CLI parsing (clap) arrives in M2.` + `fn main() {}`.

### 3.1c `crates/note-core/Cargo.toml`
```toml
[package]
name = "note-core"
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
description = "IO-free domain types, typed ids, and errors for note"

[dependencies]
ulid      = { workspace = true }   # default-features=false
serde     = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }

[lints]
workspace = true
```

### 3.2 `src/lib.rs`
```rust
#![forbid(unsafe_code)]
//! note-core: the IO-free, front-end-free domain model for `note`.
//!
//! INVARIANTS (M0): no rusqlite, no filesystem, no ratatui, no clap, no network.
//! No ambient clock or RNG: timestamps and id entropy are INJECTED at the edge
//! (note-cli / note-store). `ulid` is built without its std feature so the
//! ambient constructors are unavailable here; the durable enforcement is the
//! source guard in tests/io_free_guard.rs.

mod content;
mod error;
mod id;
mod link;
mod note;
mod tag;
mod time;
mod title;

pub use content::ContentKind;
pub use error::{Error, IdError, Result, TagError, WikiError};
pub use id::NoteId;
pub use link::{WikiLink, WikiTarget};
pub use note::Note;
pub use tag::Tag;
pub use time::Timestamp;
pub use title::derive_title;
```

### 3.3 `src/error.rs`
```rust
use thiserror::Error;

/// Crate-wide result alias; `E` defaults to [`Error`].
pub type Result<T, E = Error> = core::result::Result<T, E>;

/// Failure parsing a ULID-backed id (26-char Crockford base32).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum IdError {
    #[error("note id has invalid length (expected 26 Crockford base32 chars)")]
    InvalidLength,
    #[error("note id contains an invalid character")]
    InvalidChar,
    /// 26 valid chars but the value overflows 128 bits, so ulid silently masks it
    /// to a different canonical string; rejected to keep parse<->display total.
    #[error("note id is not in canonical form (value out of range)")]
    NonCanonical,
}
impl From<ulid::DecodeError> for IdError {
    // Intentionally exhaustive against ulid::DecodeError (NOT #[non_exhaustive] in
    // 1.2.x): a future added variant must break this match and force review.
    fn from(e: ulid::DecodeError) -> Self {
        match e {
            ulid::DecodeError::InvalidLength => Self::InvalidLength,
            ulid::DecodeError::InvalidChar   => Self::InvalidChar,
        }
    }
}

/// Failure normalizing/validating a [`crate::Tag`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TagError {
    #[error("tag is empty after normalization")]
    Empty,
    #[error("tag {input:?} contains invalid character {ch:?}")]
    InvalidChar { input: String, ch: char },
}

/// Failure parsing the inner content of a wikilink target.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum WikiError {
    #[error("wikilink target is empty")]
    EmptyTarget,
}

/// Crate-wide umbrella for multi-step fallible flows.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)] Id(#[from] IdError),
    #[error(transparent)] Tag(#[from] TagError),
    #[error(transparent)] Wiki(#[from] WikiError),
}
```

### 3.4 `src/id.rs`
```rust
use core::{fmt, str::FromStr};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ulid::Ulid;
use crate::error::IdError;

/// Typed identity for a note: 128-bit ULID, lexicographically time-sortable,
/// 26-char Crockford base32, stable across export/import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NoteId(Ulid);

impl NoteId {
    /// Deterministic mint for tests / import: NO clock, NO RNG. (`Ulid::from_parts`
    /// masks ts to 48 bits and random to 80 bits.) Both this and `timestamp_ms`
    /// are verified `const fn` on ulid 1.2.x.
    #[inline] pub const fn from_parts(timestamp_ms: u64, random: u128) -> Self {
        Self(Ulid::from_parts(timestamp_ms, random))
    }
    #[inline] pub const fn timestamp_ms(self) -> u64 { self.0.timestamp_ms() }
}
impl From<Ulid> for NoteId { fn from(u: Ulid) -> Self { Self(u) } }
impl From<NoteId> for Ulid { fn from(n: NoteId) -> Self { n.0 } }

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { fmt::Display::fmt(&self.0, f) }
}
impl FromStr for NoteId {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let id = Ulid::from_string(s).map_err(IdError::from)?;
        // ulid accepts lowercase (normalizes to uppercase) and silently masks
        // 26-char overflow inputs. Compare against the uppercased input so valid
        // lowercase ids pass but overflow/garbage is rejected.
        if id.to_string() != s.to_ascii_uppercase() {
            return Err(IdError::NonCanonical);
        }
        Ok(Self(id))
    }
}
// Serialize as the 26-char canonical string (portable JSON for M4 export round-trip).
// MANUAL impls are REQUIRED: ulid's `serde` feature fails to compile under
// default-features=false (verified: E0433, uses String without the std prelude).
impl Serialize for NoteId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for NoteId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
```

### 3.5 `src/time.rs`
```rust
use serde::{Deserialize, Serialize};

/// UTC instant as milliseconds since the Unix epoch (maps to SQLite INTEGER in M1).
/// Core NEVER reads the wall clock; the edge supplies `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const UNIX_EPOCH: Self = Self(0);
    #[inline] pub const fn from_unix_millis(ms: i64) -> Self { Self(ms) }
    #[inline] pub const fn as_unix_millis(self) -> i64 { self.0 }
}
// Intentionally absent: Timestamp::now() -> edge (M1/M2).
```

### 3.6 `src/content.rs`
```rust
use core::fmt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Plain,
    #[default] Markdown,
}
impl ContentKind {
    pub const fn as_str(self) -> &'static str {
        match self { Self::Plain => "plain", Self::Markdown => "markdown" }
    }
    pub const fn is_markdown(self) -> bool { matches!(self, Self::Markdown) }
}
impl fmt::Display for ContentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.as_str()) }
}
// FromStr (md/text CLI aliases) deferred to note-cli (M2).
```

### 3.7 `src/tag.rs`
```rust
use core::{fmt, str::FromStr};
use serde::{Deserialize, Deserializer, Serialize};
use crate::error::TagError;

/// Canonical tag. FORM: trim; strip ONE leading '#'; trim; Unicode-lowercase;
/// every char must be Unicode-alphanumeric or '-' or '_' or '/'. The '/' enables
/// hierarchical tags (e.g. `projeto/note`, Obsidian-style). Empty after
/// normalization is an error. (No length cap in M0; per-segment validation —
/// rejecting empty segments like `a//b` or a leading/trailing '/' — is a
/// deferred refinement.)
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(into = "String")]
pub struct Tag(String);

impl Tag {
    pub fn new(raw: &str) -> Result<Self, TagError> {
        let s = raw.trim();
        let s = s.strip_prefix('#').unwrap_or(s).trim();
        if s.is_empty() { return Err(TagError::Empty); }
        let canon = s.to_lowercase();
        if let Some(ch) = canon.chars().find(|c| !Self::is_allowed(*c)) {
            return Err(TagError::InvalidChar { input: canon, ch });
        }
        Ok(Self(canon))
    }
    fn is_allowed(c: char) -> bool { c.is_alphanumeric() || matches!(c, '-' | '_' | '/') }
    #[inline] pub fn as_str(&self) -> &str { &self.0 }
    #[inline] pub fn into_string(self) -> String { self.0 }
}
impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) } // no leading '#'
}
impl FromStr for Tag { type Err = TagError; fn from_str(s: &str) -> Result<Self, Self::Err> { Self::new(s) } }
impl TryFrom<String> for Tag { type Error = TagError; fn try_from(s: String) -> Result<Self, Self::Error> { Self::new(&s) } }
impl From<Tag> for String { fn from(t: Tag) -> Self { t.0 } }
impl<'de> Deserialize<'de> for Tag {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Tag::new(&s).map_err(serde::de::Error::custom)   // re-normalizes on load
    }
}
```

### 3.8 `src/link.rs` (Model X; NO `Link` index struct — that is M1)
```rust
use core::{fmt, str::FromStr};
use serde::{Deserialize, Serialize};
use crate::error::WikiError;
use crate::id::NoteId;

/// Target of a [[wikilink]]: a stable id OR a (possibly dangling) title.
/// A token is `ById` iff it parses as a canonical ULID; otherwise `ByTitle`.
/// Resolving `ByTitle -> NoteId` is a later milestone (note-store, M1+).
/// CANONICAL FORM: a `ByTitle` produced via `FromStr` is always trimmed,
/// non-empty, and pipe-free; Display<->FromStr round-trips for such values.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WikiTarget { ById(NoteId), ByTitle(String) }
impl WikiTarget {
    fn classify(token: &str) -> Self {
        match token.parse::<NoteId>() {
            Ok(id) => Self::ById(id),
            Err(_) => Self::ByTitle(token.to_string()),
        }
    }
}
impl fmt::Display for WikiTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self { Self::ById(id) => write!(f, "{id}"), Self::ByTitle(t) => f.write_str(t) }
    }
}
impl FromStr for WikiTarget {
    type Err = WikiError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        if t.is_empty() { return Err(WikiError::EmptyTarget); }
        Ok(Self::classify(t))
    }
}

/// A parsed wikilink occurrence. PIPE SEMANTICS (Obsidian/Zettelkasten, Model X):
/// text LEFT of `|` = target, text RIGHT of `|` = display/alias. FromStr accepts
/// the inner content and tolerates surrounding `[[ ]]`; empty display => None.
/// (Scanning `[[...]]` spans out of a markdown body is note-md/M3.)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WikiLink { pub target: WikiTarget, pub display: Option<String> }
impl FromStr for WikiLink {
    type Err = WikiError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let inner = s.trim();
        let inner = inner.strip_prefix("[[").and_then(|x| x.strip_suffix("]]")).unwrap_or(inner);
        let (target_str, display) = match inner.split_once('|') {
            Some((t, d)) => { let d = d.trim(); (t, if d.is_empty() { None } else { Some(d.to_string()) }) }
            None => (inner, None),
        };
        Ok(Self { target: target_str.parse()?, display })
    }
}
impl fmt::Display for WikiLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.display {
            Some(d) => write!(f, "{}|{}", self.target, d),
            None    => write!(f, "{}", self.target),
        }
    }
}
```

### 3.9 `src/title.rs` (trivial ATX H1; no fence/setext/closing-hash — that is M3)
```rust
use crate::content::ContentKind;

/// Derive the effective (display) title:
/// 1) explicit non-empty (trimmed) title; else
/// 2) for Markdown, the first ATX H1 (a line whose trimmed form is `#` + whitespace
///    + non-empty text — NO fenced-code awareness, NO setext, NO closing-hash trim;
///    real CommonMark parsing is note-md/M3 and may refine this); else
/// 3) the first non-empty line, trimmed; else
/// 4) empty string.
pub fn derive_title(title: Option<&str>, body: &str, kind: ContentKind) -> String {
    if let Some(t) = title {
        let t = t.trim();
        if !t.is_empty() { return t.to_string(); }
    }
    if kind.is_markdown() {
        if let Some(h1) = first_h1(body) { return h1; }
    }
    first_non_empty_line(body).unwrap_or_default()
}

fn first_h1(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let rest = line.trim().strip_prefix('#')?;        // exactly one leading '#'
        if rest.starts_with(char::is_whitespace) {        // followed by whitespace = ATX H1
            let h = rest.trim();
            (!h.is_empty()).then(|| h.to_string())
        } else {
            None                                          // "## Sub", "#NoSpace" are not H1
        }
    })
}
fn first_non_empty_line(body: &str) -> Option<String> {
    body.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}
```

### 3.10 `src/note.rs`
```rust
use std::collections::BTreeSet;
use serde::{Deserialize, Serialize};
use crate::{ContentKind, NoteId, Tag, Timestamp};
use crate::title::derive_title;

/// A note. The BODY is the source of truth for wikilinks (no link field here);
/// `tags` are authoritative metadata. created/updated are explicit (not derived
/// from the id) so export->import preserves them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: NoteId,
    pub title: Option<String>,
    pub body: String,
    #[serde(default)] pub content_kind: ContentKind,
    #[serde(default)] pub tags: BTreeSet<Tag>,
    pub created: Timestamp,
    pub updated: Timestamp,
}
impl Note {
    /// All time/id values are INJECTED by the caller (edge), never read here.
    pub fn new(
        id: NoteId, body: impl Into<String>, content_kind: ContentKind,
        created: Timestamp, updated: Timestamp,
    ) -> Self {
        Self { id, title: None, body: body.into(), content_kind, tags: BTreeSet::new(), created, updated }
    }
    pub fn effective_title(&self) -> String {
        derive_title(self.title.as_deref(), &self.body, self.content_kind)
    }
}
```

### 3.11 `tests/io_free_guard.rs` (durable cross-milestone guard)
```rust
//! Asserts note-core source never reaches for an ambient clock, RNG, or IO,
//! independent of Cargo feature unification (which can turn ulid's std on in M1+).
//! Tests MAY do IO; the library may not. Comment lines are skipped.
use std::{fs, path::Path};

const FORBIDDEN: &[&str] = &[
    "Ulid::new", "Ulid::from_datetime", "SystemTime", "Instant::now",
    "std::time", "std::fs", "std::net",
];

#[test]
fn core_is_io_and_ambient_free() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() { stack.push(path); continue; }
            if path.extension().is_none_or(|e| e != "rs") { continue; }
            let text = fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("//") || t.starts_with('*') { continue; } // skip comments/docs
                for needle in FORBIDDEN {
                    if line.contains(needle) {
                        offenders.push(format!("{}:{}: {needle}", path.display(), n + 1));
                    }
                }
            }
        }
    }
    assert!(offenders.is_empty(), "ambient/IO usage in note-core src:\n{}", offenders.join("\n"));
}
```

---

## 4. Full M0 test list (example-based; no proptest in M0)
Unit tests live inline as `#[cfg(test)] mod tests` to reach private parsers. Rejection tests assert the **exact** error variant (never bare `is_err()`).

**`id.rs`**
1. `noteid_parse_display_roundtrip` — known canonical 26-char ULID parses; `to_string()` returns it.
2. `noteid_display_is_26_chars`.
3. `noteid_from_parts_recovers_timestamp` — `from_parts(1_469_922_850_259, 0xDEAD_BEEF).timestamp_ms() == 1_469_922_850_259` (ts < 2^48).
4. `noteid_from_str_trims_whitespace`.
5. `noteid_accepts_lowercase` — lowercase canonical parses; Display is uppercase.
6. `noteid_rejects_empty` → `IdError::InvalidLength`.
7. `noteid_rejects_too_short` (25 chars) → `InvalidLength`.
8. `noteid_rejects_too_long` (27 chars) → `InvalidLength`.
9. `noteid_rejects_invalid_char` — 26-char fixture `"0123456789ABCDEFGHJKMNPQRI"` (ends in `I`) → `InvalidChar`.
10. `noteid_rejects_noncanonical_overflow` — `"80000000000000000000000000"` → `IdError::NonCanonical`.
11. `noteid_time_ordering` — `from_parts(t1,_) < from_parts(t2,_)` for t1<t2 (both < 2^48).
12. `noteid_hash_dedup` — same id twice in a `HashSet` → len 1.
13. `noteid_serde_json_is_canonical_string` — `serde_json::to_value(id)` is a `String` equal to Display (export contract).
14. `noteid_serde_json_roundtrip`.
15. `noteid_serde_rejects_bad_string`.

**`time.rs`**
16. `timestamp_roundtrip_millis`.
17. `timestamp_ordering`.
18. `timestamp_unix_epoch_is_zero`.
19. `timestamp_serde_is_transparent_integer` — serializes to a bare JSON integer and round-trips.

**`content.rs`**
20. `contentkind_default_is_markdown`.
21. `contentkind_as_str_and_display` — Plain→"plain", Markdown→"markdown" for both.
22. `contentkind_is_markdown`.
23. `contentkind_serde_lowercase_roundtrip`.

**`tag.rs`**
24. `tag_trims_and_lowercases` — `"  Rust  "` → `"rust"`.
25. `tag_strips_single_leading_hash` — `"#rust"` → `"rust"`.
26. `tag_allows_dash_underscore` — `"a_b-c"` Ok.
27. `tag_equality_after_normalization` — `Tag::new("Rust") == Tag::new("  rust ")`.
28. `tag_ord_by_canonical_value` — `["zebra","Apple"]` → apple < zebra.
29. `tag_set_collapses_duplicates` — BTreeSet of `["Rust","rust"," RUST ","#rust"]` → len 1.
30. `tag_display_has_no_hash`.
31. `tag_rejects_empty` → `TagError::Empty`.
32. `tag_rejects_whitespace_only` → `Empty`.
33. `tag_rejects_only_hash` (`"#"`) → `Empty`.
34. `tag_rejects_internal_whitespace` (`"two words"`) → `InvalidChar { ch: ' ', .. }`.
35. `tag_rejects_punctuation` (`"bad!"`, also `,`, `|`) → `InvalidChar`.
36. `tag_allows_slash` (`"Projeto/Note"`) → Ok, canonical `"projeto/note"` (hierarchical tags enabled).
37. `tag_serde_renormalizes_on_deserialize` — `"\"Rust\""` → `Tag("rust")`.

**`link.rs`**
38. `wikitarget_id_form` — canonical ULID token → `ById`.
39. `wikitarget_title_form` — `"Some Title"` → `ByTitle("Some Title")`.
40. `wikitarget_idish_but_invalid_is_title` — `"01ARZ"` → `ByTitle("01ARZ")`.
41. `wikitarget_overflow_is_title` — `"80000000000000000000000000"` (non-canonical) → `ByTitle(...)`.
42. `wikitarget_rejects_empty` — `""` and `"   "` → `WikiError::EmptyTarget`.
43. `wikitarget_display_roundtrip` — `ById`/`ByTitle` (trimmed) Display re-parses equal.
44. `wikilink_title_only` — display None.
45. `wikilink_id_only` — display None.
46. `wikilink_target_pipe_display` — `"Some Title|click here"` → `ByTitle("Some Title")`, display `"click here"`; Display round-trips byte-identically.
47. `wikilink_id_pipe_display` — `"<ulid>|Display Text"` → `ById`, display `"Display Text"`; round-trips.
48. `wikilink_strips_outer_brackets` — `"[[Plain]]"` → `ByTitle("Plain")`, None.
49. `wikilink_empty_display_is_none` — `"Plain|"` → None.
50. `wikilink_empty_target_errors` — `"|Display"` and `"|"` → `EmptyTarget`.

**`title.rs`**
51. `title_explicit_wins` — title `Some("Explicit")`, body `"# Other"` → `"Explicit"`.
52. `title_empty_explicit_falls_through` — `Some("   ")` → derives from body.
53. `title_md_h1_atx` — `("# Hello\nbody", Markdown)` → `"Hello"`.
54. `title_md_h1_trims_inner_space` — `"#   Spaced   \n"` → `"Spaced"`.
55. `title_md_h1_requires_space` — `"#NoSpace\n"` → `"#NoSpace"` (not a heading).
56. `title_md_only_level_one` — `"## Sub\n# Real"` → `"Real"`; `"## OnlySub"` → `"## OnlySub"`.
57. `title_first_nonempty_line` — `("plain para\nmore", Markdown)` no H1 → `"plain para"`.
58. `title_skips_leading_blanks` — `"\n\n  \n# Title"` → `"Title"`; `"\n\n \nFirst real"` → `"First real"`.
59. `title_handles_crlf` — `"# Title\r\nbody"` → `"Title"` (no trailing `\r`).
60. `title_plain_kind_ignores_hash` — `("# not a heading", Plain)` → `"# not a heading"`.
61. `title_empty_body` — `("", Markdown)` and `("", Plain)` → `""`.
62. `title_whitespace_only_body` — `("  \n\t\n", _)` → `""`.
(Removed from prior draft: fenced-code `# fake` and closing-hash `# Hello ##` cases — those are M3 CommonMark semantics; M0's detector is deliberately trivial.)

**`note.rs`**
63. `note_effective_title_from_h1`.
64. `note_effective_title_explicit_overrides`.
65. `note_json_roundtrip` — id as 26-char string, tags re-normalized, defaults applied.
66. `note_serde_defaults` — JSON missing `content_kind`/`tags` → Markdown + empty set.

**`error.rs`**
67. `error_display_nonempty` — Display of every variant of `IdError`, `TagError`, `WikiError`, umbrella `Error` is non-empty.
68. `error_from_conversions` — each leaf converts into umbrella `Error` via `?`/`From`.

**`tests/public_api.rs`**
69. `public_surface_is_reachable` — using only `note_core::{…}` public paths: build `NoteId` (`from_parts`/`FromStr`), `Tag::new`, `WikiLink::from_str`, `WikiTarget::from_str`, call `derive_title`, build a `Note` with tags + content kind.
70. `note_uses_typed_noteid` — `let _: NoteId = note.id;` (typed-identity at the boundary).
71. `export_id_is_canonical_string` — `NoteId` Display equals its `serde_json` string form.

**`tests/io_free_guard.rs`**
72. `core_is_io_and_ambient_free` — no forbidden ambient/IO tokens in `src/`.

---

## 5. CI gates (exact, in order, fail-fast)
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
- `-D warnings` is passed ONLY on the clippy command line (scopes denial to workspace crates); do NOT set `RUSTFLAGS=-D warnings` globally (would deny third-party warnings).
- No `--all-features` (M0 declares no Cargo features).
- `ci.sh` runs exactly these three. No cargo-deny / cargo-audit in M0.
- Optional `.github/workflows/ci.yml`: one job, toolchain auto-installed from `rust-toolchain.toml`, cache via `Swatinem/rust-cache@v2`, runs the three gates.

---

## 6. Done-when checklist
- [ ] `cargo build --workspace` green — all five crates compile (four clean stubs; `note-cli` produces the `note` binary).
- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo test --workspace` green — every test in §4 passes (incl. `public_api` and `io_free_guard`).
- [ ] note-core unit tests cover the four mandated areas: NoteId round-trip (parse<->display via ULID), tag normalization, title derivation (H1 / first-line / empty), wikilink-target parsing (id / title / alias forms).
- [ ] Durable IO-free guard `core_is_io_and_ambient_free` passes (core never names `Ulid::new`/`Ulid::from_datetime`/`SystemTime`/`Instant::now`/`std::time`/`std::fs`/`std::net`).
- [ ] M0 sanity: `cargo tree -p note-core --edges normal | grep -E 'rand|getrandom'` returns nothing (M0-only; the source guard above is the cross-milestone guarantee).
- [ ] `Cargo.lock` committed; `/target` and `*.sqlite` ignored.
- [ ] note-core source carries NO rusqlite / filesystem / ratatui / clap / network.

---

## 7. Held at the M0 boundary (do NOT pull forward)
- Id minting (`Ulid::new`, wall clock, entropy) → note-store/note-cli (M1/M2). Core has only `from_parts`/`From<Ulid>`.
- `Timestamp::now()` → edge.
- Markdown body scanning for `[[…]]` spans, fenced-code / setext / ATX-edge title rules, real CommonMark (pulldown-cmark) → note-md (M3). M0's H1 detector is deliberately trivial and will be reconciled with pulldown-cmark in M3.
- Link resolution (`ByTitle → NoteId`) and the directed/resolved link-index struct → note-store/M1.
- `ContentKind::FromStr` (md/text CLI aliases) → note-cli (M2).
- anyhow → binary edge (M2). Core uses thiserror only.
- Export/import implementation → note-md (M3/M4). serde lives in core as design + dev-dep tests only.
- Supply-chain tooling (cargo-deny/cargo-audit) → a later milestone when real third-party deps land.
- proptest → deferred; example-based tests fully cover the four mandated areas.

---

## 8. Resolved decisions (settled with the user)
1. **License = `MIT OR Apache-2.0`** (standard Rust dual-license). `repository` slug + `authors` still omitted from the manifest until confirmed; not needed to build.
2. **Hierarchical tags ENABLED.** Tag char policy is Unicode-alphanumeric + `-` + `_` + `/`; `projeto/note` is valid. Test #36 is now `tag_allows_slash`. Per-segment validation (empty segments, leading/trailing `/`) is a deferred refinement, not an M0 blocker.
3. **Tags are case-insensitive** (`Rust`/`RUST`/`rust` → `rust`, with dedup). Confirmed intended.
4. **Wikilink pipe = Model X**: `[[target|display]]` — left of `|` is the target (title or id), right is the display alias. Confirmed. (A future Model-Y reconsideration, where a ULID side is always canonical regardless of position, can be revisited before M3 but is not planned.)

## 9. Still to confirm before first publishable commit (non-blocking for coding)
- `repository` URL and `authors` string for the workspace manifest.
