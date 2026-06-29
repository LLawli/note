use crate::error::IdError;
use core::{fmt, str::FromStr};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ulid::Ulid;

/// Typed identity for a note: 128-bit ULID, lexicographically time-sortable,
/// 26-char Crockford base32, stable across export/import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NoteId(Ulid);

impl NoteId {
    /// Deterministic mint for tests / import: NO clock, NO RNG. `Ulid::from_parts`
    /// masks `timestamp_ms` to 48 bits and `random` to 80 bits.
    #[inline]
    #[must_use]
    pub const fn from_parts(timestamp_ms: u64, random: u128) -> Self {
        Self(Ulid::from_parts(timestamp_ms, random))
    }

    #[inline]
    #[must_use]
    pub const fn timestamp_ms(self) -> u64 {
        self.0.timestamp_ms()
    }
}

impl From<Ulid> for NoteId {
    fn from(u: Ulid) -> Self {
        Self(u)
    }
}

impl From<NoteId> for Ulid {
    fn from(n: NoteId) -> Self {
        n.0
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for NoteId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let id = Ulid::from_string(s).map_err(IdError::from)?;
        // ulid accepts lowercase (normalizes to uppercase) and silently masks
        // 26-char overflow inputs. Compare against the uppercased input so valid
        // lowercase ids pass but overflow/garbage is rejected.
        if id.to_string() != s.to_ascii_uppercase() {
            return Err(IdError::NonCanonical);
        }
        Ok(Self(id))
    }
}

// Serialize as the 26-char canonical string (portable JSON for M4 export round-trip).
// MANUAL impls are REQUIRED: ulid's `serde` feature fails to compile under
// default-features=false (it uses `String` without the std prelude).
impl Serialize for NoteId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for NoteId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const CANONICAL: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    #[test]
    fn noteid_parse_display_roundtrip() {
        let id: NoteId = CANONICAL.parse().unwrap();
        assert_eq!(id.to_string(), CANONICAL);
    }

    #[test]
    fn noteid_display_is_26_chars() {
        let id = NoteId::from_parts(1_469_922_850_259, 0xDEAD_BEEF);
        assert_eq!(id.to_string().len(), 26);
    }

    #[test]
    fn noteid_from_parts_recovers_timestamp() {
        let id = NoteId::from_parts(1_469_922_850_259, 0xDEAD_BEEF);
        assert_eq!(id.timestamp_ms(), 1_469_922_850_259);
    }

    #[test]
    fn noteid_from_str_trims_whitespace() {
        let id: NoteId = format!("  {CANONICAL}  ").parse().unwrap();
        assert_eq!(id.to_string(), CANONICAL);
    }

    #[test]
    fn noteid_accepts_lowercase() {
        let id: NoteId = CANONICAL.to_lowercase().parse().unwrap();
        assert_eq!(id.to_string(), CANONICAL);
    }

    #[test]
    fn noteid_rejects_empty() {
        assert_eq!("".parse::<NoteId>().unwrap_err(), IdError::InvalidLength);
    }

    #[test]
    fn noteid_rejects_too_short() {
        assert_eq!(
            "0123456789ABCDEFGHJKMNPQR".parse::<NoteId>().unwrap_err(),
            IdError::InvalidLength
        );
    }

    #[test]
    fn noteid_rejects_too_long() {
        assert_eq!(
            "01ARZ3NDEKTSV4RRFFQ69G5FAVX".parse::<NoteId>().unwrap_err(),
            IdError::InvalidLength
        );
    }

    #[test]
    fn noteid_rejects_invalid_char() {
        // 26 chars, ends in 'I' which is excluded from Crockford base32.
        assert_eq!(
            "0123456789ABCDEFGHJKMNPQRI".parse::<NoteId>().unwrap_err(),
            IdError::InvalidChar
        );
    }

    #[test]
    fn noteid_rejects_noncanonical_overflow() {
        // First char '8' makes the value exceed 128 bits; ulid masks it silently.
        assert_eq!(
            "80000000000000000000000000".parse::<NoteId>().unwrap_err(),
            IdError::NonCanonical
        );
    }

    #[test]
    fn noteid_time_ordering() {
        let a = NoteId::from_parts(1000, 0);
        let b = NoteId::from_parts(2000, 0);
        assert!(a < b);
    }

    #[test]
    fn noteid_hash_dedup() {
        let id: NoteId = CANONICAL.parse().unwrap();
        let set: HashSet<NoteId> = [id, id].into_iter().collect();
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn noteid_serde_json_is_canonical_string() {
        let id: NoteId = CANONICAL.parse().unwrap();
        let v = serde_json::to_value(id).unwrap();
        assert_eq!(v, serde_json::Value::String(CANONICAL.to_string()));
    }

    #[test]
    fn noteid_serde_json_roundtrip() {
        let id: NoteId = CANONICAL.parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        let back: NoteId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn noteid_serde_rejects_bad_string() {
        assert!(serde_json::from_str::<NoteId>("\"not-a-ulid\"").is_err());
    }
}
