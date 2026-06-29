//! The single-writer actor. ALL writes funnel through one `std::sync::mpsc`
//! channel to one dedicated OS thread that owns the read-write connection.
//! Every mutation runs in a transaction so the note row, its tags, its links
//! and the FTS index (via triggers) commit or roll back together.

use crate::error::{Result, StoreError};
use crate::mint;
use crate::model::{ImportNote, ImportOutcome, NewNote, NotePatch};
use note_core::{Note, NoteId, Tag, Timestamp, WikiLink, WikiTarget};
use rusqlite::{Connection, Transaction, params};
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
        }
    }
}

fn create(conn: &mut Connection, input: NewNote) -> Result<Note> {
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
    let tx = conn.transaction()?;
    let created: Option<i64> = tx
        .query_row(
            "SELECT created FROM notes WHERE id = ?1",
            params![id.to_string()],
            |r| r.get(0),
        )
        .ok();
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
    // ON DELETE CASCADE clears tags/links; the AFTER DELETE trigger clears FTS.
    let n = tx.execute("DELETE FROM notes WHERE id = ?1", params![id.to_string()])?;
    tx.commit()?;
    Ok(n > 0)
}

fn replace_links(conn: &mut Connection, source: NoteId, links: &[WikiLink]) -> Result<()> {
    let tx = conn.transaction()?;
    write_links(&tx, source, links)?;
    tx.commit()?;
    Ok(())
}

/// Replace a note's outgoing links within an existing transaction, so the link
/// graph commits atomically with the note row (invariant: indexes never lag the
/// data). Id targets resolve by existence; title targets by unique
/// effective-title match.
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
                let resolved = crate::reader::resolve_title_to_id(tx, t)?;
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
