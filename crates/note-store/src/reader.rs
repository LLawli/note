//! The read side: a small pool of read-only connections, separate from the
//! single writer. WAL lets these read concurrently with an in-flight write.

use crate::error::Result;
use crate::model::{self, Link};
use note_core::{Note, NoteId, Tag, WikiTarget};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params, params_from_iter};
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// A pool of read-only SQLite connections. Connections are created on demand and
/// returned to an idle list for reuse.
#[derive(Debug)]
pub struct ReaderPool {
    path: PathBuf,
    idle: Mutex<Vec<Connection>>,
}

/// RAII handle to a checked-out connection; returns it to the pool on drop
/// (including during panic unwinding), so a connection is never lost.
struct Pooled<'a> {
    pool: &'a ReaderPool,
    conn: Option<Connection>,
}

impl Drop for Pooled<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.idle().push(conn);
        }
    }
}

impl Deref for Pooled<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn
            .as_ref()
            .expect("pooled connection present until drop")
    }
}

impl ReaderPool {
    pub(crate) fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            idle: Mutex::new(Vec::new()),
        }
    }

    fn idle(&self) -> MutexGuard<'_, Vec<Connection>> {
        // Recover from poisoning rather than cascading a panic across all reads;
        // the inner Vec<Connection> is always in a consistent state.
        self.idle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn checkout(&self) -> Result<Connection> {
        if let Some(conn) = self.idle().pop() {
            return Ok(conn);
        }
        let conn = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(conn)
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        // The guard returns the connection to the pool on drop, even if `f`
        // panics (no leak / no permanent loss of a pooled connection).
        let guard = Pooled {
            pool: self,
            conn: Some(self.checkout()?),
        };
        f(&guard)
    }

    /// Run an id-selecting query and hydrate the matched notes on a single pooled
    /// connection (one notes query + one tags query per chunk). The shared shape
    /// behind every list/search read.
    fn notes_for<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Vec<Note>> {
        self.with(|conn| {
            let ids = {
                let mut stmt = conn.prepare(sql)?;
                collect_ids(stmt.query(params)?)?
            };
            load_all(conn, &ids)
        })
    }

    /// Fetch a single note by its exact id (the chunk-of-one case of `load_all`).
    pub fn get_note(&self, id: NoteId) -> Result<Option<Note>> {
        self.with(|conn| Ok(load_all(conn, &[id.to_string()])?.into_iter().next()))
    }

    /// The most recently updated note, if any (powers `note show` with no arg).
    pub fn most_recent(&self) -> Result<Option<Note>> {
        self.with(|conn| {
            let id: Option<String> = conn
                .query_row(
                    "SELECT id FROM notes ORDER BY updated DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .optional()?; // only no-rows -> None; real errors propagate
            match id {
                Some(id) => Ok(load_all(conn, &[id])?.into_iter().next()),
                None => Ok(None),
            }
        })
    }

    /// Notes ordered most-recently-updated first.
    pub fn list_notes(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let limit = clamp_i64(limit);
        let offset = clamp_i64(offset);
        self.notes_for(
            "SELECT id FROM notes ORDER BY updated DESC LIMIT ?1 OFFSET ?2",
            params![limit, offset],
        )
    }

    /// Prefix-aware full-text search over a user-typed query: each whitespace
    /// term is matched as a prefix (so "mensag" finds "mensagem"). Terms are
    /// AND-ed. An empty/whitespace query returns no matches.
    pub fn search_prefix(&self, user_query: &str, limit: usize) -> Result<Vec<Note>> {
        let query = to_prefix_query(user_query);
        if query.is_empty() {
            return Ok(Vec::new());
        }
        self.search(&query, limit)
    }

    /// Full-text search over title + body, best matches first. Takes a raw FTS5
    /// query; callers wanting prefix behavior should use [`Self::search_prefix`].
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        let limit = clamp_i64(limit);
        self.notes_for(
            "SELECT n.id FROM notes_fts f
             JOIN notes n ON n.rowid = f.rowid
             WHERE notes_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
            params![query, limit],
        )
    }

    /// All tags with their note counts, ordered alphabetically.
    pub fn all_tags(&self) -> Result<Vec<(Tag, usize)>> {
        self.with(|conn| {
            let mut stmt =
                conn.prepare("SELECT tag, COUNT(*) FROM tags GROUP BY tag ORDER BY tag")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (raw, count) = row?;
                let tag = model::parse_tag(&raw)?;
                out.push((tag, usize::try_from(count).unwrap_or(0)));
            }
            Ok(out)
        })
    }

    /// Notes carrying `tag`, most-recently-updated first.
    pub fn list_by_tag(&self, tag: &Tag, limit: usize) -> Result<Vec<Note>> {
        let limit = clamp_i64(limit);
        self.notes_for(
            "SELECT n.id FROM notes n
             JOIN tags t ON t.note_id = n.id
             WHERE t.tag = ?1
             ORDER BY n.updated DESC
             LIMIT ?2",
            params![tag.as_str(), limit],
        )
    }

    /// Every note, ordered most-recently-updated first (powers export).
    pub fn all_notes(&self) -> Result<Vec<Note>> {
        self.notes_for("SELECT id FROM notes ORDER BY updated DESC", [])
    }

    /// Total number of notes (powers `note status`).
    pub fn count_notes(&self) -> Result<usize> {
        self.with(|conn| {
            let n: i64 = conn.query_row("SELECT count(*) FROM notes", [], |r| r.get(0))?;
            Ok(usize::try_from(n).unwrap_or(0))
        })
    }

    /// Resolve a user-supplied reference to candidate notes, in priority order:
    /// full canonical ULID, then a git-style ULID prefix, then an exact
    /// case-insensitive effective-title match, then a full-text fallback. Returns
    /// every candidate; the caller decides unique vs ambiguous vs none.
    pub fn resolve_ref(&self, reference: &str) -> Result<Vec<Note>> {
        let r = reference.trim();
        if r.is_empty() {
            return Ok(Vec::new());
        }
        // 1. full canonical id
        if let Ok(id) = NoteId::from_str(r) {
            return Ok(self.get_note(id)?.into_iter().collect());
        }
        // 2. git-style ULID prefix (Crockford chars, shorter than a full id)
        if is_ulid_prefix(r) {
            let pattern = format!("{}%", r.to_ascii_uppercase());
            let hits = self.notes_for(
                "SELECT id FROM notes WHERE id LIKE ?1 ORDER BY id LIMIT 50",
                params![pattern],
            )?;
            if !hits.is_empty() {
                return Ok(hits);
            }
        }
        // 3 + 4. exact effective-title match wins; else fall back to the same
        // (already-fetched) ranked FTS candidates. One scan, shared with the
        // writer's link resolution via `title_candidates` (no double-scan, no
        // divergence).
        self.with(|conn| {
            let (exact, all) = title_candidates(conn, r)?;
            let chosen = if exact.is_empty() { all } else { exact };
            let ids: Vec<String> = chosen.iter().map(NoteId::to_string).collect();
            load_all(conn, &ids)
        })
    }

    /// Outgoing links for a note (the link graph row view).
    pub fn links_for(&self, source: NoteId) -> Result<Vec<Link>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT target_kind, target_value, display, resolved_id
                 FROM links WHERE source_id = ?1",
            )?;
            let rows = stmt.query_map(params![source.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (kind, value, display, resolved) = row?;
                let target = match kind.as_str() {
                    "id" => WikiTarget::ById(model::parse_id(&value)?),
                    _ => WikiTarget::ByTitle(value),
                };
                let resolved = resolved.as_deref().map(model::parse_id).transpose()?;
                out.push(Link {
                    source,
                    target,
                    display,
                    resolved,
                });
            }
            Ok(out)
        })
    }

    /// Notes that link TO `target` (the reverse of the link graph), most-recently
    /// updated first. Based on the stored `resolved_id`, so a link counts once its
    /// source note has been written with the target resolved (index-backed via
    /// `idx_links_resolved`).
    pub fn backlinks(&self, target: NoteId) -> Result<Vec<Note>> {
        self.notes_for(
            "SELECT DISTINCT l.source_id FROM links l
             JOIN notes n ON n.id = l.source_id
             WHERE l.resolved_id = ?1
             ORDER BY n.updated DESC",
            params![target.to_string()],
        )
    }

    /// Resolve a wikilink `ByTitle` value to a unique `NoteId`: a unique git-style
    /// ULID prefix first (so `[[01KWC654QV]]` resolves like `note show`), else a
    /// unique exact effective-title match. No FTS fallback (links are
    /// deterministic). Public mirror of the writer's link resolution.
    pub fn resolve_link_target(&self, value: &str) -> Result<Option<NoteId>> {
        self.with(|conn| resolve_link_value(conn, value))
    }

    /// The note a link currently points to: its stored `resolved` id if present,
    /// otherwise a fresh resolution against the live DB. Lets a followed/listed
    /// link reflect a target that appeared after the source was last written
    /// (e.g. a short-id-prefix link stored before this resolver existed).
    pub fn resolve_link(&self, link: &Link) -> Result<Option<NoteId>> {
        if let Some(id) = link.resolved {
            return Ok(Some(id));
        }
        match &link.target {
            WikiTarget::ById(id) => Ok(self.get_note(*id)?.map(|n| n.id)),
            WikiTarget::ByTitle(value) => self.resolve_link_target(value),
        }
    }
}

