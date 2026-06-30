//! M1 integration tests: real SQLite file, real migrations, the writer thread
//! and the read pool. Covers CRUD, tags, FTS search + ranking, the link graph,
//! and the FTS<->notes desync guard.

use note_core::{ContentKind, NoteId, Tag, Timestamp, WikiLink, WikiTarget};
use note_store::{ImportNote, ImportOutcome, NewNote, NotePatch, Store};
use std::collections::BTreeSet;
use std::str::FromStr;

fn tmp_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("notes.sqlite")).unwrap();
    (store, dir)
}

fn tags(names: &[&str]) -> BTreeSet<Tag> {
    names.iter().map(|n| Tag::new(n).unwrap()).collect()
}

fn new_note(body: &str, tag_names: &[&str]) -> NewNote {
    NewNote {
        title: None,
        body: body.to_owned(),
        content_kind: ContentKind::Markdown,
        tags: tags(tag_names),
        links: Vec::new(),
    }
}

#[test]
fn create_then_get_roundtrip() {
    let (store, _dir) = tmp_store();
    let created = store
        .writer()
        .create_note(new_note("# Hello\nworld", &["rust", "cli"]))
        .unwrap();

    let fetched = store.readers().get_note(created.id).unwrap().unwrap();
    assert_eq!(fetched, created);
    assert_eq!(fetched.effective_title(), "Hello");
    assert_eq!(fetched.tags, tags(&["cli", "rust"]));
    assert_eq!(fetched.created, fetched.updated);
}

#[test]
fn get_missing_is_none() {
    let (store, _dir) = tmp_store();
    let id = NoteId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    assert!(store.readers().get_note(id).unwrap().is_none());
}

#[test]
fn update_changes_fields_and_bumps_updated() {
    let (store, _dir) = tmp_store();
    let created = store
        .writer()
        .create_note(new_note("original", &["a"]))
        .unwrap();

    let patch = NotePatch {
        title: Some("Explicit".to_owned()),
        body: "rewritten body".to_owned(),
        content_kind: ContentKind::Plain,
        tags: tags(&["b", "c"]),
        links: Vec::new(),
    };
    let updated = store
        .writer()
        .update_note(created.id, patch)
        .unwrap()
        .unwrap();

    assert_eq!(updated.id, created.id);
    assert_eq!(updated.title.as_deref(), Some("Explicit"));
    assert_eq!(updated.body, "rewritten body");
    assert_eq!(updated.content_kind, ContentKind::Plain);
    assert_eq!(updated.tags, tags(&["b", "c"]));
    assert_eq!(updated.created, created.created);
    assert!(updated.updated.as_unix_millis() >= created.updated.as_unix_millis());

    let reloaded = store.readers().get_note(created.id).unwrap().unwrap();
    assert_eq!(reloaded, updated);
}

#[test]
fn update_missing_returns_none() {
    let (store, _dir) = tmp_store();
    let id = NoteId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let patch = NotePatch {
        title: None,
        body: "x".to_owned(),
        content_kind: ContentKind::Markdown,
        tags: BTreeSet::new(),
        links: Vec::new(),
    };
    assert!(store.writer().update_note(id, patch).unwrap().is_none());
}

#[test]
fn delete_removes_note_and_returns_flag() {
    let (store, _dir) = tmp_store();
    let created = store
        .writer()
        .create_note(new_note("doomed", &["x"]))
        .unwrap();

    assert!(store.writer().delete_note(created.id).unwrap());
    assert!(store.readers().get_note(created.id).unwrap().is_none());
    // second delete is a no-op
    assert!(!store.writer().delete_note(created.id).unwrap());
}

