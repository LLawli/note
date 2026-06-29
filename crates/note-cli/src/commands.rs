//! Subcommand implementations. Front-end only: parse input, drive `note-store`,
//! render output. No business rules live here.

use crate::cli::{
    Cli, Command, DeleteArgs, EditArgs, ExportArgs, Format, ImportArgs, LinksArgs, ListArgs,
    NewArgs, SearchArgs, ShowArgs, StatusArgs, TagArgs, TagsArgs,
};
use crate::config::Config;
use crate::editor;
use crate::picker::{self, Pick};
use crate::render::{self, Mode};
use anyhow::{Context, Result, bail};
use note_core::{ContentKind, Note, NoteId, Tag};
use note_store::{ImportNote, ImportOutcome, NewNote, NotePatch, Store};
use std::collections::BTreeSet;
use std::io::{IsTerminal, Read, Write};
use std::path::Path;
use std::process::ExitCode;

/// Parse-time entry point: resolve config, open the store, run the subcommand
/// (or launch the TUI when invoked bare).
pub fn dispatch(cli: Cli) -> Result<ExitCode> {
    let config = Config::load(cli.data_dir)?;
    let store = Store::open(config.db_path())
        .with_context(|| format!("opening database at {}", config.db_path().display()))?;

    let Some(command) = cli.command else {
        return run_tui(&store, &config);
    };

    match command {
        Command::New(args) => cmd_new(&store, &config, args),
        Command::Edit(args) => cmd_edit(&store, &config, args),
        Command::Delete(args) => cmd_delete(&store, &config, &args),
        Command::Show(args) => cmd_show(&store, &config, &args),
        Command::Links(args) => cmd_links(&store, &config, &args),
        Command::Search(args) => cmd_search(&store, &args),
        Command::List(args) => cmd_list(&store, &args),
        Command::Tags(args) => cmd_tags(&store, &args),
        Command::Tag(args) => cmd_tag(&store, &config, &args),
        Command::Import(args) => cmd_import(&store, &args),
        Command::Export(args) => cmd_export(&store, &args),
        Command::Status(args) => cmd_status(&store, &config, &args),
    }
}

fn cmd_import(store: &Store, args: &ImportArgs) -> Result<ExitCode> {
    let (mut created, mut updated, mut failed) = (0u32, 0u32, 0u32);
    let mut imported: Vec<(NoteId, Vec<note_core::WikiLink>)> = Vec::new();
    for path in &args.paths {
        match import_one(store, path) {
            Ok((outcome, id, links)) => {
                match outcome {
                    ImportOutcome::Created => created += 1,
                    ImportOutcome::Updated => updated += 1,
                }
                imported.push((id, links));
            }
            Err(err) => {
                failed += 1;
                eprintln!("skip {}: {err:#}", path.display());
            }
        }
    }

    // Second pass: re-resolve links now that every imported note exists, so
    // forward references between files in the batch are no longer left dangling
    // (first-pass resolution can't see notes imported later).
    for (id, links) in imported {
        store
            .writer()
            .replace_links(id, links)
            .context("resolving imported links")?;
    }

    if args.json {
        let report =
            serde_json::json!({ "created": created, "updated": updated, "failed": failed });
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        eprintln!("imported {created} new, {updated} updated, {failed} failed");
    }
    Ok(if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Import a single file (atomic via the store's per-note transaction).
/// Idempotent: a file carrying a known id updates in place. Returns the note id
/// and its extracted links so the caller can re-resolve them after the whole
/// batch is in (forward references can't resolve on the first pass).
fn import_one(
    store: &Store,
    path: &Path,
) -> Result<(ImportOutcome, NoteId, Vec<note_core::WikiLink>)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let is_json = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));

    let mut req = if is_json {
        let note = note_md::from_json(&text)?;
        ImportNote {
            id: Some(note.id),
            title: note.title,
            content_kind: note.content_kind,
            tags: note.tags,
            created: Some(note.created),
            updated: Some(note.updated),
            links: note_md::extract_wikilinks(&note.body),
            body: note.body,
        }
    } else {
        let imported = note_md::from_markdown(&text)?;
        ImportNote {
            id: imported.id,
            title: imported.title,
            content_kind: imported.content_kind,
            tags: imported.tags,
            created: imported.created,
            updated: imported.updated,
            links: note_md::extract_wikilinks(&imported.body),
            body: imported.body,
        }
    };

    // Normalize the title (trim, empty -> None) like cmd_new, so an explicit
    // empty-string title can't slip a junk note past the empty-note guard.
    req.title = req
        .title
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty());

    // Mirror cmd_new/edit: don't import junk empties (id-less markdown would also
    // mint a fresh id on every re-import, accumulating duplicates).
    if req.body.trim().is_empty() && req.title.is_none() {
        bail!("refusing to import an empty note: {}", path.display());
    }

    let links = req.links.clone();
    let (note, outcome) = store.writer().import_note(req)?;
    Ok((outcome, note.id, links))
}