/// Clamp a `usize` count/offset into SQLite's signed `i64` domain.
fn clamp_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// FTS candidates for `title`, rank-ordered, split into `(exact, all)`: `exact`
/// are notes whose effective title equals `title` case-insensitively; `all` is
/// the full rank-ordered candidate set. One scan, the single source of truth for
/// title matching, shared by `resolve_ref` (reader/pool) and `resolve_title_to_id`
/// (writer transaction) so they never diverge or double-scan. `ORDER BY rank`
/// keeps a unique exact-title note from being truncated out by the `LIMIT`.
fn title_candidates(conn: &Connection, title: &str) -> Result<(Vec<NoteId>, Vec<NoteId>)> {
    let needle = title.trim().to_lowercase();
    if needle.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title, n.body, n.content_kind FROM notes_fts f
         JOIN notes n ON n.rowid = f.rowid
         WHERE notes_fts MATCH ?1 ORDER BY rank LIMIT 50",
    )?;
    let rows = stmt.query_map(params![quote_fts(title.trim())], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    let mut all = Vec::new();
    let mut exact = Vec::new();
    for row in rows {
        let (id, title_col, body, kind) = row?;
        let nid = model::parse_id(&id)?;
        let effective = note_core::derive_title(
            title_col.as_deref(),
            &body,
            note_core::ContentKind::from_wire(&kind),
        );
        if effective.to_lowercase() == needle {
            exact.push(nid);
        }
        all.push(nid);
    }
    Ok((exact, all))
}