#[test]
fn search_finds_by_body_and_title() {
    let (store, _dir) = tmp_store();
    store
        .writer()
        .create_note(new_note("the quick brown fox", &[]))
        .unwrap();
    store
        .writer()
        .create_note(new_note("# Lazy Dog\nsleeping", &[]))
        .unwrap();
    store
        .writer()
        .create_note(new_note("unrelated text", &[]))
        .unwrap();

    let fox = store.readers().search("fox", 10).unwrap();
    assert_eq!(fox.len(), 1);
    assert!(fox[0].body.contains("fox"));

    let dog = store.readers().search("dog", 10).unwrap();
    assert_eq!(dog.len(), 1);
    assert_eq!(dog[0].effective_title(), "Lazy Dog");

    assert_eq!(
        store.readers().search("nonexistentterm", 10).unwrap().len(),
        0
    );
}

#[test]
fn prefix_search_matches_partial_words() {
    let (store, _dir) = tmp_store();
    store
        .writer()
        .create_note(new_note("deixe uma mensagem", &[]))
        .unwrap();
    store
        .writer()
        .create_note(new_note("outra mensagem aqui", &[]))
        .unwrap();
    store
        .writer()
        .create_note(new_note("nada a ver", &[]))
        .unwrap();

    // whole-token search misses the partial word ...
    assert_eq!(store.readers().search("mensag", 10).unwrap().len(), 0);
    // ... but prefix search finds both notes.
    assert_eq!(
        store.readers().search_prefix("mensag", 10).unwrap().len(),
        2
    );
    // exact word still works
    assert_eq!(
        store.readers().search_prefix("mensagem", 10).unwrap().len(),
        2
    );
    // empty query is a no-op, not an error
    assert_eq!(store.readers().search_prefix("   ", 10).unwrap().len(), 0);
}

#[test]
fn search_reflects_updates_and_deletes() {
    let (store, _dir) = tmp_store();
    let n = store
        .writer()
        .create_note(new_note("findme alpha", &[]))
        .unwrap();
    assert_eq!(store.readers().search("findme", 10).unwrap().len(), 1);

    // update removes the old term, adds a new one
    let patch = NotePatch {
        title: None,
        body: "replaced beta".to_owned(),
        content_kind: ContentKind::Markdown,
        tags: BTreeSet::new(),
        links: Vec::new(),
    };
    store.writer().update_note(n.id, patch).unwrap();
    assert_eq!(store.readers().search("findme", 10).unwrap().len(), 0);
    assert_eq!(store.readers().search("beta", 10).unwrap().len(), 1);

    // delete removes it from the index
    store.writer().delete_note(n.id).unwrap();
    assert_eq!(store.readers().search("beta", 10).unwrap().len(), 0);
}

#[test]
fn list_orders_by_updated_desc() {
    let (store, _dir) = tmp_store();
    let a = store.writer().create_note(new_note("first", &[])).unwrap();
    let b = store.writer().create_note(new_note("second", &[])).unwrap();

    // touch `a` so it becomes most-recent
    let patch = NotePatch {
        title: None,
        body: "first touched".to_owned(),
        content_kind: ContentKind::Markdown,
        tags: BTreeSet::new(),
        links: Vec::new(),
    };
    store.writer().update_note(a.id, patch).unwrap();

    let listed = store.readers().list_notes(10, 0).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, a.id);
    assert_eq!(listed[1].id, b.id);

    assert_eq!(store.readers().most_recent().unwrap().unwrap().id, a.id);
}

#[test]
fn most_recent_empty_db_is_none() {
    let (store, _dir) = tmp_store();
    assert!(store.readers().most_recent().unwrap().is_none());
}

#[test]
fn links_store_with_id_resolution_and_dangling_title() {
    let (store, _dir) = tmp_store();
    let target = store
        .writer()
        .create_note(new_note("# Target", &[]))
        .unwrap();
    let source = store
        .writer()
        .create_note(new_note("# Source", &[]))
        .unwrap();

    let links = vec![
        WikiLink {
            target: WikiTarget::ById(target.id),
            display: None,
        },
        WikiLink {
            target: WikiTarget::ByTitle("Nonexistent".to_owned()),
            display: Some("alias".to_owned()),
        },
    ];
    store.writer().replace_links(source.id, links).unwrap();

    let stored = store.readers().links_for(source.id).unwrap();
    assert_eq!(stored.len(), 2);

    let by_id = stored
        .iter()
        .find(|l| matches!(l.target, WikiTarget::ById(_)))
        .unwrap();
    assert_eq!(by_id.resolved, Some(target.id));

    let by_title = stored
        .iter()
        .find(|l| matches!(l.target, WikiTarget::ByTitle(_)))
        .unwrap();
    assert_eq!(by_title.resolved, None);
    assert_eq!(by_title.display.as_deref(), Some("alias"));
}

