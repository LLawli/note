//! Input/output shapes for store operations and row<->domain mapping.

use crate::error::{Result, StoreError};
use core::fmt;
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

impl fmt::Display for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Render as the canonical wikilink body (target plus optional `|alias`),
        // delegating to note-core's WikiLink Display so the two never diverge.
        let link = WikiLink {
            target: self.target.clone(),
            display: self.display.clone(),
        };
        write!(f, "{link}")
    }
}

pub(crate) fn parse_id(s: &str) -> Result<NoteId> {
    NoteId::from_str(s).map_err(|e| StoreError::Corrupt(format!("note id {s:?}: {e}")))
}

pub(crate) fn parse_tag(s: &str) -> Result<Tag> {
    Tag::from_str(s).map_err(|e| StoreError::Corrupt(format!("tag {s:?}: {e}")))
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
        content_kind: ContentKind::from_wire(&kind_str),
        tags: BTreeSet::new(),
        created: Timestamp::from_unix_millis(row.get("created")?),
        updated: Timestamp::from_unix_millis(row.get("updated")?),
    })
}
