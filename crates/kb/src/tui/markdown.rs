//! Notes as Markdown: parsed into blocks of styled spans, then wrapped to a width.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::highlight::Span;
use super::layout::width;
use super::theme::Class;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Span>),
    List {
        ordered: bool,
        items: Vec<Vec<Span>>,
    },
    Quote(Vec<Span>),
    Code(Vec<String>),
    Heading(Vec<Span>),
}

fn push(spans: &mut Vec<Span>, text: &str, class: Class) {
    if text.is_empty() {
        return;
    }
    match spans.last_mut() {
        Some(last) if last.class == class => last.text.push_str(text),
        _ => spans.push(Span::new(text, class)),
    }
}

/// Parses `text` (the body of an entry minus its payload block).
pub fn render_notes(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current: Vec<Span> = Vec::new();
    let mut items: Vec<Vec<Span>> = Vec::new();
    let mut ordered = false;
    let mut list_depth = 0usize;
    let mut quote = false;
    let mut heading = false;
    let mut code: Option<Vec<String>> = None;
    let (mut bold, mut italic, mut link) = (false, false, false);
    let class = |bold: bool, italic: bool, link: bool, heading: bool| {
        if link {
            Class::Link
        } else if heading || bold {
            if heading { Class::Heading } else { Class::Bold }
        } else if italic {
            Class::Italic
        } else {
            Class::Plain
        }
    };
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,
            Event::Start(Tag::Link { .. }) => link = true,
            Event::End(TagEnd::Link) => link = false,
            Event::Start(Tag::Heading { .. }) => heading = true,
            Event::End(TagEnd::Heading(_)) => {
                heading = false;
                blocks.push(Block::Heading(std::mem::take(&mut current)));
            }
            Event::Start(Tag::BlockQuote(_)) => quote = true,
            Event::End(TagEnd::BlockQuote(_)) => {
                quote = false;
                if !current.is_empty() {
                    blocks.push(Block::Quote(std::mem::take(&mut current)));
                }
            }
            Event::Start(Tag::List(first)) => {
                if list_depth == 0 {
                    ordered = first.is_some();
                    items.clear();
                }
                list_depth += 1;
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                if list_depth == 0 {
                    blocks.push(Block::List {
                        ordered,
                        items: std::mem::take(&mut items),
                    });
                }
            }
            Event::End(TagEnd::Item) => items.push(std::mem::take(&mut current)),
            Event::End(TagEnd::Paragraph) => {
                if list_depth > 0 {
                    // a loose list item: keep the paragraph in the item
                } else if quote {
                    // the quote block closes it
                } else {
                    blocks.push(Block::Paragraph(std::mem::take(&mut current)));
                }
            }
            Event::Start(Tag::CodeBlock(_)) => code = Some(Vec::new()),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(lines) = code.take() {
                    blocks.push(Block::Code(lines));
                }
            }
            Event::Text(t) => match &mut code {
                Some(lines) => {
                    lines.extend(t.trim_end_matches('\n').split('\n').map(str::to_string))
                }
                None => push(&mut current, &t, class(bold, italic, link, heading)),
            },
            Event::Code(t) => push(&mut current, &t, Class::Code),
            Event::Html(t) | Event::InlineHtml(t) => push(&mut current, &t, Class::Plain),
            Event::SoftBreak => push(&mut current, " ", Class::Plain),
            Event::HardBreak => push(&mut current, "\n", Class::Plain),
            _ => {}
        }
    }
    if !current.is_empty() {
        blocks.push(Block::Paragraph(current));
    }
    blocks
}

/// Words with the class they came from; `None` text is a forced line break.
fn words(spans: &[Span]) -> Vec<(String, Class, bool)> {
    let mut out: Vec<(String, Class, bool)> = Vec::new(); // (word, class, space_before)
    let mut space = false;
    for s in spans {
        for (i, part) in s.text.split('\n').enumerate() {
            if i > 0 {
                out.push((String::new(), s.class, false)); // break marker
                space = false;
            }
            let mut first = true;
            for w in part.split(' ') {
                if w.is_empty() {
                    space = !first || space || part.starts_with(' ');
                    first = false;
                    continue;
                }
                let lead = !first || (space && !out.is_empty()) || part.starts_with(' ');
                out.push((
                    w.to_string(),
                    s.class,
                    lead && !out.is_empty() && !out.last().is_some_and(|l| l.0.is_empty()),
                ));
                first = false;
                space = part.ends_with(' ');
            }
            if part.ends_with(' ') {
                space = true;
            }
        }
    }
    out
}