#[test]
fn create_writes_links_in_the_same_call() {
    let (store, _dir) = tmp_store();
    let target = store
        .writer()
        .create_note(new_note("# Target", &[]))
        .unwrap();
    let source = store
        .writer()
        .create_note(NewNote {
            title: None,
            body: "see [[Target]]".to_owned(),
            content_kind: ContentKind::Markdown,
            tags: BTreeSet::new(),
            links: vec![WikiLink {
                target: WikiTarget::ByTitle("Target".to_owned()),
                display: None,
            }],
        })
        .unwrap();

    // links are committed atomically with the note (no separate replace_links)
    let links = store.readers().links_for(source.id).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].resolved, Some(target.id));
}

#[test]
fn ambiguous_title_is_consistent_across_reader_and_writer() {
    let (store, _dir) = tmp_store();
    // two notes share the same effective title "Dup"
    store.writer().create_note(new_note("# Dup", &[])).unwrap();
    store.writer().create_note(new_note("# Dup", &[])).unwrap();

    // reader side: resolve_ref returns BOTH candidates (ambiguous -> picker)
    assert_eq!(store.readers().resolve_ref("Dup").unwrap().len(), 2);

    // writer side: a [[Dup]] link stays dangling (ambiguous -> not resolved).
    // Both paths build on the shared `title_matches`, so they agree.
    let src = store
        .writer()
        .create_note(NewNote {
            title: None,
            body: "[[Dup]]".to_owned(),
            content_kind: ContentKind::Markdown,
            tags: BTreeSet::new(),
            links: vec![WikiLink {
                target: WikiTarget::ByTitle("Dup".to_owned()),
                display: None,
            }],
        })
        .unwrap();
    let links = store.readers().links_for(src.id).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].resolved, None);
}

#[test]
fn title_link_resolves_to_existing_note() {
    let (store, _dir) = tmp_store();
    // target's effective title comes from its H1.
    let target = store
        .writer()
        .create_note(new_note("# Meeting Notes\nbody", &[]))
        .unwrap();
    let source = store
        .writer()
        .create_note(new_note("refers elsewhere", &[]))
        .unwrap();

    let links = vec![
        // case-insensitive title match resolves to the target id
        WikiLink {
            target: WikiTarget::ByTitle("meeting notes".to_owned()),
            display: None,
        },
        WikiLink {
            target: WikiTarget::ByTitle("No Such Title".to_owned()),
            display: None,
        },
    ];
    store.writer().replace_links(source.id, links).unwrap();

    let stored = store.readers().links_for(source.id).unwrap();
    let resolved: Vec<_> = stored.iter().filter_map(|l| l.resolved).collect();
    assert_eq!(resolved, vec![target.id]);
}

#[test]
fn replace_links_is_idempotent_overwrite() {
    let (store, _dir) = tmp_store();
    let source = store.writer().create_note(new_note("src", &[])).unwrap();
    let t = store.writer().create_note(new_note("t", &[])).unwrap();

    let one = vec![WikiLink {
        target: WikiTarget::ByTitle("A".to_owned()),
        display: None,
    }];
    store.writer().replace_links(source.id, one).unwrap();
    assert_eq!(store.readers().links_for(source.id).unwrap().len(), 1);

    let two = vec![
        WikiLink {
            target: WikiTarget::ById(t.id),
            display: None,
        },
        WikiLink {
            target: WikiTarget::ByTitle("B".to_owned()),
            display: None,
        },
    ];
    store.writer().replace_links(source.id, two).unwrap();
    assert_eq!(store.readers().links_for(source.id).unwrap().len(), 2);
}