/// Resolve a wikilink title to a unique `NoteId` (case-insensitive). Returns
/// `None` when there is no match or the match is ambiguous (so a dangling link
/// stays dangling rather than resolving arbitrarily). Shared by the writer,
/// which passes its transaction (coerced to `&Connection`).
pub(crate) fn resolve_title_to_id(conn: &Connection, title: &str) -> Result<Option<NoteId>> {
    let (exact, _all) = title_candidates(conn, title)?;
    Ok(if exact.len() == 1 {
        Some(exact[0])
    } else {
        None
    })
}

/// Resolve a wikilink `ByTitle` token to a unique `NoteId`: a unique git-style
/// ULID prefix first (matching `note show`), else a unique exact effective-title
/// match. Never falls back to FTS — a link must resolve deterministically or stay
/// dangling. Shared by the writer (its transaction) and `ReaderPool::resolve_link`.
pub(crate) fn resolve_link_value(conn: &Connection, value: &str) -> Result<Option<NoteId>> {
    if let Some(id) = unique_id_prefix(conn, value)? {
        return Ok(Some(id));
    }
    resolve_title_to_id(conn, value)
}

/// The single note whose id starts with `value` (a git-style ULID prefix), or
/// `None` when `value` is not a prefix, matches nothing, or is ambiguous.
fn unique_id_prefix(conn: &Connection, value: &str) -> Result<Option<NoteId>> {
    let value = value.trim();
    if !is_ulid_prefix(value) {
        return Ok(None);
    }
    let pattern = format!("{}%", value.to_ascii_uppercase());
    let mut stmt = conn.prepare("SELECT id FROM notes WHERE id LIKE ?1 LIMIT 2")?;
    let ids = collect_ids(stmt.query(params![pattern])?)?;
    Ok(if ids.len() == 1 {
        Some(model::parse_id(&ids[0])?)
    } else {
        None
    })
}

