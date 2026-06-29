//! clap command surface for the `note` binary. Keep this in sync with the
//! README/docs and the CLI-surface regression tests (workflow rule 6).

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "note", version, about = "Terminal note-taking (CLI + TUI)")]
pub struct Cli {
    /// Override the data directory (also via `NOTE_DATA_DIR`).
    #[arg(long, global = true, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a new note (body from --message, piped stdin, or $EDITOR).
    New(NewArgs),
    /// Edit an existing note (body in $EDITOR, or change fields via flags).
    Edit(EditArgs),
    /// Delete a note, resolving a reference (asks to confirm on a terminal).
    #[command(visible_alias = "rm")]
    Delete(DeleteArgs),
    /// Render a note, resolving a reference (no reference = most recent).
    Show(ShowArgs),
    /// List the outgoing `[[wikilinks]]` of a note.
    Links(LinksArgs),
    /// Full-text search over notes.
    Search(SearchArgs),
    /// List notes, most recently updated first (optionally filtered by tag).
    List(ListArgs),
    /// List all tags with their note counts.
    Tags(TagsArgs),
    /// Show or modify a note's tags (`--add` / `--remove`).
    Tag(TagArgs),
    /// Import notes from `.md` / `.json` files (idempotent on note id).
    Import(ImportArgs),
    /// Export every note to a directory as `.md` or `.json` files.
    Export(ExportArgs),
    /// Show database status.
    Status(StatusArgs),
}

/// Output format for export.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Format {
    Md,
    Json,
}

#[derive(Debug, Args)]
pub struct NewArgs {
    /// Optional explicit title (otherwise derived from the body).
    pub title: Option<String>,
    /// Body text. Falls back to piped stdin, then $EDITOR on a terminal.
    #[arg(short = 'm', long)]
    pub message: Option<String>,
    /// Attach a tag (repeatable).
    #[arg(short = 't', long = "tag", value_name = "TAG")]
    pub tags: Vec<String>,
    /// Store as plain text instead of markdown.
    #[arg(long)]
    pub plain: bool,
    /// Print the created note as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct EditArgs {
    /// ULID, short id prefix, title, or search terms. Omit for the most recent.
    pub reference: Option<String>,
    /// New body. Falls back to piped stdin, then $EDITOR pre-filled with the
    /// current body. Skipped when only metadata flags are given.
    #[arg(short = 'm', long)]
    pub message: Option<String>,
    /// Replace the title (an empty value clears it; omit to keep the current).
    #[arg(long)]
    pub title: Option<String>,
    /// Replace the entire tag set (repeatable; omit to keep the current tags).
    #[arg(short = 't', long = "tag", value_name = "TAG")]
    pub tags: Vec<String>,
    /// Switch the note to plain text.
    #[arg(long, conflicts_with = "markdown")]
    pub plain: bool,
    /// Switch the note to markdown.
    #[arg(long)]
    pub markdown: bool,
    /// Print the updated note as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// ULID, short id prefix, title, or search terms (required).
    pub reference: String,
    /// Delete without confirmation (required when stdin is not a terminal).
    #[arg(short = 'y', long, visible_alias = "force")]
    pub yes: bool,
    /// Print the deleted id as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    /// ULID, short id prefix, title, or search terms. Omit for the most recent.
    pub reference: Option<String>,
    /// Force raw output (no markdown styling).
    #[arg(long, conflicts_with = "render")]
    pub raw: bool,
    /// Force styled markdown output even when piped.
    #[arg(long)]
    pub render: bool,
    /// Print the note as JSON.
    #[arg(long, conflicts_with_all = ["raw", "render"])]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct LinksArgs {
    /// ULID, short id prefix, title, or search terms. Omit for the most recent.
    pub reference: Option<String>,
    /// Print the links as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Full-text query.
    pub query: String,
    /// Maximum number of results.
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: usize,
    /// Print results as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Only list notes carrying this tag.
    #[arg(short = 't', long, value_name = "TAG")]
    pub tag: Option<String>,
    /// Maximum number of notes.
    #[arg(short = 'n', long, default_value_t = 50)]
    pub limit: usize,
    /// Print results as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TagsArgs {
    /// Print tags as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TagArgs {
    /// ULID, short id prefix, title, or search terms. Omit for the most recent.
    pub reference: Option<String>,
    /// Tag to add (repeatable).
    #[arg(short = 'a', long = "add", value_name = "TAG")]
    pub add: Vec<String>,
    /// Tag to remove (repeatable).
    #[arg(short = 'r', long = "remove", value_name = "TAG")]
    pub remove: Vec<String>,
    /// Print the note's tags as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// One or more `.md` / `.json` files to import.
    #[arg(required = true, value_name = "FILE")]
    pub paths: Vec<PathBuf>,
    /// Print a JSON summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Destination directory (created if needed).
    pub dir: PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Md)]
    pub format: Format,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Print status as JSON.
    #[arg(long)]
    pub json: bool,
}
