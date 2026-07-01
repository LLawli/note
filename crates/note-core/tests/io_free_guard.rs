//! Asserts note-core source never reaches for an ambient clock, RNG, or IO,
//! independent of Cargo feature unification (which can turn ulid's std on in M1+).
//! Tests MAY do IO; the library may not. Comment/doc lines are skipped.

use std::{fs, path::Path};

const FORBIDDEN: &[&str] = &[
    "Ulid::new",
    "Ulid::from_datetime",
    "SystemTime",
    "Instant::now",
    "std::time",
    "std::fs",
    "std::net",
    "std::env",
    "env::var",
    "rand::",
    "getrandom",
    "thread_rng",
];

#[test]
fn core_is_io_and_ambient_free() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("//") || t.starts_with('*') {
                    continue; // skip comments / doc lines
                }
                // Only scan the code portion: a needle mentioned in a trailing
                // comment (e.g. `// resets SystemTime`) must not fail the guard.
                let code = line.split_once("//").map_or(line, |(before, _)| before);
                for needle in FORBIDDEN {
                    if code.contains(needle) {
                        offenders.push(format!("{}:{}: {needle}", path.display(), n + 1));
                    }
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "ambient/IO usage in note-core src:\n{}",
        offenders.join("\n")
    );
}
