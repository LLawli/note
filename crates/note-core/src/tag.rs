use crate::error::TagError;
use core::{fmt, str::FromStr};
use serde::{Deserialize, Deserializer, Serialize};

/// Canonical tag. FORM: trim; strip ONE leading '#'; trim; Unicode-lowercase;
/// every char must be Unicode-alphanumeric or '-' or '_' or '/'. The '/' enables
/// hierarchical tags (e.g. `projeto/note`, Obsidian-style). Empty after
/// normalization is an error. (No length cap in M0; per-segment validation —
/// rejecting empty segments like `a//b` or a leading/trailing '/' — is a
/// deferred refinement.)
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(into = "String")]
pub struct Tag(String);

impl Tag {
    pub fn new(raw: &str) -> Result<Self, TagError> {
        let s = raw.trim();
        let s = s.strip_prefix('#').unwrap_or(s).trim();
        if s.is_empty() {
            return Err(TagError::Empty);
        }
        let canon = s.to_lowercase();
        if let Some(ch) = canon.chars().find(|c| !Self::is_allowed(*c)) {
            return Err(TagError::InvalidChar { input: canon, ch });
        }
        Ok(Self(canon))
    }

    fn is_allowed(c: char) -> bool {
        c.is_alphanumeric() || matches!(c, '-' | '_' | '/')
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[inline]
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Tag {
    type Err = TagError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for Tag {
    type Error = TagError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}

impl From<Tag> for String {
    fn from(t: Tag) -> Self {
        t.0
    }
}

impl<'de> Deserialize<'de> for Tag {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Tag::new(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn tag_trims_and_lowercases() {
        assert_eq!(Tag::new("  Rust  ").unwrap().as_str(), "rust");
    }

    #[test]
    fn tag_strips_single_leading_hash() {
        assert_eq!(Tag::new("#rust").unwrap().as_str(), "rust");
    }

    #[test]
    fn tag_allows_dash_underscore() {
        assert_eq!(Tag::new("a_b-c").unwrap().as_str(), "a_b-c");
    }

    #[test]
    fn tag_allows_slash() {
        assert_eq!(Tag::new("Projeto/Note").unwrap().as_str(), "projeto/note");
    }

    #[test]
    fn tag_equality_after_normalization() {
        assert_eq!(Tag::new("Rust").unwrap(), Tag::new("  rust ").unwrap());
    }

    #[test]
    fn tag_ord_by_canonical_value() {
        assert!(Tag::new("Apple").unwrap() < Tag::new("zebra").unwrap());
    }

    #[test]
    fn tag_set_collapses_duplicates() {
        let set: BTreeSet<Tag> = ["Rust", "rust", " RUST ", "#rust"]
            .iter()
            .map(|s| Tag::new(s).unwrap())
            .collect();
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn tag_display_has_no_hash() {
        assert_eq!(Tag::new("#rust").unwrap().to_string(), "rust");
    }

    #[test]
    fn tag_rejects_empty() {
        assert_eq!(Tag::new("").unwrap_err(), TagError::Empty);
    }

    #[test]
    fn tag_rejects_whitespace_only() {
        assert_eq!(Tag::new("   ").unwrap_err(), TagError::Empty);
    }

    #[test]
    fn tag_rejects_only_hash() {
        assert_eq!(Tag::new("#").unwrap_err(), TagError::Empty);
    }

    #[test]
    fn tag_rejects_internal_whitespace() {
        assert!(matches!(
            Tag::new("two words").unwrap_err(),
            TagError::InvalidChar { ch: ' ', .. }
        ));
    }

    #[test]
    fn tag_rejects_punctuation() {
        for bad in ["bad!", "a,b", "a|b"] {
            assert!(matches!(
                Tag::new(bad).unwrap_err(),
                TagError::InvalidChar { .. }
            ));
        }
    }

    #[test]
    fn tag_serde_renormalizes_on_deserialize() {
        let t: Tag = serde_json::from_str("\"Rust\"").unwrap();
        assert_eq!(t.as_str(), "rust");
    }

    #[test]
    fn tag_serde_serializes_canonical() {
        assert_eq!(
            serde_json::to_string(&Tag::new("#Rust").unwrap()).unwrap(),
            "\"rust\""
        );
    }
}
