use crate::title::derive_title;
use crate::{ContentKind, NoteId, Tag, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A note. The BODY is the source of truth for wikilinks (no link field here);
/// `tags` are authoritative metadata. `created`/`updated` are explicit (not
/// derived from the id) so export->import preserves them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: NoteId,
    pub title: Option<String>,
    pub body: String,
    #[serde(default)]
    pub content_kind: ContentKind,
    #[serde(default)]
    pub tags: BTreeSet<Tag>,
    pub created: Timestamp,
    pub updated: Timestamp,
}

impl Note {
    /// All time/id values are INJECTED by the caller (edge), never read here.
    pub fn new(
        id: NoteId,
        body: impl Into<String>,
        content_kind: ContentKind,
        created: Timestamp,
        updated: Timestamp,
    ) -> Self {
        Self {
            id,
            title: None,
            body: body.into(),
            content_kind,
            tags: BTreeSet::new(),
            created,
            updated,
        }
    }

    #[must_use]
    pub fn effective_title(&self) -> String {
        derive_title(self.title.as_deref(), &self.body, self.content_kind)
    }

    /// The effective title, or `"(untitled)"` when empty. The canonical fallback
    /// for listing/displaying a note across the CLI and TUI.
    #[must_use]
    pub fn display_title(&self) -> String {
        let title = self.effective_title();
        if title.is_empty() {
            "(untitled)".to_owned()
        } else {
            title
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(body: &str, kind: ContentKind) -> Note {
        let id = NoteId::from_parts(1000, 7);
        Note::new(
            id,
            body,
            kind,
            Timestamp::from_unix_millis(1000),
            Timestamp::from_unix_millis(2000),
        )
    }

    #[test]
    fn note_effective_title_from_h1() {
        let n = sample("# Heading\nrest", ContentKind::Markdown);
        assert_eq!(n.effective_title(), "Heading");
    }

    #[test]
    fn note_effective_title_explicit_overrides() {
        let mut n = sample("# Heading", ContentKind::Markdown);
        n.title = Some("Explicit".to_string());
        assert_eq!(n.effective_title(), "Explicit");
    }

    #[test]
    fn note_json_roundtrip() {
        let mut n = sample("# Body", ContentKind::Markdown);
        n.tags.insert(Tag::new("Rust").unwrap());
        n.tags.insert(Tag::new("#rust").unwrap()); // collapses to one
        let json = serde_json::to_string(&n).unwrap();
        let back: Note = serde_json::from_str(&json).unwrap();
        assert_eq!(n, back);
        assert_eq!(back.tags.len(), 1);
    }

    #[test]
    fn note_serde_defaults() {
        let id = NoteId::from_parts(1, 1);
        let json = format!(r#"{{"id":"{id}","title":null,"body":"x","created":1,"updated":2}}"#);
        let n: Note = serde_json::from_str(&json).unwrap();
        assert_eq!(n.content_kind, ContentKind::Markdown);
        assert!(n.tags.is_empty());
    }
}
