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
use std::borrow::Cow;
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
        // A newline in a scalar would either break re-parsing (a continuation
        // line has no `key:`) or, worse, an injected `\n---\n` would silently
        // truncate the frontmatter. Collapse interior newlines so the block
        // always round-trips (invariant 8).
        let _ = writeln!(out, "title: {}", frontmatter_scalar(title));
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
    // Frontmatter detection keys off LF fences; a CRLF (or lone-CR) file would
    // otherwise read as "no frontmatter" and lose id/tags/timestamps.
    let text = normalize_newlines(text);
    let (frontmatter, body) = split_frontmatter(&text);
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
            "content_kind" => note.content_kind = ContentKind::from_wire(value),
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
/// `(None, full_text)` when there is no complete frontmatter block, or when the
/// leading `---…---` block is prose (a thematic break / Setext layout) rather
/// than `key: value` lines — so arbitrary markdown that happens to open with a
/// horizontal rule imports as body instead of erroring.
fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return (None, text);
    };
    // Empty frontmatter block: "---\n---\n<body>" or a bare "---\n---".
    if let Some(body) = rest.strip_prefix("---\n") {
        return (Some(""), body);
    }
    if rest == "---" {
        return (Some(""), "");
    }
    if let Some(end) = rest.find("\n---\n") {
        let fm = &rest[..end];
        if looks_like_frontmatter(fm) {
            return (Some(fm), &rest[end + 5..]);
        }
        return (None, text);
    }
    if let Some(stripped) = rest.strip_suffix("\n---") {
        if looks_like_frontmatter(stripped) {
            return (Some(stripped), "");
        }
        return (None, text);
    }
    (None, text)
}

/// A candidate frontmatter block is real only if every non-blank line is a
/// `key: value` pair; a colon-less prose line means it was two horizontal rules.
fn looks_like_frontmatter(block: &str) -> bool {
    block
        .lines()
        .filter(|l| !l.trim().is_empty())
        .all(|l| l.contains(':'))
}

/// Normalize CRLF and lone-CR line endings to LF (borrow when already clean).
fn normalize_newlines(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// Flatten interior newlines in a frontmatter scalar to spaces so the emitted
/// `key: value` line stays single-line and re-parses losslessly.
fn frontmatter_scalar(value: &str) -> Cow<'_, str> {
    if value.contains('\n') || value.contains('\r') {
        Cow::Owned(value.replace(['\r', '\n'], " "))
    } else {
        Cow::Borrowed(value)
    }
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
    fn crlf_frontmatter_is_parsed_not_swallowed() {
        let md = "---\r\ntitle: T\r\ntags: a, b\r\ncontent_kind: plain\r\ncreated: 5\r\nupdated: 9\r\n---\r\nbody\r\nline";
        let parsed = from_markdown(md).unwrap();
        assert_eq!(parsed.title.as_deref(), Some("T"));
        assert_eq!(parsed.tags.len(), 2);
        assert_eq!(parsed.content_kind, ContentKind::Plain);
        assert_eq!(parsed.created, Some(Timestamp::from_unix_millis(5)));
        assert_eq!(parsed.updated, Some(Timestamp::from_unix_millis(9)));
        assert_eq!(parsed.body, "body\nline");
    }

    #[test]
    fn title_with_newline_roundtrips_and_keeps_metadata() {
        let id = NoteId::from_parts(7, 7);
        let mut note = Note::new(
            id,
            "body",
            ContentKind::Plain,
            Timestamp::from_unix_millis(11),
            Timestamp::from_unix_millis(22),
        );
        note.title = Some("line one\nline two".to_owned());
        let parsed = from_markdown(&to_markdown(&note)).unwrap();
        assert_eq!(parsed.title.as_deref(), Some("line one line two"));
        assert_eq!(parsed.content_kind, ContentKind::Plain);
        assert_eq!(parsed.created, Some(Timestamp::from_unix_millis(11)));
        assert_eq!(parsed.updated, Some(Timestamp::from_unix_millis(22)));
    }

    #[test]
    fn title_with_divider_does_not_truncate_frontmatter() {
        let id = NoteId::from_parts(8, 8);
        let mut note = Note::new(
            id,
            "body",
            ContentKind::Plain,
            Timestamp::from_unix_millis(1),
            Timestamp::from_unix_millis(2),
        );
        note.title = Some("a\n---".to_owned());
        let parsed = from_markdown(&to_markdown(&note)).unwrap();
        assert_eq!(parsed.title.as_deref(), Some("a ---"));
        assert_eq!(parsed.created, Some(Timestamp::from_unix_millis(1)));
        assert_eq!(parsed.updated, Some(Timestamp::from_unix_millis(2)));
        assert_eq!(parsed.body, "body");
    }

    #[test]
    fn leading_rule_block_of_prose_is_body_not_frontmatter() {
        let text = "---\nprose with no colon\n---\nmore body";
        let parsed = from_markdown(text).unwrap();
        assert_eq!(parsed.id, None);
        assert_eq!(parsed.body, text);
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