fn cmd_export(store: &Store, args: &ExportArgs) -> Result<ExitCode> {
    std::fs::create_dir_all(&args.dir)
        .with_context(|| format!("creating export dir {}", args.dir.display()))?;
    let notes = store.readers().all_notes()?;
    let ext = match args.format {
        Format::Md => "md",
        Format::Json => "json",
    };
    for note in &notes {
        let content = match args.format {
            Format::Md => note_md::to_markdown(note),
            Format::Json => note_md::to_json(note)?,
        };
        let path = args.dir.join(format!("{}.{ext}", note.id));
        atomic_write(&path, &content)?;
    }
    eprintln!("exported {} notes to {}", notes.len(), args.dir.display());
    Ok(ExitCode::SUCCESS)
}

/// Write `content` to `path` atomically: a sibling temp file, then rename.
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, content).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("finalizing {}", path.display()))?;
    Ok(())
}

/// Run the interactive TUI, re-entering it after each in-app edit request so the
/// user can edit a note in `$EDITOR` and return to browsing.
fn run_tui(store: &Store, config: &Config) -> Result<ExitCode> {
    loop {
        match note_tui::run(store).context("running the TUI")? {
            note_tui::Outcome::Quit => return Ok(ExitCode::SUCCESS),
            note_tui::Outcome::Edit(id) => edit_note_in_editor(store, config, id)?,
        }
    }
}

/// Edit a note's body in `$EDITOR` (used by the TUI's edit request). Failures
/// are reported but never abort the TUI session.
fn edit_note_in_editor(store: &Store, config: &Config, id: NoteId) -> Result<()> {
    let Some(note) = store.readers().get_note(id)? else {
        return Ok(());
    };
    let body = match editor::capture_body(config, &note.body) {
        Ok(body) => body.trim_end().to_owned(),
        Err(err) => {
            eprintln!("edit cancelled: {err:#}");
            return Ok(());
        }
    };
    // Re-read after the editor returns so a concurrent metadata change (e.g.
    // `note tag` in another terminal while $EDITOR was open) is not clobbered;
    // only the body comes from the edit.
    let Some(fresh) = store.readers().get_note(id)? else {
        return Ok(());
    };
    if body.is_empty() && fresh.title.is_none() {
        eprintln!("edit cancelled: refusing to leave an empty note");
        return Ok(());
    }
    let patch = NotePatch {
        title: fresh.title,
        content_kind: fresh.content_kind,
        tags: fresh.tags,
        links: note_md::extract_wikilinks(&body),
        body,
    };
    store.writer().update_note(id, patch)?;
    Ok(())
}

