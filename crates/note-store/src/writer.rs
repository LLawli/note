//! The single-writer actor. ALL writes funnel through one `std::sync::mpsc`
//! channel to one dedicated OS thread that owns the read-write connection.
//! Every mutation runs in a transaction so the note row, its tags, its links
//! and the FTS index (via triggers) commit or roll back together.

use crate::error::{Result, StoreError};
use crate::mint;
use crate::model::{ImportNote, ImportOutcome, NewNote, NotePatch};
use note_core::{ContentKind, Note, NoteId, Tag, Timestamp, WikiLink, WikiTarget};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

type Reply<T> = Sender<Result<T>>;

enum WriteCmd {
    Create {
        input: NewNote,
        reply: Reply<Note>,
    },
    Update {
        id: NoteId,
        patch: NotePatch,
        reply: Reply<Option<Note>>,
    },
    Delete {
        id: NoteId,
        reply: Reply<bool>,
    },
    ReplaceLinks {
        source: NoteId,
        links: Vec<WikiLink>,
        reply: Reply<()>,
    },
    Import {
        req: Box<ImportNote>,
        reply: Reply<(Note, ImportOutcome)>,
    },
    Reindex {
        reply: Reply<usize>,
    },
}

/// Sync handle to the writer thread. Intentionally NOT `Clone`: the store holds
/// the single command sender, so `Store::drop` dropping it always ends the writer
/// loop and lets the join complete (a surviving clone would block join forever).
#[derive(Debug)]
pub struct WriterHandle {
    tx: Sender<WriteCmd>,
}

impl WriterHandle {
    /// Spawn the writer thread around an already-initialised connection
    /// (migrations + pragmas applied). Returns the handle and the join handle.
    pub(crate) fn spawn(conn: Connection) -> std::io::Result<(Self, JoinHandle<()>)> {
        let (tx, rx) = mpsc::channel();
        let join = std::thread::Builder::new()
            .name("note-writer".to_owned())
            .spawn(move || run(conn, &rx))?;
        Ok((Self { tx }, join))
    }

    fn dispatch<T>(&self, make: impl FnOnce(Reply<T>) -> WriteCmd) -> Result<T> {
        let (rtx, rrx) = mpsc::channel();
        self.tx
            .send(make(rtx))
            .map_err(|_| StoreError::WriterGone)?;
        rrx.recv().map_err(|_| StoreError::WriterGone)?
    }

    pub fn create_note(&self, input: NewNote) -> Result<Note> {
        self.dispatch(|reply| WriteCmd::Create { input, reply })
    }

    pub fn update_note(&self, id: NoteId, patch: NotePatch) -> Result<Option<Note>> {
        self.dispatch(|reply| WriteCmd::Update { id, patch, reply })
    }

    pub fn delete_note(&self, id: NoteId) -> Result<bool> {
        self.dispatch(|reply| WriteCmd::Delete { id, reply })
    }

    /// Replace the full set of outgoing links for `source`. Id targets resolve by
    /// existence; title targets resolve to a unique effective-title match.
    pub fn replace_links(&self, source: NoteId, links: Vec<WikiLink>) -> Result<()> {
        self.dispatch(|reply| WriteCmd::ReplaceLinks {
            source,
            links,
            reply,
        })
    }

    /// Import a note: insert it (minting id/timestamps if absent) or, when an id
    /// already exists, update it in place — preserving the supplied timestamps so
    /// re-importing the same export is idempotent.
    pub fn import_note(&self, req: ImportNote) -> Result<(Note, ImportOutcome)> {
        self.dispatch(|reply| WriteCmd::Import {
            req: Box::new(req),
            reply,
        })
    }

    /// Re-resolve every link's stored `resolved_id` against the current notes and
    /// return how many changed. Lets `[[wikilinks]]` written before their target
    /// existed (or before id-prefix resolution) resolve — and feed backlinks —
    /// without re-saving each source. A one-shot maintenance pass, not a per-read
    /// scan.
    pub fn reindex(&self) -> Result<usize> {
        self.dispatch(|reply| WriteCmd::Reindex { reply })
    }
}

fn run(mut conn: Connection, rx: &Receiver<WriteCmd>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            WriteCmd::Create { input, reply } => {
                let _ = reply.send(create(&mut conn, input));
            }
            WriteCmd::Update { id, patch, reply } => {
                let _ = reply.send(update(&mut conn, id, patch));
            }
            WriteCmd::Delete { id, reply } => {
                let _ = reply.send(delete(&mut conn, id));
            }
            WriteCmd::ReplaceLinks {
                source,
                links,
                reply,
            } => {
                let _ = reply.send(replace_links(&mut conn, source, &links));
            }
            WriteCmd::Import { req, reply } => {
                let _ = reply.send(import(&mut conn, *req));
            }
            WriteCmd::Reindex { reply } => {
                let _ = reply.send(reindex(&mut conn));
            }
        }
    }
}

