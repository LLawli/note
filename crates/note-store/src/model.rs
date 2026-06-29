//! Input/output shapes for store operations and row<->domain mapping.

use crate::error::{Result, StoreError};
use note_core::{ContentKind, Note, NoteId, Tag, Timestamp, WikiLink, WikiTarget};
use rusqlite::Row;
use std::collections::BTreeSet;
use std::str::FromStr;

/// Fields supplied by the caller to create a note. The store mints the `id` and
/// stamps `created`/`updated`. `links` are the note's outgoing `[[wikilinks]]`
/// (extracted at the edge), committed in the same transaction as the note.
#[derive(Debug, Clone, Default)]
pub struct NewNote {
    pub title: Option<String>,
    pub body: String,
    pub content_kind: ContentKind,
    pub tags: BTreeSet<Tag>,
    pub links: Vec<WikiLink>,
}

/// A full replacement of a note's mutable fields. The store bumps `updated` and
/// replaces the note's outgoing links in the same transaction.
#[derive(Debug, Clone, Default)]
pub struct NotePatch {
    pub title: Option<String>,
    pub body: String,
    pub content_kind: ContentKind,
    pub tags: BTreeSet<Tag>,
    pub links: Vec<WikiLink>,
}

/// A note to import: `id`/`created`/`updated` are optional and minted by the
/// store when absent (externally-authored markdown has none).
#[derive(Debug, Clone, Default)]
pub struct ImportNote {
    pub id: Option<NoteId>,
    pub title: Option<String>,
    pub body: String,
    pub content_kind: ContentKind,
    pub tags: BTreeSet<Tag>,
    pub created: Option<Timestamp>,
    pub updated: Option<Timestamp>,
    pub links: Vec<WikiLink>,
}

/// Whether an import created a new note or updated an existing one (same id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    Created,
    Updated,
}

/// A stored link from one note to a (possibly dangling) target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub source: NoteId,
    pub target: WikiTarget,
    pub display: Option<String>,
    /// The note this link resolves to, or `None` when dangling.
    pub resolved: Option<NoteId>,
}

/// Parse a content-kind string as stored in the DB. Unknown values fall back to
/// markdown (the default). A private helper because `ContentKind::FromStr` is a
/// later (M2) public concern.
pub(crate) fn content_kind_from_db(s: &str) -> ContentKind {
    match s {
        "plain" => ContentKind::Plain,
        _ => ContentKind::Markdown,
    }
}

pub(crate) fn parse_id(s: &str) -> Result<NoteId> {
    NoteId::from_str(s).map_err(|e| StoreError::Corrupt(format!("note id {s:?}: {e}")))
}

/// Map a `notes` row (id, title, body, content_kind, created, updated) to a
/// `Note` with the given tags.
pub(crate) fn note_from_row(row: &Row<'_>, tags: BTreeSet<Tag>) -> Result<Note> {
    let mut note = note_from_row_no_tags(row)?;
    note.tags = tags;
    Ok(note)
}

/// Map a `notes` row to a `Note` with an empty tag set (the caller attaches tags,
/// e.g. after a batched tags query).
pub(crate) fn note_from_row_no_tags(row: &Row<'_>) -> Result<Note> {
    let id_str: String = row.get("id")?;
    let kind_str: String = row.get("content_kind")?;
    Ok(Note {
        id: parse_id(&id_str)?,
        title: row.get("title")?,
        body: row.get("body")?,
        content_kind: content_kind_from_db(&kind_str),
        tags: BTreeSet::new(),
        created: Timestamp::from_unix_millis(row.get("created")?),
        updated: Timestamp::from_unix_millis(row.get("updated")?),
    })
}
