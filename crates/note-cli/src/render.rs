//! TTY-aware output. Styled markdown on a terminal, raw text when piped or
//! redirected, so `note show x > out.md` and `note show x | …` stay clean.

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
    } else {
        print!("{body}");
        if !body.ends_with('\n') {
            println!();
        }
    }
}