/// Re-resolve every link's `resolved_id` against the current notes (id targets by
/// existence, title targets by unique ULID-prefix then unique title) and persist
/// it. Returns how many links changed. The whole pass runs in one transaction.
fn reindex(conn: &mut Connection) -> Result<usize> {
    let tx = conn.transaction()?;
    let links: Vec<(i64, String, String, Option<String>)> = {
        let mut stmt =
            tx.prepare("SELECT rowid, target_kind, target_value, resolved_id FROM links")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut changed = 0usize;
    {
        let mut upd = tx.prepare("UPDATE links SET resolved_id = ?2 WHERE rowid = ?1")?;
        for (rowid, kind, value, current) in &links {
            let resolved: Option<String> = if kind == "id" {
                note_exists(&tx, crate::model::parse_id(value)?)?.then(|| value.clone())
            } else {
                crate::reader::resolve_link_value(&tx, value)?.map(|id| id.to_string())
            };
            if &resolved != current {
                upd.execute(params![rowid, resolved])?;
                changed += 1;
            }
        }
    }
    tx.commit()?;
    Ok(changed)
}

fn create(conn: &mut Connection, input: NewNote) -> Result<Note> {
    ensure_not_empty(input.title.as_deref(), &input.body, input.content_kind)?;
    let now = mint::now();
    let note = Note {
        id: mint::new_id(),
        title: input.title,
        body: input.body,
        content_kind: input.content_kind,
        tags: input.tags,
        created: now,
        updated: now,
    };
    let tx = conn.transaction()?;
    insert_note(&tx, &note)?;
    write_links(&tx, note.id, &input.links)?;
    tx.commit()?;
    Ok(note)
}

fn import(conn: &mut Connection, req: ImportNote) -> Result<(Note, ImportOutcome)> {
    ensure_not_empty(req.title.as_deref(), &req.body, req.content_kind)?;
    let created = req.created.unwrap_or_else(mint::now);
    let note = Note {
        id: req.id.unwrap_or_else(mint::new_id),
        title: req.title,
        body: req.body,
        content_kind: req.content_kind,
        tags: req.tags,
        created,
        updated: req.updated.unwrap_or(created),
    };
    let tx = conn.transaction()?;
    let existed = note_exists(&tx, note.id)?;
    // Upsert on the ULID; the FTS triggers keep the index synced for both the
    // insert and the on-conflict update path.
    tx.execute(
        "INSERT INTO notes (id, title, body, content_kind, created, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
            title = excluded.title, body = excluded.body,
            content_kind = excluded.content_kind,
            created = excluded.created, updated = excluded.updated",
        params![
            note.id.to_string(),
            note.title,
            note.body,
            note.content_kind.as_str(),
            note.created.as_unix_millis(),
            note.updated.as_unix_millis(),
        ],
    )?;
    replace_tags(&tx, note.id, note.tags.iter())?;
    write_links(&tx, note.id, &req.links)?;
    tx.commit()?;
    let outcome = if existed {
        ImportOutcome::Updated
    } else {
        ImportOutcome::Created
    };
    Ok((note, outcome))
}

fn update(conn: &mut Connection, id: NoteId, patch: NotePatch) -> Result<Option<Note>> {
    ensure_not_empty(patch.title.as_deref(), &patch.body, patch.content_kind)?;
    let tx = conn.transaction()?;
    // `.optional()?` maps only no-rows to None; a real DB error propagates
    // instead of masquerading as "note not found" (which would silently drop
    // the user's edit).
    let created: Option<i64> = tx
        .query_row(
            "SELECT created FROM notes WHERE id = ?1",
            params![id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let Some(created) = created else {
        return Ok(None);
    };
    let now = mint::now();
    tx.execute(
        "UPDATE notes SET title = ?2, body = ?3, content_kind = ?4, updated = ?5 WHERE id = ?1",
        params![
            id.to_string(),
            patch.title,
            patch.body,
            patch.content_kind.as_str(),
            now.as_unix_millis(),
        ],
    )?;
    replace_tags(&tx, id, patch.tags.iter())?;
    write_links(&tx, id, &patch.links)?;
    tx.commit()?;
    Ok(Some(Note {
        id,
        title: patch.title,
        body: patch.body,
        content_kind: patch.content_kind,
        tags: patch.tags,
        created: Timestamp::from_unix_millis(created),
        updated: now,
    }))
}

fn delete(conn: &mut Connection, id: NoteId) -> Result<bool> {
    let tx = conn.transaction()?;
    let id = id.to_string();
    // ON DELETE CASCADE clears this note's OWN tags/outgoing-links; the AFTER
    // DELETE trigger clears its FTS row. But inbound links from OTHER notes keep
    // a stale `resolved_id` (bare TEXT, no FK) pointing at the removed id, so
    // null them here — otherwise `note links` lists a dead short-id until the
    // next reindex. (ULIDs are never reused, so a stale id can't mis-resolve.)
    tx.execute(
        "UPDATE links SET resolved_id = NULL WHERE resolved_id = ?1",
        params![id],
    )?;
    let n = tx.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    tx.commit()?;
    Ok(n > 0)
}

/// Reject a create/update/import that would persist a note with neither a title
/// nor any body content (invariant 5: this domain rule lives in the store, not
/// only in the CLI front-end, so every writer path is covered). Empty ⟺ the
/// effective title derives to nothing (no explicit title, no non-blank body).
fn ensure_not_empty(title: Option<&str>, body: &str, kind: ContentKind) -> Result<()> {
    if note_core::derive_title(title, body, kind).trim().is_empty() {
        return Err(StoreError::EmptyNote);
    }
    Ok(())
}

fn replace_links(conn: &mut Connection, source: NoteId, links: &[WikiLink]) -> Result<()> {
    let tx = conn.transaction()?;
    write_links(&tx, source, links)?;
    tx.commit()?;
    Ok(())
}

/// Replace a note's outgoing links within an existing transaction, so the link
/// graph commits atomically with the note row (invariant: indexes never lag the
/// data). Id targets resolve by existence; title targets by unique ULID-prefix
/// then unique effective-title match (so `[[01KWC654QV]]` resolves like `show`).
fn write_links(tx: &Transaction<'_>, source: NoteId, links: &[WikiLink]) -> Result<()> {
    let source = source.to_string();
    tx.execute("DELETE FROM links WHERE source_id = ?1", params![source])?;
    // Prepare the INSERT once and reuse the formatted source id across all links,
    // rather than re-parsing the SQL and re-allocating the ULID per iteration.
    let mut stmt = tx.prepare(
        "INSERT INTO links (source_id, target_kind, target_value, display, resolved_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for link in links {
        let (kind, value, resolved) = match &link.target {
            WikiTarget::ById(id) => {
                let exists = note_exists(tx, *id)?;
                ("id", id.to_string(), exists.then(|| id.to_string()))
            }
            WikiTarget::ByTitle(t) => {
                let resolved = crate::reader::resolve_link_value(tx, t)?;
                ("title", t.clone(), resolved.map(|id| id.to_string()))
            }
        };
        stmt.execute(params![source, kind, value, link.display, resolved])?;
    }
    Ok(())
}

/// Does a note with this id exist? Shared by import's upsert outcome and the link
/// writer's id-target resolution.
fn note_exists(tx: &Transaction<'_>, id: NoteId) -> Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1)",
        params![id.to_string()],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

fn insert_note(tx: &Transaction<'_>, note: &Note) -> Result<()> {
    tx.execute(
        "INSERT INTO notes (id, title, body, content_kind, created, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            note.id.to_string(),
            note.title,
            note.body,
            note.content_kind.as_str(),
            note.created.as_unix_millis(),
            note.updated.as_unix_millis(),
        ],
    )?;
    insert_tags(tx, note.id, note.tags.iter())?;
    Ok(())
}

fn insert_tags<'a>(
    tx: &Transaction<'_>,
    id: NoteId,
    tags: impl Iterator<Item = &'a Tag>,
) -> Result<()> {
    let id = id.to_string();
    let mut stmt = tx.prepare("INSERT INTO tags (note_id, tag) VALUES (?1, ?2)")?;
    for tag in tags {
        stmt.execute(params![id, tag.as_str()])?;
    }
    Ok(())
}

/// Clear and re-insert a note's tags (for update/import paths).
fn replace_tags<'a>(
    tx: &Transaction<'_>,
    id: NoteId,
    tags: impl Iterator<Item = &'a Tag>,
) -> Result<()> {
    tx.execute(
        "DELETE FROM tags WHERE note_id = ?1",
        params![id.to_string()],
    )?;
    insert_tags(tx, id, tags)
}
