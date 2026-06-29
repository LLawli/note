use serde::{Deserialize, Serialize};

/// UTC instant as milliseconds since the Unix epoch (maps to SQLite INTEGER in M1).
/// Core NEVER reads the wall clock; the edge supplies `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const UNIX_EPOCH: Self = Self(0);

    #[inline]
    #[must_use]
    pub const fn from_unix_millis(ms: i64) -> Self {
        Self(ms)
    }

    #[inline]
    #[must_use]
    pub const fn as_unix_millis(self) -> i64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_roundtrip_millis() {
        let t = Timestamp::from_unix_millis(1_469_922_850_259);
        assert_eq!(t.as_unix_millis(), 1_469_922_850_259);
    }

    #[test]
    fn timestamp_ordering() {
        assert!(Timestamp::from_unix_millis(1) < Timestamp::from_unix_millis(2));
    }

    #[test]
    fn timestamp_unix_epoch_is_zero() {
        assert_eq!(Timestamp::UNIX_EPOCH.as_unix_millis(), 0);
    }

    #[test]
    fn timestamp_serde_is_transparent_integer() {
        let t = Timestamp::from_unix_millis(42);
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "42");
        let back: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}