#[test]
fn deleting_source_cascades_links() {
    let (store, _dir) = tmp_store();
    let source = store.writer().create_note(new_note("src", &[])).unwrap();
    let links = vec![WikiLink {
        target: WikiTarget::ByTitle("X".to_owned()),
        display: None,
    }];
    store.writer().replace_links(source.id, links).unwrap();
    assert_eq!(store.readers().links_for(source.id).unwrap().len(), 1);

    store.writer().delete_note(source.id).unwrap();
    assert_eq!(store.readers().links_for(source.id).unwrap().len(), 0);
}

#[test]
fn reopening_persists_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.sqlite");
    let id = {
        let store = Store::open(&path).unwrap();
        store
            .writer()
            .create_note(new_note("persist me", &["keep"]))
            .unwrap()
            .id
    };
    // second open: migrations must be idempotent and data must survive
    let store = Store::open(&path).unwrap();
    let note = store.readers().get_note(id).unwrap().unwrap();
    assert_eq!(note.body, "persist me");
    assert_eq!(note.tags, tags(&["keep"]));
}

#[test]
fn import_creates_then_updates_idempotently() {
    let (store, _dir) = tmp_store();
    let id = NoteId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let req = ImportNote {
        id: Some(id),
        title: Some("Imported".to_owned()),
        body: "body one".to_owned(),
        content_kind: ContentKind::Markdown,
        tags: tags(&["x"]),
        created: Some(Timestamp::from_unix_millis(111)),
        updated: Some(Timestamp::from_unix_millis(222)),
        links: Vec::new(),
    };

    let (note, outcome) = store.writer().import_note(req.clone()).unwrap();
    assert_eq!(outcome, ImportOutcome::Created);
    assert_eq!(note.id, id);
    assert_eq!(note.created, Timestamp::from_unix_millis(111));

    // re-importing the same id updates in place, preserving supplied timestamps
    let (_, outcome) = store.writer().import_note(req).unwrap();
    assert_eq!(outcome, ImportOutcome::Updated);
    assert_eq!(store.readers().count_notes().unwrap(), 1);

    let reloaded = store.readers().get_note(id).unwrap().unwrap();
    assert_eq!(reloaded.created, Timestamp::from_unix_millis(111));
    assert_eq!(reloaded.updated, Timestamp::from_unix_millis(222));
    assert!(store.readers().search("body", 10).unwrap().len() == 1);
}

#[test]
fn import_without_id_mints_one() {
    let (store, _dir) = tmp_store();
    let req = ImportNote {
        id: None,
        title: None,
        body: "fresh import".to_owned(),
        content_kind: ContentKind::Markdown,
        tags: BTreeSet::new(),
        created: None,
        updated: None,
        links: Vec::new(),
    };
    let (note, outcome) = store.writer().import_note(req).unwrap();
    assert_eq!(outcome, ImportOutcome::Created);
    assert!(store.readers().get_note(note.id).unwrap().is_some());
}

/// The carved-in invariant: the FTS index never drifts from `notes`. After a mix
/// of creates, updates and deletes, the row counts must match exactly.
#[test]
fn fts_never_desyncs_from_notes() {
    let (store, _dir) = tmp_store();
    let mut ids = Vec::new();
    for i in 0..10 {
        ids.push(
            store
                .writer()
                .create_note(new_note(&format!("note number {i}"), &[]))
                .unwrap()
                .id,
        );
    }
    for id in ids.iter().take(3).copied() {
        let patch = NotePatch {
            title: None,
            body: "edited content".to_owned(),
            content_kind: ContentKind::Markdown,
            tags: BTreeSet::new(),
            links: Vec::new(),
        };
        store.writer().update_note(id, patch).unwrap();
    }
    for id in ids.iter().skip(7).copied() {
        store.writer().delete_note(id).unwrap();
    }

    // Every surviving note must be findable, and a broad search must return
    // exactly the 7 remaining notes (10 created - 3 deleted).
    let all = store
        .readers()
        .search("note OR edited OR content OR number", 100)
        .unwrap();
    assert_eq!(all.len(), 7);
    assert_eq!(store.readers().list_notes(100, 0).unwrap().len(), 7);
}

