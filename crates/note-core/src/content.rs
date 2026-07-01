use core::fmt;
use serde::{Deserialize, Serialize};

/// Whether a note's body is interpreted as plain text or markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Plain,
    #[default]
    Markdown,
}

impl<'de> Deserialize<'de> for ContentKind {
    /// Decode through the shared, lenient wire vocabulary so JSON matches
    /// note-md's frontmatter parse: an unknown or future tag degrades to the
    /// [`Markdown`](Self::Markdown) default instead of hard-erroring (which the
    /// derived `rename_all` impl would do). Mirrors `NoteId`/`Tag`.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_wire(&s))
    }
}

impl ContentKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Markdown => "markdown",
        }
    }

    /// Lenient inverse of [`as_str`](Self::as_str) for stored / serialized
    /// values: `"plain"` is [`Plain`](Self::Plain); anything else (unknown or
    /// future tags) falls back to the [`Markdown`](Self::Markdown) default. This
    /// is the one home for the wire vocabulary — the store's DB-column decode and
    /// note-md's frontmatter parse both route through it instead of re-matching.
    #[must_use]
    pub fn from_wire(s: &str) -> Self {
        match s {
            "plain" => Self::Plain,
            _ => Self::Markdown,
        }
    }

    #[must_use]
    pub const fn is_markdown(self) -> bool {
        matches!(self, Self::Markdown)
    }
}

impl fmt::Display for ContentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contentkind_default_is_markdown() {
        assert_eq!(ContentKind::default(), ContentKind::Markdown);
    }

    #[test]
    fn contentkind_as_str_and_display() {
        assert_eq!(ContentKind::Plain.as_str(), "plain");
        assert_eq!(ContentKind::Markdown.as_str(), "markdown");
        assert_eq!(ContentKind::Plain.to_string(), "plain");
        assert_eq!(ContentKind::Markdown.to_string(), "markdown");
    }

    #[test]
    fn contentkind_from_wire_is_lenient_inverse() {
        assert_eq!(ContentKind::from_wire("plain"), ContentKind::Plain);
        assert_eq!(ContentKind::from_wire("markdown"), ContentKind::Markdown);
        assert_eq!(ContentKind::from_wire("???"), ContentKind::Markdown);
    }

    #[test]
    fn contentkind_is_markdown() {
        assert!(ContentKind::Markdown.is_markdown());
        assert!(!ContentKind::Plain.is_markdown());
    }

    #[test]
    fn contentkind_serde_lowercase_roundtrip() {
        assert_eq!(
            serde_json::to_string(&ContentKind::Plain).unwrap(),
            "\"plain\""
        );
        let back: ContentKind = serde_json::from_str("\"markdown\"").unwrap();
        assert_eq!(back, ContentKind::Markdown);
    }

    #[test]
    fn contentkind_deserialize_is_lenient_like_from_wire() {
        // Unknown/future tags must NOT hard-error on the serde path (they don't
        // on the note-md frontmatter path); both share `from_wire`.
        let plain: ContentKind = serde_json::from_str("\"plain\"").unwrap();
        assert_eq!(plain, ContentKind::Plain);
        let unknown: ContentKind = serde_json::from_str("\"future-kind\"").unwrap();
        assert_eq!(unknown, ContentKind::Markdown);
    }
}
