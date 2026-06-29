use crate::error::WikiError;
use crate::id::NoteId;
use core::{fmt, str::FromStr};
use serde::{Deserialize, Serialize};

/// Target of a `[[wikilink]]`: a stable id OR a (possibly dangling) title.
/// A token is `ById` iff it parses as a canonical ULID; otherwise `ByTitle`.
/// Resolving `ByTitle -> NoteId` is a later milestone (note-store, M1+).
/// CANONICAL FORM: a `ByTitle` produced via `FromStr` is always trimmed,
/// non-empty, and pipe-free; Display<->FromStr round-trips for such values.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WikiTarget {
    ById(NoteId),
    ByTitle(String),
}

impl WikiTarget {
    fn classify(token: &str) -> Self {
        match token.parse::<NoteId>() {
            Ok(id) => Self::ById(id),
            Err(_) => Self::ByTitle(token.to_string()),
        }
    }
}

impl fmt::Display for WikiTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ById(id) => write!(f, "{id}"),
            Self::ByTitle(t) => f.write_str(t),
        }
    }
}

impl FromStr for WikiTarget {
    type Err = WikiError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        if t.is_empty() {
            return Err(WikiError::EmptyTarget);
        }
        Ok(Self::classify(t))
    }
}

/// A parsed wikilink occurrence. PIPE SEMANTICS (Model X, Obsidian-style): text
/// LEFT of `|` is the target, text RIGHT of `|` is the display/alias. `FromStr`
/// accepts the inner content and tolerates surrounding `[[ ]]`; an empty display
/// becomes `None`. (Scanning `[[...]]` spans out of a markdown body is note-md/M3.)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WikiLink {
    pub target: WikiTarget,
    pub display: Option<String>,
}

impl FromStr for WikiLink {
    type Err = WikiError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let inner = s.trim();
        let inner = inner
            .strip_prefix("[[")
            .and_then(|x| x.strip_suffix("]]"))
            .unwrap_or(inner);
        let (target_str, display) = match inner.split_once('|') {
            Some((t, d)) => {
                let d = d.trim();
                (
                    t,
                    if d.is_empty() {
                        None
                    } else {
                        Some(d.to_string())
                    },
                )
            }
            None => (inner, None),
        };
        Ok(Self {
            target: target_str.parse()?,
            display,
        })
    }
}

impl fmt::Display for WikiLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.display {
            Some(d) => write!(f, "{}|{}", self.target, d),
            None => write!(f, "{}", self.target),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANONICAL: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    #[test]
    fn wikitarget_id_form() {
        let t: WikiTarget = CANONICAL.parse().unwrap();
        assert!(matches!(t, WikiTarget::ById(_)));
    }

    #[test]
    fn wikitarget_title_form() {
        let t: WikiTarget = "Some Title".parse().unwrap();
        assert_eq!(t, WikiTarget::ByTitle("Some Title".to_string()));
    }

    #[test]
    fn wikitarget_idish_but_invalid_is_title() {
        let t: WikiTarget = "01ARZ".parse().unwrap();
        assert_eq!(t, WikiTarget::ByTitle("01ARZ".to_string()));
    }

    #[test]
    fn wikitarget_overflow_is_title() {
        let t: WikiTarget = "80000000000000000000000000".parse().unwrap();
        assert_eq!(
            t,
            WikiTarget::ByTitle("80000000000000000000000000".to_string())
        );
    }

    #[test]
    fn wikitarget_rejects_empty() {
        assert_eq!(
            "".parse::<WikiTarget>().unwrap_err(),
            WikiError::EmptyTarget
        );
        assert_eq!(
            "   ".parse::<WikiTarget>().unwrap_err(),
            WikiError::EmptyTarget
        );
    }

    #[test]
    fn wikitarget_display_roundtrip() {
        for s in [CANONICAL, "Some Title"] {
            let t: WikiTarget = s.parse().unwrap();
            assert_eq!(t.to_string().parse::<WikiTarget>().unwrap(), t);
        }
    }

    #[test]
    fn wikilink_title_only() {
        let l: WikiLink = "Some Title".parse().unwrap();
        assert_eq!(l.target, WikiTarget::ByTitle("Some Title".to_string()));
        assert!(l.display.is_none());
    }

    #[test]
    fn wikilink_id_only() {
        let l: WikiLink = CANONICAL.parse().unwrap();
        assert!(matches!(l.target, WikiTarget::ById(_)));
        assert!(l.display.is_none());
    }

    #[test]
    fn wikilink_target_pipe_display() {
        let l: WikiLink = "Some Title|click here".parse().unwrap();
        assert_eq!(l.target, WikiTarget::ByTitle("Some Title".to_string()));
        assert_eq!(l.display.as_deref(), Some("click here"));
        assert_eq!(l.to_string().parse::<WikiLink>().unwrap(), l);
    }

    #[test]
    fn wikilink_id_pipe_display() {
        let l: WikiLink = format!("{CANONICAL}|Display Text").parse().unwrap();
        assert!(matches!(l.target, WikiTarget::ById(_)));
        assert_eq!(l.display.as_deref(), Some("Display Text"));
        assert_eq!(l.to_string().parse::<WikiLink>().unwrap(), l);
    }

    #[test]
    fn wikilink_strips_outer_brackets() {
        let l: WikiLink = "[[Plain]]".parse().unwrap();
        assert_eq!(l.target, WikiTarget::ByTitle("Plain".to_string()));
        assert!(l.display.is_none());
    }

    #[test]
    fn wikilink_empty_display_is_none() {
        let l: WikiLink = "Plain|".parse().unwrap();
        assert!(l.display.is_none());
    }

    #[test]
    fn wikilink_empty_target_errors() {
        assert_eq!(
            "|Display".parse::<WikiLink>().unwrap_err(),
            WikiError::EmptyTarget
        );
        assert_eq!("|".parse::<WikiLink>().unwrap_err(), WikiError::EmptyTarget);
    }
}
