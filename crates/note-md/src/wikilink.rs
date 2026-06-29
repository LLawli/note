//! Extract `[[wikilinks]]` from a markdown body, ignoring code.

use note_core::WikiLink;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::collections::HashSet;
use std::ops::Range;

/// Extract the `[[wikilinks]]` from a markdown body in document order,
/// de-duplicated by their canonical form. Links inside fenced/indented code
/// blocks and inline code spans are ignored. Malformed targets (e.g. `[[]]`)
/// are skipped.
///
/// `pulldown-cmark` may split a `[[…]]` run across several text events, so we
/// scan the raw body for the spans and use the parser only to mask out the byte
/// ranges that fall inside code.
#[must_use]
pub fn extract_wikilinks(body: &str) -> Vec<WikiLink> {
    // Cheap substring scan first: a body with no `[[…]]` span (the common case on
    // most writes) skips the full pulldown-cmark parse that code_ranges runs.
    let spans = scan_double_brackets(body);
    if spans.is_empty() {
        return Vec::new();
    }
    let code = code_ranges(body);
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for (open, inner) in spans {
        if code.iter().any(|r| r.contains(&open)) {
            continue;
        }
        if let Ok(link) = inner.parse::<WikiLink>()
            && seen.insert(link.to_string())
        {
            out.push(link);
        }
    }
    out
}

/// Byte ranges of the body that are code (fenced/indented blocks and inline
/// spans), where `[[…]]` must NOT be treated as a wikilink.
fn code_ranges(body: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut depth = 0u32;
    for (event, range) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => depth += 1,
            Event::End(TagEnd::CodeBlock) => depth = depth.saturating_sub(1),
            Event::Code(_) => ranges.push(range),
            Event::Text(_) if depth > 0 => ranges.push(range),
            _ => {}
        }
    }
    ranges
}

/// Yield `(open_offset, inner)` for each `[[ … ]]` span (non-nesting, L→R).
fn scan_double_brackets(body: &str) -> Vec<(usize, &str)> {
    let mut spans = Vec::new();
    let mut idx = 0;
    while let Some(rel) = body[idx..].find("[[") {
        let open = idx + rel;
        let after = open + 2;
        let Some(crel) = body[after..].find("]]") else {
            break;
        };
        let close = after + crel;
        spans.push((open, &body[after..close]));
        idx = close + 2;
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use note_core::WikiTarget;

    const ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn forms(links: &[WikiLink]) -> Vec<String> {
        links.iter().map(WikiLink::to_string).collect()
    }

    #[test]
    fn extracts_single_title() {
        let links = extract_wikilinks("see [[Some Note]] here");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, WikiTarget::ByTitle("Some Note".to_owned()));
        assert!(links[0].display.is_none());
    }

    #[test]
    fn extracts_id_target() {
        let links = extract_wikilinks(&format!("link to [[{ULID}]]"));
        assert_eq!(links.len(), 1);
        assert!(matches!(links[0].target, WikiTarget::ById(_)));
    }

    #[test]
    fn extracts_alias_form() {
        let links = extract_wikilinks("[[Target|click here]]");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, WikiTarget::ByTitle("Target".to_owned()));
        assert_eq!(links[0].display.as_deref(), Some("click here"));
    }

    #[test]
    fn extracts_multiple_in_order() {
        let links = extract_wikilinks("[[A]] then [[B]] then [[C]]");
        assert_eq!(forms(&links), vec!["A", "B", "C"]);
    }

    #[test]
    fn dedupes_repeated_links() {
        let links = extract_wikilinks("[[A]] and again [[A]] and [[A|alias]]");
        assert_eq!(forms(&links), vec!["A", "A|alias"]);
    }

    #[test]
    fn ignores_fenced_code_block() {
        let body = "real [[Linked]]\n\n```\ncode [[NotLinked]]\n```\n";
        assert_eq!(forms(&extract_wikilinks(body)), vec!["Linked"]);
    }

    #[test]
    fn ignores_inline_code() {
        let links = extract_wikilinks("text `[[NotLinked]]` and [[Linked]]");
        assert_eq!(forms(&links), vec!["Linked"]);
    }

    #[test]
    fn skips_empty_target() {
        assert!(extract_wikilinks("empty [[]] and [[   ]] here").is_empty());
    }

    #[test]
    fn handles_unclosed_brackets() {
        assert!(extract_wikilinks("dangling [[ open").is_empty());
    }

    #[test]
    fn finds_links_across_lines() {
        let links = extract_wikilinks("first [[A]]\nsecond line [[B]]");
        assert_eq!(forms(&links), vec!["A", "B"]);
    }

    #[test]
    fn no_links_in_plain_text() {
        assert!(extract_wikilinks("just some prose, no links").is_empty());
    }
}
