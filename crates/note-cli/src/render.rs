//! TTY-aware output. Styled markdown on a terminal, raw text when piped or
//! redirected, so `note show x > out.md` and `note show x | …` stay clean.

use std::borrow::Cow;
use std::io::IsTerminal;

/// How a note body should be emitted.
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    /// Auto-detect from whether stdout is a terminal.
    Auto,
    /// Always raw.
    Raw,
    /// Always styled.
    Styled,
}

impl Mode {
    /// Resolve from the `--raw` / `--render` flags.
    #[must_use]
    pub fn from_flags(raw: bool, render: bool) -> Self {
        match (raw, render) {
            (true, _) => Self::Raw,
            (_, true) => Self::Styled,
            _ => Self::Auto,
        }
    }

    fn styled(self) -> bool {
        match self {
            Self::Raw => false,
            Self::Styled => true,
            Self::Auto => std::io::stdout().is_terminal(),
        }
    }
}

/// Print a note body, styling markdown only when appropriate.
pub fn print_body(body: &str, mode: Mode) {
    if mode.styled() {
        termimad::MadSkin::default().print_text(body);
        return;
    }
    // Raw output straight to a terminal: neutralize control sequences that could
    // be embedded in imported note content (OSC clipboard writes, title/cursor
    // abuse). Piped/redirected output is left byte-exact so
    // `note show x --raw > out.md` stays clean.
    let out: Cow<'_, str> = if std::io::stdout().is_terminal() {
        Cow::Owned(sanitize_for_tty(body))
    } else {
        Cow::Borrowed(body)
    };
    print!("{out}");
    if !out.ends_with('\n') {
        println!();
    }
}

/// Replace C0/C1 control characters (and DEL) with U+FFFD, keeping only tab and
/// newline, so raw note content can't drive the terminal via escape sequences.
fn sanitize_for_tty(body: &str) -> String {
    body.chars()
        .map(|c| match c {
            '\t' | '\n' => c,
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize_for_tty;

    #[test]
    fn sanitize_strips_control_but_keeps_tab_and_newline() {
        let dirty = "a\x1b]0;pwn\x07b\tc\nd\x00e";
        let clean = sanitize_for_tty(dirty);
        assert!(!clean.contains('\x1b'), "ESC neutralized");
        assert!(!clean.contains('\x07'), "BEL neutralized");
        assert!(!clean.contains('\x00'), "NUL neutralized");
        assert!(clean.contains('\t'), "tab kept");
        assert!(clean.contains('\n'), "newline kept");
        assert_eq!(clean.matches('a').count(), 1);
        assert!(clean.contains('e'), "printable text preserved");
    }
}
