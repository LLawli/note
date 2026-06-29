//! note-md: pure markdown parsing and interchange for `note`.
//!
//! Depends only on note-core; never touches SQLite. M3 owns `[[wikilink]]`
//! extraction; M4 TODO adds md/json import-export conversion.

mod interchange;
mod wikilink;

pub use interchange::{ImportedNote, MdError, from_json, from_markdown, to_json, to_markdown};
pub use wikilink::extract_wikilinks;