/// Fills `spans` into lines of at most `cols` columns; `first` and `rest` are the prefixes of the
/// first and the following lines (hanging indent).
fn fill(
    spans: &[Span],
    cols: usize,
    first: &str,
    rest: &str,
    prefix_class: Class,
) -> Vec<Vec<Span>> {
    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut cur: Vec<Span> = Vec::new();
    let mut used = 0usize;
    let start = |prefix: &str, cur: &mut Vec<Span>, used: &mut usize| {
        cur.clear();
        if !prefix.is_empty() {
            cur.push(Span::new(prefix, prefix_class));
        }
        *used = width(prefix);
    };
    start(first, &mut cur, &mut used);
    let mut at_start = true;
    for (word, class, space) in words(spans) {
        if word.is_empty() {
            lines.push(std::mem::take(&mut cur));
            start(rest, &mut cur, &mut used);
            at_start = true;
            continue;
        }
        let mut word = word;
        loop {
            let need = width(&word) + usize::from(space && !at_start);
            if used + need <= cols || (at_start && used + width(&word) <= cols.max(1)) {
                if space && !at_start {
                    push(&mut cur, " ", class);
                    used += 1;
                }
                used += width(&word);
                push(&mut cur, &word, class);
                at_start = false;
                break;
            }
            if !at_start {
                lines.push(std::mem::take(&mut cur));
                start(rest, &mut cur, &mut used);
                at_start = true;
                continue;
            }
            // a single word longer than the line: split it by characters
            let room = cols.saturating_sub(used).max(1);
            let mut head = String::new();
            let mut w = 0;
            let mut tail = word.chars().peekable();
            while let Some(&c) = tail.peek() {
                let cw = width(&c.to_string());
                if w + cw > room && !head.is_empty() {
                    break;
                }
                head.push(c);
                w += cw;
                tail.next();
            }
            push(&mut cur, &head, class);
            lines.push(std::mem::take(&mut cur));
            start(rest, &mut cur, &mut used);
            word = tail.collect();
            if word.is_empty() {
                break;
            }
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// The blocks as lines no wider than `cols`, with a blank line between blocks.
pub fn wrap(blocks: &[Block], cols: usize) -> Vec<Vec<Span>> {
    let cols = cols.max(4);
    let mut out: Vec<Vec<Span>> = Vec::new();
    for (n, block) in blocks.iter().enumerate() {
        if n > 0 {
            out.push(Vec::new());
        }
        match block {
            Block::Paragraph(spans) => out.extend(fill(spans, cols, "", "", Class::Plain)),
            Block::Heading(spans) => out.extend(fill(spans, cols, "", "", Class::Heading)),
            Block::Quote(spans) => out.extend(fill(spans, cols, "▏ ", "▏ ", Class::Quote)),
            Block::List { ordered, items } => {
                for (i, item) in items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}. ", i + 1)
                    } else {
                        "• ".to_string()
                    };
                    let hang = " ".repeat(width(&marker));
                    out.extend(fill(item, cols, &marker, &hang, Class::Muted));
                }
            }
            Block::Code(lines) => {
                for l in lines {
                    out.push(vec![Span::new(
                        super::layout::truncate(&format!("  {l}"), cols),
                        Class::Code,
                    )]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(lines: &[Vec<Span>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn inline_styles_lists_quotes_and_code() {
        let src =
            "**b** *i* `c` [t](http://u)\n\n- one\n- two\n\n1. a\n2. b\n\n> q\n\n```\ncode\n```\n";
        let blocks = render_notes(src);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        let classes: Vec<Class> = p
            .iter()
            .filter(|s| s.class != Class::Plain)
            .map(|s| s.class)
            .collect();
        assert_eq!(
            classes,
            vec![Class::Bold, Class::Italic, Class::Code, Class::Link]
        );
        let text: String = p.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "b i c t", "the URL is not in the text");
        assert!(matches!(&blocks[1], Block::List { ordered: false, items } if items.len() == 2));
        assert!(matches!(&blocks[2], Block::List { ordered: true, items } if items.len() == 2));
        assert!(matches!(&blocks[3], Block::Quote(q) if q[0].text == "q"));
        assert!(matches!(&blocks[4], Block::Code(c) if c == &vec!["code".to_string()]));
    }

    #[test]
    fn headings_and_unsupported_markup_become_text() {
        let blocks =
            render_notes("# Title\n\ntext with <b>html</b>\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(
            matches!(&blocks[0], Block::Heading(h) if h[0].text == "Title" && h[0].class == Class::Heading)
        );
        let all = text_of(&wrap(&blocks, 80)).join("\n");
        assert!(
            all.contains("html") && all.contains('1') && all.contains('2'),
            "{all}"
        );
        assert!(render_notes("").is_empty());
    }

    #[test]
    fn wrapping_respects_the_width_with_wide_characters_and_hanging_indents() {
        let blocks = render_notes(
            "The quick brown fox jumps over the lazy dog and keeps running far away\n\n- a bullet item that is long enough to wrap onto a second line\n\n日本語のメモ日本語のメモ日本語のメモ日本語のメモ",
        );
        for cols in [20usize, 33, 80] {
            for line in text_of(&wrap(&blocks, cols)) {
                assert!(width(&line) <= cols, "{cols}: {line:?} is {}", width(&line));
            }
        }
        let lines = text_of(&wrap(&blocks, 20));
        let bullet = lines.iter().position(|l| l.starts_with("• ")).unwrap();
        assert!(lines[bullet + 1].starts_with("  "), "{lines:?}");
        let joined = lines.join(" ");
        assert!(joined.contains("quick brown fox") && joined.contains("lazy dog"));
    }

    #[test]
    fn long_words_are_split_rather_than_overflowing() {
        let blocks =
            render_notes("x https://example.com/a/very/long/url/that/never/ends/at/all/ok");
        for l in text_of(&wrap(&blocks, 16)) {
            assert!(width(&l) <= 16, "{l:?}");
        }
        let joined: String = text_of(&wrap(&blocks, 16)).concat();
        assert!(
            joined
                .replace(' ', "")
                .contains("example.com/a/very/long/url")
        );
    }

    #[test]
    fn blank_line_between_blocks_and_hard_breaks() {
        let lines = text_of(&wrap(&render_notes("one\n\ntwo"), 40));
        assert_eq!(lines, vec!["one", "", "two"]);
        let lines = text_of(&wrap(&render_notes("a  \nb"), 40));
        assert_eq!(lines, vec!["a", "b"]);
    }
}
