//! Perf probe for `backlinks` at 10k notes (not part of CI). Run with:
//!   cargo test -p note-store --release --test bench_backlinks -- --ignored --nocapture
//! Builds the realistic worst case for the live (read-time) backlink scan: a
//! popular "Hub" note referenced by many links that were stored *dangling*
//! (written before the Hub existed), among a large pool of unrelated dangling
//! links the scan must still walk.

use note_core::{ContentKind, NoteId, WikiLink, WikiTarget};
use note_store::{NewNote, Store};
use std::collections::BTreeSet;
use std::time::Instant;

const N: usize = 10_000;
const HUB_REFS: usize = 500; // dangling links that point at the Hub

fn note(body: &str, links: Vec<WikiLink>) -> NewNote {
    NewNote {
        title: None,
        body: body.to_owned(),
        content_kind: ContentKind::Markdown,
        tags: BTreeSet::new(),
        links,
    }
}

fn title_link(title: &str) -> Vec<WikiLink> {
    vec![WikiLink {
        target: WikiTarget::ByTitle(title.to_owned()),
        display: None,
    }]
}

#[test]
#[ignore = "perf probe; run with --release --ignored --nocapture"]
fn bench_backlinks_10k() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("notes.sqlite")).unwrap();
    let w = store.writer();

    let t0 = Instant::now();
    // HUB_REFS notes link [[Hub]] BEFORE the Hub exists -> stored dangling.
    for i in 0..HUB_REFS {
        w.create_note(note(&format!("ref {i} -> [[Hub]]"), title_link("Hub")))
            .unwrap();
    }
    // The Hub itself.
    let hub: NoteId = w.create_note(note("# Hub\nthe hub", vec![])).unwrap().id;
    // Fill up to N with notes carrying an unrelated dangling link, so the
    // dangling-link scan has a realistic amount to walk past.
    for i in HUB_REFS + 1..N {
        w.create_note(note(
            &format!("filler {i}"),
            title_link(&format!("ghost-{i}")),
        ))
        .unwrap();
    }
    eprintln!(
        "built {} notes in {:?} ({HUB_REFS} dangling refs to Hub)",
        store.readers().count_notes().unwrap(),
        t0.elapsed()
    );

    // Backlinks reads the stored `resolved_id`, so before a reindex the dangling
    // refs do not count and the lookup is a cheap indexed query.
    let bench = |label: &str| {
        let mut worst = std::time::Duration::ZERO;
        let mut hits = 0;
        for _ in 0..5 {
            let t = Instant::now();
            hits = store.readers().backlinks(hub).unwrap().len();
            worst = worst.max(t.elapsed());
        }
        eprintln!("backlinks(Hub) {label}: {hits} hits, worst of 5 = {worst:?}");
    };

    bench("before reindex");

    // A one-shot reindex resolves the dangling refs into the stored graph.
    let t = Instant::now();
    let changed = store.writer().reindex().unwrap();
    eprintln!("reindex: {changed} links updated in {:?}", t.elapsed());

    bench("after reindex");
}
