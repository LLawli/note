//! Performance gate over a large, realistic corpus assembled at runtime.
//!
//! To avoid committing a giant fixture, the corpus is seeded from real RSS feeds
//! (`tests/feeds.json`) fetched with `curl` (no extra deps), then multiplied to
//! `NOTE_PERF_N` notes with small variations and random `[[wikilinks]]`. Several
//! feeds are listed so one being down is fine; if none respond the test SKIPS
//! (never fails a PR on a network blip).
//!
//! Run:  cargo test -p note-store --release --test perf -- --ignored --nocapture
//! Env:  NOTE_PERF_N        corpus size (default 50_000)
//!       NOTE_PERF_ENFORCE  `1` => assert the ceilings (gate); unset => report-only
//!
//! Enforcement starts OFF (informative): the numbers print and the run stays
//! green. Once the ceilings are calibrated from real CI runners, flip
//! `NOTE_PERF_ENFORCE=1` and make the job a required check.

use note_core::{ContentKind, NoteId, Tag, WikiLink, WikiTarget};
use note_store::{NewNote, Store};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const HUB_TITLE: &str = "Perf Hub";

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn enforce() -> bool {
    std::env::var("NOTE_PERF_ENFORCE").is_ok_and(|v| v == "1")
}

/// Report `actual` against `ceiling`; assert it only when enforcement is on.
fn gate(label: &str, actual: Duration, ceiling: Duration) {
    let over = actual > ceiling;
    eprintln!(
        "  [{}] {label}: {actual:?} (ceiling {ceiling:?})",
        if over { "OVER" } else { "ok" }
    );
    if enforce() {
        assert!(!over, "{label}: {actual:?} exceeds ceiling {ceiling:?}");
    }
}

// ---------------------------------------------------------------- corpus seeds

fn feed_urls() -> Vec<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/feeds.json");
    let json = std::fs::read_to_string(path).unwrap_or_default();
    json.split('"')
        .filter(|s| s.starts_with("http"))
        .map(str::to_owned)
        .collect()
}

fn fetch(url: &str) -> Option<String> {
    let out = std::process::Command::new("curl")
        .args(["-sSL", "--max-time", "15", "-A", "note-perf-bench", url])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let body = String::from_utf8_lossy(&out.stdout).into_owned();
    (body.len() > 200).then_some(body)
}

/// Inner text of the first `<tag …>…</tag>` in `block`: CDATA-unwrapped,
/// tag-stripped, with a few entities decoded and whitespace collapsed.
fn tag_text(block: &str, tag: &str) -> Option<String> {
    let open = block.find(&format!("<{tag}"))?;
    let gt = block[open..].find('>')? + open + 1;
    let close = block[gt..].find(&format!("</{tag}>"))? + gt;
    let raw = block[gt..close]
        .trim()
        .trim_start_matches("<![CDATA[")
        .trim_end_matches("]]>");
    Some(clean(raw))
}

fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn seeds_from(xml: &str, out: &mut Vec<(String, String)>) {
    // RSS uses <item>, Atom uses <entry>; splitting on the marker yields one
    // block per post (the first split piece is the channel preamble).
    for marker in ["<item", "<entry"] {
        let blocks: Vec<&str> = xml.split(marker).skip(1).collect();
        if blocks.is_empty() {
            continue;
        }
        for block in blocks {
            let title = tag_text(block, "title");
            let body = tag_text(block, "description")
                .or_else(|| tag_text(block, "summary"))
                .or_else(|| tag_text(block, "content"));
            if let (Some(t), Some(b)) = (title, body)
                && !t.is_empty()
                && b.len() >= 40
            {
                out.push((t, b));
            }
        }
        break; // a feed is one or the other; don't double-count
    }
}

fn load_seeds() -> Vec<(String, String)> {
    let mut seeds = Vec::new();
    for url in feed_urls() {
        match fetch(&url) {
            Some(xml) => {
                let before = seeds.len();
                seeds_from(&xml, &mut seeds);
                eprintln!("feed {url}: +{} posts", seeds.len() - before);
            }
            None => eprintln!("feed {url}: no response"),
        }
        if seeds.len() >= 30 {
            break; // enough variety to multiply from
        }
    }
    seeds
}

// ------------------------------------------------------------- corpus assembly

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn word(&mut self, seeds: &[(String, String)]) -> Option<String> {
        seeds[self.below(seeds.len())]
            .0
            .split(|c: char| !c.is_ascii_alphanumeric())
            .find(|w| w.len() >= 4 && w.chars().all(|c| c.is_ascii_alphabetic()))
            .map(str::to_lowercase)
    }
}

/// Deterministic per-index title (so the random links below resolve).
fn note_title(seeds: &[(String, String)], i: usize) -> String {
    if i == 0 {
        HUB_TITLE.to_owned()
    } else {
        format!("{} {i}", seeds[i % seeds.len()].0)
    }
}

