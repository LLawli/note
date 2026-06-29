use crate::content::ContentKind;

/// Derive the effective (display) title, in priority order:
///
/// 1. an explicit, non-empty (trimmed) title;
/// 2. for Markdown, the first ATX H1 (a line whose trimmed form is `#` + whitespace + text);
/// 3. otherwise the first non-empty line, trimmed;
/// 4. otherwise an empty string.
///
/// The H1 rule is DELIBERATELY trivial: no fenced-code awareness, no setext, no
/// closing-hash trimming. Real CommonMark parsing is note-md/M3 and may refine it.
#[must_use]
pub fn derive_title(title: Option<&str>, body: &str, kind: ContentKind) -> String {
    if let Some(t) = title {
        let t = t.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if kind.is_markdown()
        && let Some(h1) = first_h1(body)
    {
        return h1;
    }
    first_non_empty_line(body).unwrap_or_default()
}

fn first_h1(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let rest = line.trim().strip_prefix('#')?; // exactly one leading '#'
        // '#' must be followed by whitespace ("## Sub", "#NoSpace" are not H1),
        // and the trimmed heading text must be non-empty.
        rest.starts_with(char::is_whitespace)
            .then(|| rest.trim())
            .filter(|h| !h.is_empty())
            .map(str::to_string)
    })
}

fn first_non_empty_line(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ContentKind::{Markdown, Plain};

    #[test]
    fn title_explicit_wins() {
        assert_eq!(
            derive_title(Some("Explicit"), "# Other", Markdown),
            "Explicit"
        );
    }

    #[test]
    fn title_empty_explicit_falls_through() {
        assert_eq!(
            derive_title(Some("   "), "# Body Title", Markdown),
            "Body Title"
        );
    }

    #[test]
    fn title_md_h1_atx() {
        assert_eq!(derive_title(None, "# Hello\nbody", Markdown), "Hello");
    }

    #[test]
    fn title_md_h1_trims_inner_space() {
        assert_eq!(derive_title(None, "#   Spaced   \n", Markdown), "Spaced");
    }

    #[test]
    fn title_md_h1_requires_space() {
        assert_eq!(derive_title(None, "#NoSpace\n", Markdown), "#NoSpace");
    }

    #[test]
    fn title_md_only_level_one() {
        assert_eq!(derive_title(None, "## Sub\n# Real", Markdown), "Real");
        assert_eq!(derive_title(None, "## OnlySub", Markdown), "## OnlySub");
    }

    #[test]
    fn title_first_nonempty_line() {
        assert_eq!(
            derive_title(None, "plain para\nmore", Markdown),
            "plain para"
        );
    }

    #[test]
    fn title_skips_leading_blanks() {
        assert_eq!(derive_title(None, "\n\n  \n# Title", Markdown), "Title");
        assert_eq!(
            derive_title(None, "\n\n \nFirst real", Markdown),
            "First real"
        );
    }

    #[test]
    fn title_handles_crlf() {
        assert_eq!(derive_title(None, "# Title\r\nbody", Markdown), "Title");
    }

    #[test]
    fn title_plain_kind_ignores_hash() {
        assert_eq!(
            derive_title(None, "# not a heading", Plain),
            "# not a heading"
        );
    }

    #[test]
    fn title_empty_body() {
        assert_eq!(derive_title(None, "", Markdown), "");
        assert_eq!(derive_title(None, "", Plain), "");
    }

    #[test]
    fn title_whitespace_only_body() {
        assert_eq!(derive_title(None, "  \n\t\n", Markdown), "");
    }
}
