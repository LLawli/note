//! The edge where ambient identity/time enter the system. `note-core` is
//! IO-free and never reads a clock or RNG; this module is allowed to, because
//! `note-store` is the storage edge.

use note_core::{NoteId, Timestamp};
use std::time::{SystemTime, UNIX_EPOCH};
use ulid::Ulid;

/// Mint a fresh, time-sortable `NoteId` from the wall clock + RNG.
pub(crate) fn new_id() -> NoteId {
    NoteId::from(Ulid::new())
}

/// Current wall-clock instant as a `Timestamp` (Unix milliseconds, UTC).
pub(crate) fn now() -> Timestamp {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Timestamp::from_unix_millis(ms)
}
