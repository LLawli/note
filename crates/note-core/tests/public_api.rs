//! Black-box test of the note-core public surface + typed-identity lock.

use note_core::{ContentKind, Note, NoteId, Tag, Timestamp, WikiLink, WikiTarget, derive_title};

const CANONICAL: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

#[test]
fn public_surface_is_reachable() {
    let id_from_parts = NoteId::from_parts(1000, 42);
    let id_from_str: NoteId = CANONICAL.parse().unwrap();
    assert_ne!(id_from_parts, id_from_str);

    let tag = Tag::new("#Rust").unwrap();
    assert_eq!(tag.as_str(), "rust");

    let link: WikiLink = "Some Title|alias".parse().unwrap();
    assert_eq!(link.target, WikiTarget::ByTitle("Some Title".to_string()));

    let target: WikiTarget = CANONICAL.parse().unwrap();
    assert!(matches!(target, WikiTarget::ById(_)));

    assert_eq!(
        derive_title(None, "# Hello", ContentKind::Markdown),
        "Hello"
    );

    let mut note = Note::new(
        id_from_parts,
        "# Title\nbody",
        ContentKind::Markdown,
        Timestamp::from_unix_millis(1),
        Timestamp::from_unix_millis(2),
    );
    note.tags.insert(tag);
    assert_eq!(note.effective_title(), "Title");
}

#[test]
fn note_uses_typed_noteid() {
    let note = Note::new(
        NoteId::from_parts(1, 1),
        "x",
        ContentKind::Plain,
        Timestamp::UNIX_EPOCH,
        Timestamp::UNIX_EPOCH,
    );
    let _: NoteId = note.id; // typed identity at the boundary, not a bare String/int
}

#[test]
fn export_id_is_canonical_string() {
    let id: NoteId = CANONICAL.parse().unwrap();
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, format!("\"{id}\""));
}
