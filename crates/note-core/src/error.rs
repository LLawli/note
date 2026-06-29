use thiserror::Error;

/// Crate-wide result alias; `E` defaults to [`Error`].
pub type Result<T, E = Error> = core::result::Result<T, E>;

/// Failure parsing a ULID-backed id (26-char Crockford base32).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum IdError {
    #[error("note id has invalid length (expected 26 Crockford base32 chars)")]
    InvalidLength,
    #[error("note id contains an invalid character")]
    InvalidChar,
    /// 26 valid chars but the value overflows 128 bits, so ulid silently masks it
    /// to a different canonical string; rejected to keep parse<->display total.
    #[error("note id is not in canonical form (value out of range)")]
    NonCanonical,
}

impl From<ulid::DecodeError> for IdError {
    // Intentionally exhaustive against ulid::DecodeError (NOT #[non_exhaustive] in
    // 1.2.x): a future added variant must break this match and force review.
    fn from(e: ulid::DecodeError) -> Self {
        match e {
            ulid::DecodeError::InvalidLength => Self::InvalidLength,
            ulid::DecodeError::InvalidChar => Self::InvalidChar,
        }
    }
}

/// Failure normalizing/validating a [`crate::Tag`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TagError {
    #[error("tag is empty after normalization")]
    Empty,
    #[error("tag {input:?} contains invalid character {ch:?}")]
    InvalidChar { input: String, ch: char },
}

/// Failure parsing the inner content of a wikilink target.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum WikiError {
    #[error("wikilink target is empty")]
    EmptyTarget,
}

/// Crate-wide umbrella for multi-step fallible flows.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Id(#[from] IdError),
    #[error(transparent)]
    Tag(#[from] TagError),
    #[error(transparent)]
    Wiki(#[from] WikiError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_nonempty() {
        let cases: Vec<String> = vec![
            IdError::InvalidLength.to_string(),
            IdError::InvalidChar.to_string(),
            IdError::NonCanonical.to_string(),
            TagError::Empty.to_string(),
            TagError::InvalidChar {
                input: "a b".into(),
                ch: ' ',
            }
            .to_string(),
            WikiError::EmptyTarget.to_string(),
            Error::from(IdError::InvalidChar).to_string(),
            Error::from(TagError::Empty).to_string(),
            Error::from(WikiError::EmptyTarget).to_string(),
        ];
        for msg in cases {
            assert!(!msg.is_empty(), "error Display must be non-empty");
        }
    }

    #[test]
    fn error_from_conversions() {
        let id: Error = IdError::NonCanonical.into();
        assert!(matches!(id, Error::Id(IdError::NonCanonical)));
        let tag: Error = TagError::Empty.into();
        assert!(matches!(tag, Error::Tag(TagError::Empty)));
        let wiki: Error = WikiError::EmptyTarget.into();
        assert!(matches!(wiki, Error::Wiki(WikiError::EmptyTarget)));
    }

    #[test]
    fn iderror_from_decode_error() {
        assert_eq!(
            IdError::from(ulid::DecodeError::InvalidLength),
            IdError::InvalidLength
        );
        assert_eq!(
            IdError::from(ulid::DecodeError::InvalidChar),
            IdError::InvalidChar
        );
    }
}
