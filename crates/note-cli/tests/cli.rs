//! CLI-surface regression tests: drive the real `note` binary against a
//! throwaway data dir. Covers the subcommand surface, NoteRef resolution kinds,
//! the piped-vs-styled render path, the ambiguity fallback, and JSON output.

use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;
use tempfile::TempDir;

fn note(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("note").unwrap();
    cmd.env("NOTE_DATA_DIR", dir.path());
    cmd
}

/// Create a note via --message and return its id (stdout).
fn create(dir: &TempDir, title: &str, body: &str, tags: &[&str]) -> String {
    let mut cmd = note(dir);
    cmd.args(["new", title, "-m", body]);
    for t in tags {
        cmd.args(["-t", t]);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "new failed: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn help_lists_every_subcommand() {
    let dir = TempDir::new().unwrap();
    note(&dir)
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("new"))
        .stdout(contains("edit"))
        .stdout(contains("delete"))
        .stdout(contains("show"))
        .stdout(contains("links"))
        .stdout(contains("search"))
        .stdout(contains("list"))
        .stdout(contains("import"))
        .stdout(contains("export"))
        .stdout(contains("tags"))
        .stdout(contains("status"));
}

#[test]
fn new_then_show_roundtrips_the_body() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Title", "# Title\nbody text", &[]);
    note(&dir)
        .args(["show", &id, "--raw"])
        .assert()
        .success()
        .stdout(contains("# Title"))
        .stdout(contains("body text"));
}

#[test]
fn show_piped_output_is_raw_markdown() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Heading", "# Heading\ntext", &[]);
    // Piped (not a TTY) => auto mode prints raw markdown, hashes intact.
    note(&dir)
        .args(["show", &id])
        .assert()
        .success()
        .stdout(contains("# Heading"));
}

#[test]
fn resolve_by_full_id() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Exact", "body here", &[]);
    note(&dir)
        .args(["show", &id, "--raw"])
        .assert()
        .success()
        .stdout(contains("body here"));
}

#[test]
fn resolve_by_case_insensitive_title() {
    let dir = TempDir::new().unwrap();
    create(&dir, "My Special Title", "the body", &[]);
    note(&dir)
        .args(["show", "my special title", "--raw"])
        .assert()
        .success()
        .stdout(contains("the body"));
}

#[test]
fn resolve_by_unique_prefix() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Only", "lonely body", &[]);
    let prefix = &id[..10];
    note(&dir)
        .args(["show", prefix, "--raw"])
        .assert()
        .success()
        .stdout(contains("lonely body"));
}

#[test]
fn ambiguous_reference_lists_and_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    create(&dir, "Alpha", "aaa", &[]);
    create(&dir, "Beta", "bbb", &[]);
    // "01" is a valid ULID prefix shared by every 2026-era id => ambiguous.
    note(&dir)
        .args(["show", "01"])
        .assert()
        .failure()
        .stderr(contains("ambiguous"))
        .stderr(contains("Alpha"))
        .stderr(contains("Beta"));
}

#[test]
fn missing_reference_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    create(&dir, "Something", "content", &[]);
    note(&dir)
        .args(["show", "totally-absent-xyz"])
        .assert()
        .failure()
        .stderr(contains("no note matches"));
}

#[test]
fn search_finds_by_term() {
    let dir = TempDir::new().unwrap();
    create(&dir, "Rusty", "all about rustlang", &[]);
    create(&dir, "Other", "unrelated", &[]);
    note(&dir)
        .args(["search", "rustlang"])
        .assert()
        .success()
        .stdout(contains("Rusty"));
    note(&dir)
        .args(["search", "noopematch"])
        .assert()
        .success()
        .stderr(contains("no matches"));
}

#[test]
fn search_matches_word_prefixes() {
    let dir = TempDir::new().unwrap();
    create(&dir, "Recado", "deixe uma mensagem", &[]);
    create(&dir, "Outro", "segunda mensagem", &[]);
    // a partial word finds both notes (prefix search)
    let out = note(&dir).args(["search", "mensag"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout.lines().count(),
        2,
        "prefix should match both: {stdout:?}"
    );
}

#[test]
fn list_shows_titles_most_recent_first() {
    let dir = TempDir::new().unwrap();
    create(&dir, "First", "1", &[]);
    create(&dir, "Second", "2", &[]);
    let out = note(&dir).arg("list").output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let first_line = stdout.lines().next().unwrap_or_default();
    assert!(
        first_line.contains("Second"),
        "most recent should lead: {stdout:?}"
    );
}

#[test]
fn status_reports_count() {
    let dir = TempDir::new().unwrap();
    create(&dir, "One", "x", &[]);
    note(&dir)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("notes:"))
        .stdout(contains("1"));
}

#[test]
fn status_json_is_machine_readable() {
    let dir = TempDir::new().unwrap();
    create(&dir, "One", "x", &[]);
    note(&dir)
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(contains("\"notes\""))
        .stdout(contains("\"database\""));
}

