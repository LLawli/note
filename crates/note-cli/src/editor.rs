//! Interactive body capture via `$EDITOR` / `$VISUAL` (resolved in `Config`).

use crate::config::Config;
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::process::Command;

/// Open the configured editor on a temporary markdown file, pre-filled with
/// `initial`, and return the saved contents. Errors if no editor is configured.
pub fn capture_body(config: &Config, initial: &str) -> Result<String> {
    let editor = config
        .editor
        .as_deref()
        .context("no $EDITOR/$VISUAL set; pass --message or pipe the body on stdin")?;

    let mut file = tempfile::Builder::new()
        .prefix("note-")
        .suffix(".md")
        .tempfile()
        .context("creating temp file for editor")?;
    file.write_all(initial.as_bytes())
        .context("seeding editor buffer")?;
    file.flush().ok();
    let path = file.path().to_owned();

    // POSIX-style word splitting (honors quotes) so an editor whose path has a
    // space or that needs a quoted argument parses correctly, e.g.
    // `"/opt/My Editor/edit" --wait` — not mis-split like `split_whitespace`.
    let mut parts = shlex::split(editor)
        .with_context(|| format!("could not parse editor command {editor:?}"))?;
    if parts.is_empty() {
        bail!("empty editor command");
    }
    let program = parts.remove(0);
    let status = Command::new(&program)
        .args(&parts)
        .arg(&path)
        .status()
        .with_context(|| format!("launching editor {editor:?}"))?;
    if !status.success() {
        bail!("editor exited without saving");
    }

    std::fs::read_to_string(&path).context("reading edited note body")
}
