//! Optional `fzf` picker for ambiguous references. `fzf` is never a hard
//! dependency: if it is not on `$PATH`, the caller falls back to a numbered
//! list. The CLI never embeds `ratatui` for this.

use crate::config::Config;
use anyhow::Result;
use note_core::Note;
use std::io::Write;
use std::process::{Command, Stdio};

/// Outcome of attempting an interactive pick.
#[derive(Debug)]
pub enum Pick {
    /// The user selected a candidate.
    Chosen(Box<Note>),
    /// The user aborted the picker (e.g. pressed Esc).
    Aborted,
    /// `fzf` is not installed; the caller should fall back to a numbered list.
    NoFzf,
}

/// Launch `fzf` over the candidates with a first-lines preview. Each line is
/// `id<TAB>title`; the preview runs `note show <id> --raw` against the same
/// data dir so it never escapes the user's configured database.
pub fn pick(config: &Config, candidates: &[Note]) -> Result<Pick> {
    // If we can't locate our own binary for the preview command, degrade to the
    // numbered-list fallback rather than aborting the command.
    let Ok(exe) = std::env::current_exe() else {
        return Ok(Pick::NoFzf);
    };
    let preview = format!(
        "{} --data-dir {} show {{1}} --raw",
        shell_quote(&exe.to_string_lossy()),
        shell_quote(&config.data_dir.to_string_lossy()),
    );

    let mut child = match Command::new("fzf")
        .args([
            "--delimiter",
            "\t",
            "--with-nth",
            "2..",
            "--preview",
            &preview,
            "--preview-window",
            "right:60%",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Pick::NoFzf),
        Err(e) => return Err(e.into()),
    };

    {
        let mut stdin = child.stdin.take().expect("piped stdin");
        for note in candidates {
            let title = note.effective_title().replace(['\t', '\n'], " ");
            writeln!(stdin, "{}\t{title}", note.id)?;
        }
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Ok(Pick::Aborted);
    }
    let selection = String::from_utf8_lossy(&output.stdout);
    let id = selection.split('\t').next().unwrap_or("").trim();
    Ok(candidates
        .iter()
        .find(|n| n.id.to_string() == id)
        .cloned()
        .map_or(Pick::Aborted, |n| Pick::Chosen(Box::new(n))))
}

/// Minimal single-quote shell escaping for the preview command.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}
