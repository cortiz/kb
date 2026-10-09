//! The Markdown body: the payload (first fenced code block) and the notes.

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

use crate::source::Span;

/// The payload of a `sql`, `sumo`, `cw`, `cmd` or `prompt` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    /// The block's content, with `\{{` turned into `{{` and LF line endings.
    pub text: String,
    /// The content bytes in the file (absolute).
    pub raw_span: Span,
    /// The whole block including its fences (absolute).
    pub fence_span: Span,
    /// The info string, for example `sql`.
    pub info: Option<String>,
}

/// A parsed body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    pub payload: Option<Payload>,
    pub notes: String,
}

/// Parses the body found at `span` of `text`. When `wants_payload` is false (for `link` and
/// `code`), the whole body is notes.
pub fn parse_body(text: &str, span: Span, wants_payload: bool) -> Body {
    let body = &text[span.clone()];
    if !wants_payload {
        return Body {
            payload: None,
            notes: body.to_string(),
        };
    }
    let mut fence: Option<(Span, Option<String>)> = None;
    let mut content = String::new();
    let mut content_span: Option<Span> = None;
    for (event, range) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) if fence.is_none() => {
                let info = info.trim();
                fence = Some((range, (!info.is_empty()).then(|| info.to_string())));
            }
            Event::Text(t) if fence.is_some() => {
                content.push_str(&t);
                content_span = Some(match content_span {
                    Some(s) => s.start..range.end,
                    None => range,
                });
            }
            Event::End(TagEnd::CodeBlock) if fence.is_some() => break,
            _ => {}
        }
    }
    let Some((mut fence_span, info)) = fence else {
        return Body {
            payload: None,
            notes: body.to_string(),
        };
    };
    // Include the closing fence's line ending, so removing the block leaves the notes clean.
    let rest = &body[fence_span.end..];
    fence_span.end += if rest.starts_with("\r\n") {
        2
    } else {
        usize::from(rest.starts_with('\n'))
    };
    let raw_span = content_span.unwrap_or_else(|| {
        // An empty block: an empty span just after the opening fence line.
        let after_open = body[fence_span.clone()]
            .find('\n')
            .map_or(fence_span.end, |i| fence_span.start + i + 1);
        after_open..after_open
    });
    let notes = format!("{}{}", &body[..fence_span.start], &body[fence_span.end..]);
    let shift = |s: Span| s.start + span.start..s.end + span.start;
    Body {
        payload: Some(Payload {
            text: unescape(&content),
            raw_span: shift(raw_span),
            fence_span: shift(fence_span),
            info,
        }),
        notes,
    }
}

/// Replaces every `\{{` with `{{`.
pub fn unescape(text: &str) -> String {
    text.replace("\\{{", "{{")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(text: &str) -> Body {
        parse_body(text, 0..text.len(), true)
    }

    #[test]
    fn first_fence_is_payload() {
        let text = "intro\n\n```sql\nselect 1;\n```\n\n```\nsecond\n```\nafter\n";
        let b = body(text);
        let p = b.payload.unwrap();
        assert_eq!(p.text, "select 1;\n");
        assert_eq!(p.info.as_deref(), Some("sql"));
        assert_eq!(&text[p.fence_span.clone()], "```sql\nselect 1;\n```\n");
        assert_eq!(&text[p.raw_span.clone()], "select 1;\n");
        assert_eq!(b.notes, "intro\n\n\n```\nsecond\n```\nafter\n");
    }

    #[test]
    fn long_fence_holds_inner_fences() {
        let text = "````md\nUse:\n```\ncode\n```\n````\n";
        let p = body(text).payload.unwrap();
        assert_eq!(p.text, "Use:\n```\ncode\n```\n");
        assert_eq!(p.info.as_deref(), Some("md"));
    }

    #[test]
    fn tilde_fence() {
        let p = body("~~~\nls -la\n~~~\n").payload.unwrap();
        assert_eq!(p.text, "ls -la\n");
        assert_eq!(p.info, None);
    }

    #[test]
    fn escaped_braces_in_text_but_not_raw() {
        let text = "```\necho \\{{x}} {{y}}\n```\n";
        let p = body(text).payload.unwrap();
        assert_eq!(p.text, "echo {{x}} {{y}}\n");
        assert_eq!(&text[p.raw_span.clone()], "echo \\{{x}} {{y}}\n");
    }

    #[test]
    fn fence_in_block_quote_is_found() {
        let p = body("> ```\n> q\n> ```\n").payload.unwrap();
        assert_eq!(p.text, "q\n");
    }

    #[test]
    fn indented_code_is_not_a_payload() {
        let b = body("text\n\n    indented code\n");
        assert!(b.payload.is_none());
        assert_eq!(b.notes, "text\n\n    indented code\n");
    }

    #[test]
    fn empty_fence() {
        let text = "```\n```\n";
        let p = body(text).payload.unwrap();
        assert_eq!(p.text, "");
        assert_eq!(&text[p.fence_span.clone()], "```\n```\n");
        assert!(p.raw_span.is_empty());
    }

    #[test]
    fn no_payload_wanted() {
        let text = "```\nnot a payload\n```\nnotes\n";
        let b = parse_body(text, 0..text.len(), false);
        assert!(b.payload.is_none());
        assert_eq!(b.notes, text);
    }

    #[test]
    fn spans_are_absolute() {
        let full = "---\na: 1\n---\n\n```\nx\n```\n";
        let start = full.find("\n\n").unwrap() + 1;
        let p = parse_body(full, start..full.len(), true).payload.unwrap();
        assert_eq!(&full[p.raw_span.clone()], "x\n");
        assert_eq!(&full[p.fence_span.clone()], "```\nx\n```\n");
    }

    #[test]
    fn crlf_payload() {
        let text = "```\r\nline\r\n```\r\n";
        let p = body(text).payload.unwrap();
        assert_eq!(&text[p.raw_span.clone()], "line\r\n");
    }
}