fn link_by_title(title: &str) -> Vec<WikiLink> {
    vec![WikiLink {
        target: WikiTarget::ByTitle(title.to_owned()),
        display: None,
    }]
}

#[test]
fn wikilink_resolves_by_unique_id_prefix() {
    let (store, _dir) = tmp_store();
    let target = store
        .writer()
        .create_note(new_note("# Nota real\nbody", &[]))
        .unwrap();
    // A git-style id prefix (classified ByTitle by note-core) must resolve like
    // `note show` does, not dangle. 16 chars includes random bits past the ULID's
    // 10-char timestamp so it can't collide with the source note's id.
    let prefix: String = target.id.to_string().chars().take(16).collect();
    let source = store
        .writer()
        .create_note(NewNote {
            title: None,
            body: format!("see [[{prefix}]]"),
            content_kind: ContentKind::Markdown,
            tags: BTreeSet::new(),
            links: link_by_title(&prefix),
        })
        .unwrap();

    let links = store.readers().links_for(source.id).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].resolved, Some(target.id));
}

#[test]
fn resolve_link_target_prefix_then_title() {
    let (store, _dir) = tmp_store();
    let a = store
        .writer()
        .create_note(new_note("# Alpha\nx", &[]))
        .unwrap();
    store
        .writer()
        .create_note(new_note("# Beta\ny", &[]))
        .unwrap();

    assert_eq!(
        store.readers().resolve_link_target("Alpha").unwrap(),
        Some(a.id)
    );
    // 16 chars reaches past the shared 10-char timestamp into random bits, so the
    // prefix is unique to `a` even though `a` and `b` are minted milliseconds apart.
    let prefix: String = a.id.to_string().chars().take(16).collect();
    assert_eq!(
        store.readers().resolve_link_target(&prefix).unwrap(),
        Some(a.id)
    );
    assert_eq!(store.readers().resolve_link_target("Nope").unwrap(), None);
}

#[test]
fn backlinks_returns_linking_sources() {
    let (store, _dir) = tmp_store();
    let target = store
        .writer()
        .create_note(new_note("# Target\nx", &[]))
        .unwrap();
    let s1 = store
        .writer()
        .create_note(NewNote {
            title: None,
            body: "links [[Target]]".to_owned(),
            content_kind: ContentKind::Markdown,
            tags: BTreeSet::new(),
            links: link_by_title("Target"),
        })
        .unwrap();
    let s2 = store
        .writer()
        .create_note(NewNote {
            title: None,
            body: "also [[Target]]".to_owned(),
            content_kind: ContentKind::Markdown,
            tags: BTreeSet::new(),
            links: link_by_title("Target"),
        })
        .unwrap();

    let ids: BTreeSet<_> = store
        .readers()
        .backlinks(target.id)
        .unwrap()
        .iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(ids, BTreeSet::from([s1.id, s2.id]));
    assert!(store.readers().backlinks(s1.id).unwrap().is_empty());
}

#[test]
fn resolve_link_live_resolves_a_stored_dangling_link() {
    let (store, _dir) = tmp_store();
    // Links to a title that does not exist yet -> stored dangling.
    let source = store
        .writer()
        .create_note(NewNote {
            title: None,
            body: "see [[Future]]".to_owned(),
            content_kind: ContentKind::Markdown,
            tags: BTreeSet::new(),
            links: link_by_title("Future"),
        })
        .unwrap();
    let link = store.readers().links_for(source.id).unwrap().remove(0);
    assert_eq!(link.resolved, None);

    // The target appears later; the stored row is still dangling, but live
    // resolution finds it.
    let future = store
        .writer()
        .create_note(new_note("# Future\nx", &[]))
        .unwrap();
    assert_eq!(
        store.readers().resolve_link(&link).unwrap(),
        Some(future.id)
    );
}
