//! Convert notes to/from on-disk markdown (with frontmatter) and JSON.
//!
//! Markdown form:
//! ```text
//! ---
//! id: 01ARZ3NDEKTSV4RRFFQ69G5FAV
//! title: Some Title
//! tags: cli, rust
//! content_kind: markdown
//! created: 1469922850259
//! updated: 1469922850259
//! ---
//! <body>
//! ```
//! `title`/`tags` are omitted when absent/empty. JSON form is the serialized
//! [`Note`]. Both round-trip: `to_* ∘ from_*` reproduces the same logical note.

use note_core::{ContentKind, Note, NoteId, Tag, Timestamp};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::str::FromStr;
use thiserror::Error;

/// Failure converting interchange formats.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MdError {
    #[error("malformed frontmatter: {0}")]
    Frontmatter(String),
    #[error("invalid note id in frontmatter: {0}")]
    Id(#[from] note_core::IdError),
    #[error("invalid tag in frontmatter: {0}")]
    Tag(#[from] note_core::TagError),
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),
}

/// A note parsed from a `.md` file. `id`/`created`/`updated` are optional so
/// externally-authored markdown (no frontmatter) can be imported; the storage
/// edge mints whatever is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedNote {
    pub id: Option<NoteId>,
    pub title: Option<String>,
    pub body: String,
    pub content_kind: ContentKind,
    pub tags: BTreeSet<Tag>,
    pub created: Option<Timestamp>,
    pub updated: Option<Timestamp>,
}

/// Render a note as markdown with a frontmatter header.
#[must_use]
pub fn to_markdown(note: &Note) -> String {
    let mut out = String::from("---\n");
    let _ = writeln!(out, "id: {}", note.id);
    if let Some(title) = &note.title {
        let _ = writeln!(out, "title: {title}");
    }
    if !note.tags.is_empty() {
        let tags = note
            .tags
            .iter()
            .map(Tag::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "tags: {tags}");
    }
    let _ = writeln!(out, "content_kind: {}", note.content_kind.as_str());
    let _ = writeln!(out, "created: {}", note.created.as_unix_millis());
    let _ = writeln!(out, "updated: {}", note.updated.as_unix_millis());
    out.push_str("---\n");
    out.push_str(&note.body);
    out
}

/// Parse a `.md` file (optionally with frontmatter) into an [`ImportedNote`].
pub fn from_markdown(text: &str) -> Result<ImportedNote, MdError> {
    let (frontmatter, body) = split_frontmatter(text);
    let mut note = ImportedNote {
        id: None,
        title: None,
        body: body.to_owned(),
        content_kind: ContentKind::Markdown,
        tags: BTreeSet::new(),
        created: None,
        updated: None,
    };

    let Some(frontmatter) = frontmatter else {
        return Ok(note);
    };
    for line in frontmatter.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| MdError::Frontmatter(format!("expected `key: value`, got {line:?}")))?;
        let value = value.trim();
        match key.trim() {
            "id" => note.id = Some(NoteId::from_str(value)?),
            "title" => note.title = (!value.is_empty()).then(|| value.to_owned()),
            "tags" => {
                for tag in value.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                    note.tags.insert(Tag::new(tag)?);
                }
            }
            "content_kind" => {
                note.content_kind = match value {
                    "plain" => ContentKind::Plain,
                    _ => ContentKind::Markdown,
                };
            }
            "created" => note.created = Some(parse_ts(value, "created")?),
            "updated" => note.updated = Some(parse_ts(value, "updated")?),
            _ => {} // ignore unknown keys
        }
    }
    Ok(note)
}

/// Serialize a note to pretty JSON.
pub fn to_json(note: &Note) -> Result<String, MdError> {
    Ok(serde_json::to_string_pretty(note)?)
}

/// Parse a note from JSON (a full [`Note`], as produced by [`to_json`]).
pub fn from_json(text: &str) -> Result<Note, MdError> {
    Ok(serde_json::from_str(text)?)
}