#[test]
fn show_json_includes_id_and_tags() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Tagged", "body", &["rust", "cli"]);
    note(&dir)
        .args(["show", &id, "--json"])
        .assert()
        .success()
        .stdout(contains(&id[..]))
        .stdout(contains("\"rust\""))
        .stdout(contains("\"cli\""));
}

#[test]
fn new_reads_body_from_stdin_when_piped() {
    let dir = TempDir::new().unwrap();
    let out = note(&dir)
        .args(["new", "Piped"])
        .write_stdin("body from stdin")
        .output()
        .unwrap();
    assert!(out.status.success());
    let id = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    note(&dir)
        .args(["show", &id, "--raw"])
        .assert()
        .success()
        .stdout(contains("body from stdin"));
}

#[test]
fn empty_note_is_rejected() {
    let dir = TempDir::new().unwrap();
    note(&dir)
        .args(["new", "-m", ""])
        .assert()
        .failure()
        .stderr(contains("empty note"));
}

#[test]
fn edit_replaces_body_via_message() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Draft", "old body", &[]);
    note(&dir)
        .args(["edit", &id, "-m", "new body"])
        .assert()
        .success();
    note(&dir)
        .args(["show", &id, "--raw"])
        .assert()
        .success()
        .stdout(contains("new body"))
        .stdout(contains("old body").not());
}

#[test]
fn edit_title_only_keeps_body() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Original", "precious body", &[]);
    // Metadata-only edit must not touch the body (even though stdin is piped).
    note(&dir)
        .args(["edit", &id, "--title", "Renamed"])
        .assert()
        .success();
    note(&dir)
        .args(["show", &id, "--json"])
        .assert()
        .success()
        .stdout(contains("\"Renamed\""));
    note(&dir)
        .args(["show", &id, "--raw"])
        .assert()
        .success()
        .stdout(contains("precious body"));
}

#[test]
fn edit_replaces_tags() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Tagged", "body", &["old"]);
    note(&dir)
        .args(["edit", &id, "-t", "fresh", "-t", "new"])
        .assert()
        .success();
    note(&dir)
        .args(["show", &id, "--json"])
        .assert()
        .success()
        .stdout(contains("\"fresh\""))
        .stdout(contains("\"new\""))
        .stdout(contains("\"old\"").not());
}

#[test]
fn edit_reads_body_from_stdin() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Piped", "before", &[]);
    note(&dir)
        .args(["edit", &id])
        .write_stdin("after via stdin")
        .assert()
        .success();
    note(&dir)
        .args(["show", &id, "--raw"])
        .assert()
        .success()
        .stdout(contains("after via stdin"));
}

#[test]
fn edit_missing_reference_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    create(&dir, "Something", "content", &[]);
    note(&dir)
        .args(["edit", "totally-absent-xyz", "-m", "x"])
        .assert()
        .failure()
        .stderr(contains("no note matches"));
}

#[test]
fn delete_removes_note_with_yes() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Doomed", "bye", &[]);
    note(&dir).args(["delete", &id, "--yes"]).assert().success();
    note(&dir)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("notes:    0"));
    note(&dir)
        .args(["show", &id])
        .assert()
        .failure()
        .stderr(contains("no note matches"));
}

#[test]
fn delete_rm_alias_works() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Temp", "x", &[]);
    note(&dir).args(["rm", &id, "-y"]).assert().success();
    note(&dir)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("notes:    0"));
}

#[test]
fn delete_without_yes_refuses_when_piped() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Keep", "still here", &[]);
    note(&dir)
        .args(["delete", &id])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(contains("refusing to delete"));
    // the note must survive the refused delete
    note(&dir)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("notes:    1"));
}

#[test]
fn delete_missing_reference_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    create(&dir, "Something", "content", &[]);
    note(&dir)
        .args(["delete", "totally-absent-xyz", "--yes"])
        .assert()
        .failure()
        .stderr(contains("no note matches"));
}

#[test]
fn links_are_extracted_and_resolved_on_write() {
    let dir = TempDir::new().unwrap();
    let target = create(&dir, "Target Note", "# Target Note\nbody", &[]);
    let source = create(&dir, "Source", "see [[Target Note]] and [[Missing]]", &[]);

    let out = note(&dir)
        .args(["links", &source, "--json"])
        .output()
        .unwrap();
    let json = String::from_utf8(out.stdout).unwrap();
    // the title link resolves to the target id; the missing one is dangling (null)
    assert!(
        json.contains(&target),
        "expected resolved target id in {json}"
    );
    assert!(json.contains("\"Missing\""));
    assert!(json.contains("null"));
}

