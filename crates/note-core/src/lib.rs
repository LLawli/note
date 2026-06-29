//! note-core: the IO-free, front-end-free domain model for `note`.
//!
//! INVARIANTS (M0): no rusqlite, no filesystem, no ratatui, no clap, no network.
//! No ambient clock or RNG: timestamps and id entropy are INJECTED at the edge
//! (note-cli / note-store). `ulid` is built without its std feature so the
//! ambient constructors are unavailable here; the durable enforcement is the
//! source guard in tests/io_free_guard.rs.

mod content;
mod error;
mod id;
mod link;
mod note;
mod tag;
mod time;
mod title;

pub use content::ContentKind;
pub use error::{Error, IdError, Result, TagError, WikiError};
pub use id::NoteId;
pub use link::{WikiLink, WikiTarget};
pub use note::Note;
pub use tag::Tag;
pub use time::Timestamp;
pub use title::derive_title;
