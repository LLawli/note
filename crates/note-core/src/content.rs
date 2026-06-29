use core::fmt;
use serde::{Deserialize, Serialize};

/// Whether a note's body is interpreted as plain text or markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Plain,
    #[default]
    Markdown,
}

impl ContentKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Markdown => "markdown",
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
}