/// Does `s` look like a git-style ULID prefix? (1..26 Crockford base32 chars.)
/// A full 26-char id is handled by exact parsing, not here.
fn is_ulid_prefix(s: &str) -> bool {
    let len = s.chars().count();
    (1..26).contains(&len)
        && s.chars().all(|c| {
            let c = c.to_ascii_uppercase();
            c.is_ascii_digit() || (c.is_ascii_uppercase() && !matches!(c, 'I' | 'L' | 'O' | 'U'))
        })
}

/// Wrap a raw user string as a single FTS5 phrase so arbitrary input can never
/// be misread as FTS query syntax (embedded quotes are doubled).
fn quote_fts(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Build an FTS5 prefix query from free user text: each whitespace term becomes
/// a quoted prefix phrase (`"term"*`), AND-ed together. Quotes and `*` inside a
/// term are stripped so the result is always valid FTS5 syntax. Returns an empty
/// string when there are no usable terms.
fn to_prefix_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|t| t.replace(['"', '*'], ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn collect_ids(mut rows: rusqlite::Rows<'_>) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    while let Some(row) = rows.next()? {
        ids.push(row.get::<_, String>(0)?);
    }
    Ok(ids)
}

/// Load many notes by id without an N+1: one notes query and one tags query per
/// chunk (chunked to stay under SQLite's bound-variable limit), assembled in the
/// requested id order.
fn load_all(conn: &Connection, ids: &[String]) -> Result<Vec<Note>> {
    const CHUNK: usize = 500;
    let mut by_id: HashMap<String, Note> = HashMap::with_capacity(ids.len());
    for chunk in ids.chunks(CHUNK) {
        load_chunk(conn, chunk, &mut by_id)?;
    }
    // preserve the caller's order; drop any id that no longer exists
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

fn load_chunk(conn: &Connection, ids: &[String], out: &mut HashMap<String, Note>) -> Result<()> {
    if ids.is_empty() {
        return Ok(()); // `IN ()` is invalid SQL; nothing to load
    }
    let placeholders = vec!["?"; ids.len()].join(",");

    let notes_sql = format!(
        "SELECT id, title, body, content_kind, created, updated FROM notes WHERE id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&notes_sql)?;
    let mut rows = stmt.query(params_from_iter(ids.iter()))?;
    while let Some(row) = rows.next()? {
        let note = model::note_from_row_no_tags(row)?;
        out.insert(note.id.to_string(), note);
    }

    let tags_sql = format!("SELECT note_id, tag FROM tags WHERE note_id IN ({placeholders})");
    let mut stmt = conn.prepare(&tags_sql)?;
    let mut rows = stmt.query(params_from_iter(ids.iter()))?;
    while let Some(row) = rows.next()? {
        let note_id: String = row.get(0)?;
        let raw: String = row.get(1)?;
        if let Some(note) = out.get_mut(&note_id) {
            note.tags.insert(model::parse_tag(&raw)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_ulid_prefix, to_prefix_query};

    #[test]
    fn prefix_query_makes_each_term_a_prefix() {
        assert_eq!(to_prefix_query("mensag"), "\"mensag\"*");
        assert_eq!(to_prefix_query("foo bar"), "\"foo\"* \"bar\"*");
    }

    #[test]
    fn prefix_query_ignores_empty_and_strips_fts_chars() {
        assert_eq!(to_prefix_query("   "), "");
        assert_eq!(to_prefix_query("a*b \"c\""), "\"ab\"* \"c\"*");
    }

    #[test]
    fn ulid_prefix_classification() {
        assert!(is_ulid_prefix("01ARZ"));
        assert!(!is_ulid_prefix("dog")); // 'o' is not Crockford base32
        assert!(!is_ulid_prefix("")); // empty
    }
}
