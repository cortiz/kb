//! `[[entry-id]]` references inside a note's Markdown.

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::entry::EntryType;

/// One `[[target]]` in a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryLink {
    /// The entry ID inside the brackets.
    pub target_id: String,
    /// Where the whole `[[...]]` is, in bytes of the text it was extracted from.
    pub span: Range<usize>,
    /// Filled by [`EntryLink::resolve`]; `None` when the target does not exist.
    pub resolved_type: Option<EntryType>,
    pub resolved_title: Option<String>,
}

impl EntryLink {
    pub fn is_broken(&self) -> bool {
        self.resolved_type.is_none()
    }

    /// Looks the target up: `lookup(id)` gives its type and title when it exists.
    pub fn resolve(&mut self, lookup: &dyn Fn(&str) -> Option<(EntryType, String)>) {
        if let Some((ty, title)) = lookup(&self.target_id) {
            self.resolved_type = Some(ty);
            self.resolved_title = Some(title);
        }
    }
}

/// Byte ranges where `[[` is not a link: code spans, fenced and indented code.
fn code_ranges(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut block_start = None;
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Code(_) => out.push(range),
            Event::Start(Tag::CodeBlock(_)) => block_start = Some(range.start),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = block_start.take() {
                    out.push(start..range.end);
                }
            }
            _ => {}
        }
    }
    out
}

fn is_target(id: &str) -> bool {
    crate::search::valid_id(id)
        && !id.contains(|c: char| c.is_whitespace() || c.is_control() || "[]|".contains(c))
}

/// Every `[[id]]` in `text`, in order. `\[[` is literal text, and nothing inside code counts.
pub fn extract_links(text: &str) -> Vec<EntryLink> {
    if !text.contains("[[") {
        return Vec::new();
    }
    let code = code_ranges(text);
    let in_code = |at: usize| code.iter().any(|r| r.contains(&at));
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find("[[") {
        let open = from + i;
        from = open + 2;
        if text[..open].ends_with('\\') || in_code(open) {
            continue;
        }
        let rest = &text[open + 2..];
        let Some(close) = rest.find("]]") else {
            break;
        };
        let target = &rest[..close];
        if !is_target(target) {
            continue;
        }
        let end = open + 2 + close + 2;
        out.push(EntryLink {
            target_id: target.to_string(),
            span: open..end,
            resolved_type: None,
            resolved_title: None,
        });
        from = end;
    }
    out
}

/// `text` with every link to `old` pointing at `new`; everything else is untouched.
/// `None` when nothing links to `old`.
pub fn retarget(text: &str, old: &str, new: &str) -> Option<String> {
    let links: Vec<_> = extract_links(text)
        .into_iter()
        .filter(|l| l.target_id == old)
        .collect();
    if links.is_empty() {
        return None;
    }
    let mut out = text.to_string();
    for link in links.iter().rev() {
        out.replace_range(link.span.clone(), &format!("[[{new}]]"));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(text: &str) -> Vec<String> {
        extract_links(text)
            .into_iter()
            .map(|l| l.target_id)
            .collect()
    }

    #[test]
    fn finds_links_with_their_spans() {
        let text = "See [[sumo/5xx]] and [[cmd/a-b]].";
        let links = extract_links(text);
        assert_eq!(ids(text), ["sumo/5xx", "cmd/a-b"]);
        assert_eq!(&text[links[0].span.clone()], "[[sumo/5xx]]");
        assert!(links[0].is_broken());
    }

    #[test]
    fn escaped_and_code_brackets_are_literal() {
        assert!(ids("a \\[[x/y]] b").is_empty());
        assert!(ids("`[[x/y]]`").is_empty());
        assert!(ids("```\n[[x/y]]\n```\n").is_empty());
        assert!(ids("    [[x/y]]\n").is_empty());
        assert_eq!(ids("`a` [[x/y]]"), ["x/y"]);
    }

    #[test]
    fn malformed_links_are_ignored() {
        for text in [
            "[[]]",
            "[[ a b ]]",
            "[[a|b]]",
            "[[/abs]]",
            "[[a//b]]",
            "[[open",
            "[[a\nb]]",
        ] {
            assert!(ids(text).is_empty(), "{text:?}");
        }
        assert_eq!(ids("[[a]] [[a]]"), ["a", "a"]);
    }

    #[test]
    fn resolving_fills_type_and_title() {
        let mut link = extract_links("[[sumo/x]]").remove(0);
        link.resolve(&|id| (id == "sumo/x").then(|| (EntryType::Sumo, "X".to_string())));
        assert!(!link.is_broken());
        assert_eq!(link.resolved_title.as_deref(), Some("X"));
    }

    #[test]
    fn retarget_changes_only_matching_links() {
        let text = "[[old/id]] and [[other/id]] and `[[old/id]]` and [[old/id]].\n";
        assert_eq!(
            retarget(text, "old/id", "new/id").unwrap(),
            "[[new/id]] and [[other/id]] and `[[old/id]]` and [[new/id]].\n"
        );
        assert_eq!(retarget("[[a/b]]", "x/y", "z"), None);
    }
}