fn cmd_new(store: &Store, config: &Config, args: NewArgs) -> Result<ExitCode> {
    let tags = parse_tags(&args.tags)?;
    let body = resolve_body(&args, config)?;
    let body = body.trim_end().to_owned();
    let title = args
        .title
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty());

    if body.is_empty() && title.is_none() {
        bail!("refusing to create an empty note (give a title, --message, or a body)");
    }

    let kind = if args.plain {
        ContentKind::Plain
    } else {
        ContentKind::Markdown
    };
    let note = store
        .writer()
        .create_note(NewNote {
            title,
            links: note_md::extract_wikilinks(&body),
            body,
            content_kind: kind,
            tags,
        })
        .context("creating note")?;

    if args.json {
        println!("{}", to_json(&note)?);
    } else {
        // id on stdout (scriptable: id=$(note new ...)); human line on stderr.
        println!("{}", note.id);
        eprintln!("created {}", note.display_title());
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_delete(store: &Store, config: &Config, args: &DeleteArgs) -> Result<ExitCode> {
    let note = match resolve_required(store, config, &args.reference)? {
        Resolved::Found(note) => *note,
        Resolved::Aborted => return Ok(ExitCode::SUCCESS),
        Resolved::Ambiguous => return Ok(ExitCode::FAILURE),
    };

    if !args.yes && !confirm_delete(&note)? {
        eprintln!("aborted");
        return Ok(ExitCode::SUCCESS);
    }

    if !store.writer().delete_note(note.id)? {
        bail!("note vanished before it could be deleted");
    }

    if args.json {
        println!("{}", serde_json::json!({ "deleted": note.id.to_string() }));
    } else {
        println!("{}", note.id);
        eprintln!("deleted {}", note.display_title());
    }
    Ok(ExitCode::SUCCESS)
}

/// Confirm a destructive delete. Prompts on a terminal; refuses (rather than
/// silently deleting) when stdin is not a terminal and `--yes` was not given.
fn confirm_delete(note: &Note) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("refusing to delete without --yes when stdin is not a terminal");
    }
    eprint!("delete {}? [y/N] ", note.display_title());
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn cmd_show(store: &Store, config: &Config, args: &ShowArgs) -> Result<ExitCode> {
    let note = match &args.reference {
        None => match store.readers().most_recent()? {
            Some(note) => note,
            None => bail!("no notes yet"),
        },
        Some(reference) => match resolve_required(store, config, reference)? {
            Resolved::Found(note) => *note,
            Resolved::Aborted => return Ok(ExitCode::SUCCESS),
            Resolved::Ambiguous => return Ok(ExitCode::FAILURE),
        },
    };

    if args.json {
        println!("{}", to_json(&note)?);
    } else {
        render::print_body(&note.body, Mode::from_flags(args.raw, args.render));
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_edit(store: &Store, config: &Config, args: EditArgs) -> Result<ExitCode> {
    let current = match &args.reference {
        None => store.readers().most_recent()?.context("no notes yet")?,
        Some(reference) => match resolve_required(store, config, reference)? {
            Resolved::Found(note) => *note,
            Resolved::Aborted => return Ok(ExitCode::SUCCESS),
            Resolved::Ambiguous => return Ok(ExitCode::FAILURE),
        },
    };

    let metadata_only =
        args.title.is_some() || !args.tags.is_empty() || args.plain || args.markdown;

    // Body source: --message, then (for metadata-only edits) keep the body,
    // then piped stdin, then the editor pre-filled with the current body.
    let body = if let Some(message) = &args.message {
        message.clone()
    } else if metadata_only {
        current.body.clone()
    } else if !std::io::stdin().is_terminal() {
        read_stdin()?
    } else {
        editor::capture_body(config, &current.body)?
    };
    let body = body.trim_end().to_owned();

    let title = match args.title {
        Some(t) => {
            let t = t.trim().to_owned();
            if t.is_empty() { None } else { Some(t) }
        }
        None => current.title.clone(),
    };

    let content_kind = if args.plain {
        ContentKind::Plain
    } else if args.markdown {
        ContentKind::Markdown
    } else {
        current.content_kind
    };

    let tags = if args.tags.is_empty() {
        current.tags.clone()
    } else {
        parse_tags(&args.tags)?
    };

    if body.is_empty() && title.is_none() {
        bail!("refusing to leave an empty note (give a title, --message, or a body)");
    }

    let updated = store
        .writer()
        .update_note(
            current.id,
            NotePatch {
                title,
                content_kind,
                tags,
                links: note_md::extract_wikilinks(&body),
                body,
            },
        )
        .context("updating note")?
        .context("note vanished before it could be updated")?;

    if args.json {
        println!("{}", to_json(&updated)?);
    } else {
        println!("{}", updated.id);
        eprintln!("updated {}", updated.display_title());
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_links(store: &Store, config: &Config, args: &LinksArgs) -> Result<ExitCode> {
    let note = match &args.reference {
        None => store.readers().most_recent()?.context("no notes yet")?,
        Some(reference) => match resolve_required(store, config, reference)? {
            Resolved::Found(note) => *note,
            Resolved::Aborted => return Ok(ExitCode::SUCCESS),
            Resolved::Ambiguous => return Ok(ExitCode::FAILURE),
        },
    };
    let links = store.readers().links_for(note.id)?;

    if args.json {
        let arr: Vec<_> = links
            .iter()
            .map(|l| {
                serde_json::json!({
                    "target": l.target.to_string(),
                    "display": l.display,
                    "resolved": l.resolved.map(|id| id.to_string()),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else if links.is_empty() {
        eprintln!("no links");
    } else {
        for link in &links {
            let target = match &link.display {
                Some(d) => format!("[[{}|{d}]]", link.target),
                None => format!("[[{}]]", link.target),
            };
            let status = link.resolved.map_or_else(
                || "(dangling)".to_owned(),
                |id| id.to_string().chars().take(10).collect(),
            );
            println!("{target}  ->  {status}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_search(store: &Store, args: &SearchArgs) -> Result<ExitCode> {
    let hits = store.readers().search_prefix(&args.query, args.limit)?;
    if args.json {
        println!("{}", to_json(&hits)?);
    } else if hits.is_empty() {
        eprintln!("no matches for {:?}", args.query);
    } else {
        print_list(&hits);
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_list(store: &Store, args: &ListArgs) -> Result<ExitCode> {
    let notes = match &args.tag {
        Some(raw) => {
            let tag = Tag::new(raw).with_context(|| format!("invalid tag {raw:?}"))?;
            store.readers().list_by_tag(&tag, args.limit)?
        }
        None => store.readers().list_notes(args.limit, 0)?,
    };
    if args.json {
        println!("{}", to_json(&notes)?);
    } else if notes.is_empty() {
        eprintln!("no notes yet");
    } else {
        print_list(&notes);
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_tags(store: &Store, args: &TagsArgs) -> Result<ExitCode> {
    let tags = store.readers().all_tags()?;
    if args.json {
        let arr: Vec<_> = tags
            .iter()
            .map(|(tag, count)| serde_json::json!({ "tag": tag.as_str(), "count": count }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else if tags.is_empty() {
        eprintln!("no tags yet");
    } else {
        for (tag, count) in &tags {
            println!("{tag}  ({count})");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_tag(store: &Store, config: &Config, args: &TagArgs) -> Result<ExitCode> {
    let current = match &args.reference {
        None => store.readers().most_recent()?.context("no notes yet")?,
        Some(reference) => match resolve_required(store, config, reference)? {
            Resolved::Found(note) => *note,
            Resolved::Aborted => return Ok(ExitCode::SUCCESS),
            Resolved::Ambiguous => return Ok(ExitCode::FAILURE),
        },
    };

    let note = if args.add.is_empty() && args.remove.is_empty() {
        current // read-only view of the current tags
    } else {
        let add = parse_tags(&args.add)?;
        let remove = parse_tags(&args.remove)?;
        let mut tags = current.tags.clone();
        tags.extend(add);
        tags.retain(|t| !remove.contains(t));
        store
            .writer()
            .update_note(
                current.id,
                NotePatch {
                    title: current.title.clone(),
                    content_kind: current.content_kind,
                    tags,
                    // body is unchanged; carry its links so update doesn't clear
                    // the note's outgoing link graph.
                    links: note_md::extract_wikilinks(&current.body),
                    body: current.body.clone(),
                },
            )
            .context("updating tags")?
            .context("note vanished before its tags could be updated")?
    };

    if args.json {
        let tags: Vec<&str> = note.tags.iter().map(Tag::as_str).collect();
        println!("{}", serde_json::to_string_pretty(&tags)?);
    } else if note.tags.is_empty() {
        eprintln!("(no tags)");
    } else {
        for tag in &note.tags {
            println!("{tag}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_status(store: &Store, config: &Config, args: &StatusArgs) -> Result<ExitCode> {
    let count = store.readers().count_notes()?;
    if args.json {
        let report = serde_json::json!({
            "notes": count,
            "data_dir": config.data_dir,
            "database": config.db_path(),
        });
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("notes:    {count}");
        println!("data dir: {}", config.data_dir.display());
        println!("database: {}", config.db_path().display());
    }
    Ok(ExitCode::SUCCESS)
}

/// Outcome of resolving a user reference to a single note.
enum Resolved {
    /// Exactly one note (direct match, or picked from an ambiguous set).
    Found(Box<Note>),
    /// Ambiguous and not selected (a numbered list was printed): exit non-zero.
    Ambiguous,
    /// The user deliberately aborted the interactive picker: a clean no-op.
    Aborted,
}

/// Resolve an ambiguous match to one note via the fzf picker (on a TTY) or a
/// numbered list otherwise.
fn choose(config: &Config, candidates: &[Note]) -> Result<Resolved> {
    if std::io::stdout().is_terminal() {
        match picker::pick(config, candidates)? {
            Pick::Chosen(note) => return Ok(Resolved::Found(note)),
            Pick::Aborted => return Ok(Resolved::Aborted),
            Pick::NoFzf => {}
        }
    }
    print_numbered(candidates);
    Ok(Resolved::Ambiguous)
}

/// Resolve a required reference to exactly one note. Bails when nothing matches;
/// otherwise returns one note, or an `Ambiguous`/`Aborted` outcome for the caller
/// to turn into the right exit code.
fn resolve_required(store: &Store, config: &Config, reference: &str) -> Result<Resolved> {
    let mut candidates = store.readers().resolve_ref(reference)?;
    match candidates.len() {
        0 => bail!("no note matches {reference:?}"),
        1 => Ok(Resolved::Found(Box::new(candidates.remove(0)))),
        _ => choose(config, &candidates),
    }
}

fn resolve_body(args: &NewArgs, config: &Config) -> Result<String> {
    if let Some(message) = &args.message {
        return Ok(message.clone());
    }
    if !std::io::stdin().is_terminal() {
        return read_stdin();
    }
    editor::capture_body(config, "")
}

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading body from stdin")?;
    Ok(buf)
}

fn parse_tags(raw: &[String]) -> Result<BTreeSet<Tag>> {
    raw.iter()
        .map(|t| Tag::new(t).with_context(|| format!("invalid tag {t:?}")))
        .collect()
}

fn print_list(notes: &[Note]) {
    for note in notes {
        println!("{}  {}", short_id(note), note.display_title());
    }
}

fn print_numbered(candidates: &[Note]) {
    eprintln!("ambiguous reference; {} matches:", candidates.len());
    for (i, note) in candidates.iter().enumerate() {
        eprintln!(
            "  {:>2}. {}  {}",
            i + 1,
            short_id(note),
            note.display_title()
        );
    }
    eprintln!("refine the reference, use a longer id prefix, or pick one above.");
}

fn short_id(note: &Note) -> String {
    note.id.to_string().chars().take(10).collect()
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).context("serializing JSON")
}