#[test]
fn editing_body_rewrites_the_link_graph() {
    let dir = TempDir::new().unwrap();
    create(&dir, "A", "node a", &[]);
    let source = create(&dir, "Source", "links to [[A]]", &[]);
    note(&dir)
        .args(["links", &source])
        .assert()
        .success()
        .stdout(contains("[[A]]"));

    // editing the body to drop the link must clear it from the graph
    note(&dir)
        .args(["edit", &source, "-m", "no more links"])
        .assert()
        .success();
    note(&dir)
        .args(["links", &source])
        .assert()
        .success()
        .stderr(contains("no links"));
}

#[test]
fn export_then_import_roundtrips_across_databases() {
    let src = TempDir::new().unwrap();
    let dst = TempDir::new().unwrap();
    let export = TempDir::new().unwrap();

    let target = create(&src, "Target", "# Target\nalvo", &["ref"]);
    create(&src, "Source", "links [[Target]]", &["a", "b"]);

    // export every note as markdown
    note(&src)
        .args(["export", export.path().to_str().unwrap(), "--format", "md"])
        .assert()
        .success();

    // import the whole directory into a fresh database
    let files: Vec<String> = std::fs::read_dir(export.path())
        .unwrap()
        .map(|e| e.unwrap().path().to_str().unwrap().to_owned())
        .collect();
    let mut import = note(&dst);
    import.arg("import");
    import.args(&files);
    import.assert().success();

    // the imported db must contain the same notes (ids preserved) ...
    note(&dst)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("notes:    2"));
    note(&dst)
        .args(["show", &target, "--json"])
        .assert()
        .success()
        .stdout(contains("\"ref\""))
        .stdout(contains("alvo"));
    // ... and the link graph re-resolves on import (json carries the full id)
    note(&dst)
        .args(["links", "Source", "--json"])
        .assert()
        .success()
        .stdout(contains(&target));
}

#[test]
fn import_is_idempotent_on_reimport() {
    let src = TempDir::new().unwrap();
    let export = TempDir::new().unwrap();
    create(&src, "One", "body", &[]);
    note(&src)
        .args(["export", export.path().to_str().unwrap()])
        .assert()
        .success();

    let file: String = std::fs::read_dir(export.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .to_str()
        .unwrap()
        .to_owned();

    // import twice into the same db: second time updates, never duplicates
    note(&src).args(["import", &file]).assert().success();
    note(&src)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("notes:    1"));
}

#[test]
fn export_json_format_writes_json_files() {
    let dir = TempDir::new().unwrap();
    let export = TempDir::new().unwrap();
    create(&dir, "JsonNote", "body", &[]);
    note(&dir)
        .args([
            "export",
            export.path().to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success();
    let has_json = std::fs::read_dir(export.path())
        .unwrap()
        .any(|e| e.unwrap().path().extension().is_some_and(|x| x == "json"));
    assert!(has_json, "expected a .json export file");
}

#[test]
fn tags_lists_all_with_counts() {
    let dir = TempDir::new().unwrap();
    create(&dir, "A", "x", &["lang", "fav"]);
    create(&dir, "B", "y", &["lang"]);
    note(&dir)
        .arg("tags")
        .assert()
        .success()
        .stdout(contains("lang  (2)"))
        .stdout(contains("fav  (1)"));
}

#[test]
fn list_filters_by_tag() {
    let dir = TempDir::new().unwrap();
    create(&dir, "Kept", "x", &["keep"]);
    create(&dir, "Dropped", "y", &["other"]);
    note(&dir)
        .args(["list", "--tag", "keep"])
        .assert()
        .success()
        .stdout(contains("Kept"))
        .stdout(contains("Dropped").not());
}

#[test]
fn tag_add_and_remove_modifies_note() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Note", "body", &["initial"]);
    note(&dir)
        .args(["tag", &id, "--add", "added", "--remove", "initial"])
        .assert()
        .success()
        .stdout(contains("added"))
        .stdout(contains("initial").not());
    // change is persisted
    note(&dir)
        .args(["show", &id, "--json"])
        .assert()
        .success()
        .stdout(contains("\"added\""))
        .stdout(contains("\"initial\"").not());
}

#[test]
fn tag_with_no_flags_shows_current_tags() {
    let dir = TempDir::new().unwrap();
    let id = create(&dir, "Note", "body", &["one", "two"]);
    note(&dir)
        .args(["tag", &id])
        .assert()
        .success()
        .stdout(contains("one"))
        .stdout(contains("two"));
}

#[test]
fn bare_invocation_without_a_tty_errors_cleanly() {
    // With no subcommand and no terminal (assert_cmd pipes stdio), the TUI can't
    // initialise; it must fail fast with a non-zero code, never hang.
    let dir = TempDir::new().unwrap();
    note(&dir).write_stdin("").assert().failure();
}

#[test]
fn invalid_tag_is_rejected() {
    let dir = TempDir::new().unwrap();
    note(&dir)
        .args(["new", "T", "-m", "body", "-t", "bad tag!"])
        .assert()
        .failure()
        .stderr(contains("invalid tag"));
}