fn parse_ts(value: &str, field: &str) -> Result<Timestamp, MdError> {
    value
        .parse::<i64>()
        .map(Timestamp::from_unix_millis)
        .map_err(|_| MdError::Frontmatter(format!("{field} must be an integer, got {value:?}")))
}

/// Split leading `---\n … \n---\n` frontmatter from the body. Returns
/// `(None, full_text)` when there is no complete frontmatter block.
fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    if let Some(rest) = text.strip_prefix("---\n") {
        // Empty frontmatter block: "---\n---\n<body>" or a bare "---\n---".
        if let Some(body) = rest.strip_prefix("---\n") {
            return (Some(""), body);
        }
        if rest == "---" {
            return (Some(""), "");
        }
        if let Some(end) = rest.find("\n---\n") {
            return (Some(&rest[..end]), &rest[end + 5..]);
        }
        if let Some(stripped) = rest.strip_suffix("\n---") {
            return (Some(stripped), "");
        }
    }
    (None, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Note {
        let id = NoteId::from_parts(1_469_922_850_259, 7);
        let mut note = Note::new(
            id,
            "# Heading\nbody with [[Link]]",
            ContentKind::Markdown,
            Timestamp::from_unix_millis(1000),
            Timestamp::from_unix_millis(2000),
        );
        note.title = Some("Explicit Title".to_owned());
        note.tags.insert(Tag::new("rust").unwrap());
        note.tags.insert(Tag::new("cli").unwrap());
        note
    }

    #[test]
    fn markdown_roundtrip_preserves_fields() {
        let note = sample();
        let md = to_markdown(&note);
        let parsed = from_markdown(&md).unwrap();
        assert_eq!(parsed.id, Some(note.id));
        assert_eq!(parsed.title, note.title);
        assert_eq!(parsed.body, note.body);
        assert_eq!(parsed.content_kind, note.content_kind);
        assert_eq!(parsed.tags, note.tags);
        assert_eq!(parsed.created, Some(note.created));
        assert_eq!(parsed.updated, Some(note.updated));
    }

    #[test]
    fn markdown_without_frontmatter_is_all_body() {
        let parsed = from_markdown("just a plain body\nwith two lines").unwrap();
        assert_eq!(parsed.id, None);
        assert_eq!(parsed.created, None);
        assert_eq!(parsed.body, "just a plain body\nwith two lines");
        assert!(parsed.tags.is_empty());
    }

    #[test]
    fn markdown_omits_absent_title_and_tags() {
        let id = NoteId::from_parts(1, 1);
        let note = Note::new(
            id,
            "body",
            ContentKind::Plain,
            Timestamp::UNIX_EPOCH,
            Timestamp::UNIX_EPOCH,
        );
        let md = to_markdown(&note);
        assert!(!md.contains("title:"));
        assert!(!md.contains("tags:"));
        assert!(md.contains("content_kind: plain"));
    }

    #[test]
    fn markdown_rejects_bad_id() {
        let err = from_markdown("---\nid: not-a-ulid\n---\nbody").unwrap_err();
        assert!(matches!(err, MdError::Id(_)));
    }

    #[test]
    fn empty_frontmatter_block_is_stripped() {
        let parsed = from_markdown("---\n---\nbody here").unwrap();
        assert_eq!(parsed.id, None);
        assert_eq!(parsed.body, "body here");
    }

    #[test]
    fn json_roundtrip() {
        let note = sample();
        let json = to_json(&note).unwrap();
        let back = from_json(&json).unwrap();
        assert_eq!(note, back);
    }

    #[test]
    fn body_with_dividers_survives_roundtrip() {
        let id = NoteId::from_parts(5, 5);
        let note = Note::new(
            id,
            "intro\n\n---\n\nsection after a divider",
            ContentKind::Markdown,
            Timestamp::from_unix_millis(1),
            Timestamp::from_unix_millis(1),
        );
        let parsed = from_markdown(&to_markdown(&note)).unwrap();
        assert_eq!(parsed.body, note.body);
    }
}
