//! One-off perf probe (run with `--release -- --ignored --nocapture`). Not part
//! of CI. Builds 10k notes from a REAL corpus (tech blog articles) so vocabulary,
//! note sizes and term distribution are realistic. Point it at a corpus dir via
//! `NOTE_BENCH_CORPUS` containing `corpus_paras.txt` (one paragraph per line) and
//! `corpus_titles.txt` (one title per line).

use note_core::{ContentKind, Tag, WikiLink, WikiTarget};
use note_store::{NewNote, Store};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const N: usize = 10_000;
const PARAS_PER_NOTE: usize = 6;

fn spread(i: usize, k: usize, modulo: usize) -> usize {
    // cheap deterministic scatter so each note draws different paragraphs
    i.wrapping_mul(2_654_435_761)
        .wrapping_add(k.wrapping_mul(40_503))
        % modulo
}

fn effective_title(titles: &[String], i: usize) -> String {
    format!("{} {i}", titles[i % titles.len()].trim())
}

fn body(paras: &[String], titles: &[String], i: usize) -> String {
    let mut s = format!("# {}\n\n", effective_title(titles, i));
    for k in 0..PARAS_PER_NOTE {
        s.push_str(&paras[spread(i, k, paras.len())]);
        s.push_str("\n\n");
    }
    s
}

fn tags(titles: &[String], i: usize) -> BTreeSet<Tag> {
    let mut out = BTreeSet::new();
    out.insert(Tag::new("all").unwrap());
    for token in titles[i % titles.len()]
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .take(2)
    {
        if let Ok(tag) = Tag::new(token) {
            out.insert(tag);
        }
    }
    out
}

fn links(titles: &[String], i: usize) -> Vec<WikiLink> {
    [(i * 3 + 1) % N, (i * 7 + 5) % N]
        .into_iter()
        .map(|j| WikiLink {
            target: WikiTarget::ByTitle(effective_title(titles, j)),
            display: None,
        })
        .collect()
}

#[test]
#[ignore = "perf probe; run with NOTE_BENCH_CORPUS=<dir> ... --release --ignored --nocapture"]
fn bench_real_corpus_10k() {
    let dir = std::env::var("NOTE_BENCH_CORPUS")
        .expect("set NOTE_BENCH_CORPUS to a dir with corpus_paras.txt + corpus_titles.txt");
    let read = |name: &str| -> Vec<String> {
        std::fs::read_to_string(format!("{dir}/{name}"))
            .unwrap_or_else(|e| panic!("reading {dir}/{name}: {e}"))
            .lines()
            .map(str::to_owned)
            .filter(|l| !l.trim().is_empty())
            .collect()
    };
    let paras = read("corpus_paras.txt");
    let titles = read("corpus_titles.txt");
    assert!(!paras.is_empty() && !titles.is_empty(), "empty corpus");

    let avg_body = (0..50)
        .map(|i| body(&paras, &titles, i).len())
        .sum::<usize>()
        / 50;
    eprintln!(
        "corpus: {} paragraphs, {} titles; assembling {N} notes (~{avg_body} bytes/body)",
        paras.len(),
        titles.len()
    );

    let dirdb = tempfile::tempdir().unwrap();
    let store = Store::open(dirdb.path().join("notes.sqlite")).unwrap();

    let t0 = Instant::now();
    for i in 0..N {
        store
            .writer()
            .create_note(NewNote {
                title: None,
                body: body(&paras, &titles, i),
                content_kind: ContentKind::Markdown,
                tags: tags(&titles, i),
                links: links(&titles, i),
            })
            .unwrap();
    }
    let insert = t0.elapsed();
    eprintln!(
        "WRITE: {N} real notes in {insert:?} ({:?}/note)",
        insert / N as u32
    );
    eprintln!("count = {}", store.readers().count_notes().unwrap());

    let sample = store.readers().list_notes(1, 0).unwrap();
    if let Some(n) = sample.first() {
        let ls = store.readers().links_for(n.id).unwrap();
        let resolved = ls.iter().filter(|l| l.resolved.is_some()).count();
        eprintln!("sample note: {}/{} links resolved", resolved, ls.len());
    }

    // Per-keystroke search on real frequent words (one query per SearchChar).
    for target in ["rails", "ruby", "the"] {
        let mut total = Duration::ZERO;
        let mut worst = Duration::ZERO;
        let mut last = 0usize;
        for end in 1..=target.len() {
            let q = &target[..end];
            let t = Instant::now();
            last = store.readers().search_prefix(q, 500).unwrap().len();
            let dt = t.elapsed();
            total += dt;
            worst = worst.max(dt);
        }
        eprintln!(
            "READ search '{target}': total {total:?}, worst keystroke {worst:?} (last hits={last})"
        );
    }

    for c in ["a", "e", "s"] {
        let t = Instant::now();
        let hits = store.readers().search_prefix(c, 500).unwrap();
        eprintln!("  broad {c:?} -> {} hits in {:?}", hits.len(), t.elapsed());
    }

    let t = Instant::now();
    let listed = store.readers().list_notes(500, 0).unwrap();
    eprintln!(
        "READ list_notes(500): {} in {:?}",
        listed.len(),
        t.elapsed()
    );

    let t = Instant::now();
    let all = store.readers().all_notes().unwrap();
    eprintln!(
        "READ all_notes (export scale): {} in {:?}",
        all.len(),
        t.elapsed()
    );

    let t = Instant::now();
    let by_tag = store
        .readers()
        .list_by_tag(&Tag::new("all").unwrap(), 500)
        .unwrap();
    eprintln!(
        "READ list_by_tag('all', 500): {} in {:?}",
        by_tag.len(),
        t.elapsed()
    );
}