fn make_note(seeds: &[(String, String)], i: usize, links: Vec<WikiLink>) -> NewNote {
    let title = note_title(seeds, i);
    let body = format!(
        "# {title}\n\n{}\n\nnote number {i}.",
        seeds[i % seeds.len()].1
    );
    let mut tags = BTreeSet::new();
    tags.insert(Tag::new("all").unwrap());
    NewNote {
        title: Some(title),
        body,
        content_kind: ContentKind::Markdown,
        tags,
        links,
    }
}

fn title_link(title: &str) -> WikiLink {
    WikiLink {
        target: WikiTarget::ByTitle(title.to_owned()),
        display: None,
    }
}

#[test]
#[ignore = "perf gate; run with --release --ignored --nocapture (uses network)"]
fn perf_corpus() {
    let n = env_usize("NOTE_PERF_N", 50_000).max(2);
    let seeds = load_seeds();
    if seeds.is_empty() {
        eprintln!("perf: no RSS feed responded — skipping (network/feeds.json needed)");
        return;
    }
    eprintln!(
        "perf: {} seed posts -> {n} notes (enforce={})",
        seeds.len(),
        enforce()
    );

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("notes.sqlite")).unwrap();
    let writer = store.writer();
    let readers = store.readers();

    // ---- (#3) write throughput: build the corpus ----
    let mut rng = Rng::new(0x00C0_FFEE);
    let build_start = Instant::now();
    let hub: NoteId = writer.create_note(make_note(&seeds, 0, vec![])).unwrap().id;
    for i in 1..n {
        // two random earlier links, plus a Hub link every 100th note so the Hub
        // has many backlinks for the backlinks measurement.
        let mut links = vec![
            title_link(&note_title(&seeds, rng.below(i))),
            title_link(&note_title(&seeds, rng.below(i))),
        ];
        if i % 100 == 0 {
            links.push(title_link(HUB_TITLE));
        }
        writer.create_note(make_note(&seeds, i, links)).unwrap();
    }
    let build = build_start.elapsed();
    eprintln!(
        "WRITE: {n} notes in {build:?} ({:?}/note)",
        build / n as u32
    );
    gate("write/note", build / n as u32, Duration::from_millis(5));

    // ---- (#1) search latency, one query per keystroke ----
    let word = rng.word(&seeds).unwrap_or_else(|| "note".to_owned());
    let mut worst = Duration::ZERO;
    let mut hits = 0;
    for end in 1..=word.len().min(8) {
        let mark = Instant::now();
        hits = readers.search_prefix(&word[..end], 500).unwrap().len();
        worst = worst.max(mark.elapsed());
    }
    eprintln!("SEARCH {word:?}: worst keystroke {worst:?} (last hits={hits})");
    gate("search/keystroke", worst, Duration::from_millis(150));

    // ---- (#2) backlinks of the popular Hub ----
    let mark = Instant::now();
    let back = readers.backlinks(hub).unwrap().len();
    eprintln!("BACKLINKS(hub): {back} in {:?}", mark.elapsed());
    gate("backlinks", mark.elapsed(), Duration::from_millis(50));

    // The heavier reference/list/reindex probes run only in the full tier
    // (NOTE_PERF_FULL), kept off the per-PR gate so reindex (O(N)) doesn't add
    // tens of seconds to every PR.
    if std::env::var("NOTE_PERF_FULL").is_err() {
        eprintln!("(set NOTE_PERF_FULL=1 for resolve_ref / list / reindex probes)");
        return;
    }

    // ---- (#5) reference resolution ----
    let title_mid = note_title(&seeds, n / 2);
    let mark = Instant::now();
    let _ = readers.resolve_ref(&title_mid).unwrap();
    eprintln!("RESOLVE_REF(title): {:?}", mark.elapsed());
    gate(
        "resolve_ref/title",
        mark.elapsed(),
        Duration::from_millis(50),
    );

    let prefix: String = hub.to_string().chars().take(12).collect();
    let mark = Instant::now();
    let _ = readers.resolve_ref(&prefix).unwrap();
    eprintln!("RESOLVE_REF(id-prefix): {:?}", mark.elapsed());
    gate(
        "resolve_ref/prefix",
        mark.elapsed(),
        Duration::from_millis(50),
    );

    // ---- (#6) list / tag filter ----
    let mark = Instant::now();
    let listed = readers.list_notes(500, 0).unwrap().len();
    eprintln!("LIST_NOTES(500): {listed} in {:?}", mark.elapsed());
    gate("list_notes(500)", mark.elapsed(), Duration::from_millis(50));

    let mark = Instant::now();
    let by = readers
        .list_by_tag(&Tag::new("all").unwrap(), 500)
        .unwrap()
        .len();
    eprintln!("LIST_BY_TAG(500): {by} in {:?}", mark.elapsed());
    gate(
        "list_by_tag(500)",
        mark.elapsed(),
        Duration::from_millis(100),
    );

    // ---- (#4) reindex the whole graph ----
    let mark = Instant::now();
    let changed = writer.reindex().unwrap();
    eprintln!("REINDEX: {changed} changed in {:?}", mark.elapsed());
    // Scales with link count (O(N)); a loose ceiling pending calibration.
    gate("reindex", mark.elapsed(), Duration::from_secs(45));
}
