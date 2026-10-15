//! Markdown notes for the terminal: parsed into width-independent blocks, then laid out into
//! lines of styled spans for a given width.
//!
//! Everything that reaches a span has been through [`kb_core::terminal::display`], so a note can
//! never put a control character or an escape sequence on the screen. Raw HTML is plain text.
//! Code is highlighted lazily: a block's lines are styled the first time one of them is drawn.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use kb_core::entry::EntryType;
use kb_core::entry::link::extract_links;
use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use super::highlight::{Span, highlight_fenced};
use super::layout::width;
use super::theme::Class;

/// The five GitHub callouts (`> [!NOTE]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalloutKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl CalloutKind {
    pub fn label(self) -> &'static str {
        match self {
            CalloutKind::Note => "Note",
            CalloutKind::Tip => "Tip",
            CalloutKind::Important => "Important",
            CalloutKind::Warning => "Warning",
            CalloutKind::Caution => "Caution",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            CalloutKind::Note => "ℹ",
            CalloutKind::Tip => "💡",
            CalloutKind::Important => "❗",
            CalloutKind::Warning => "⚠",
            CalloutKind::Caution => "🛑",
        }
    }

    /// 256-color indexes for the dark and the light theme.
    pub fn colors(self) -> (u8, u8) {
        match self {
            CalloutKind::Note => (75, 25),
            CalloutKind::Tip => (114, 28),
            CalloutKind::Important => (141, 54),
            CalloutKind::Warning => (214, 166),
            CalloutKind::Caution => (203, 160),
        }
    }

    fn of(kind: BlockQuoteKind) -> CalloutKind {
        match kind {
            BlockQuoteKind::Note => CalloutKind::Note,
            BlockQuoteKind::Tip => CalloutKind::Tip,
            BlockQuoteKind::Important => CalloutKind::Important,
            BlockQuoteKind::Warning => CalloutKind::Warning,
            BlockQuoteKind::Caution => CalloutKind::Caution,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub aligns: Vec<Align>,
    pub head: Vec<Vec<Span>>,
    pub rows: Vec<Vec<Vec<Span>>>,
}

/// A fenced code block. Its styled lines are made the first time they are needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock {
    pub lang: Option<String>,
    pub lines: Vec<String>,
    /// False for `NO_COLOR`: the grammar is skipped.
    pub color: bool,
    styled: OnceLock<Vec<Vec<Span>>>,
}

impl CodeBlock {
    pub fn new(lang: Option<String>, lines: Vec<String>, color: bool) -> CodeBlock {
        CodeBlock {
            lang,
            lines,
            color,
            styled: OnceLock::new(),
        }
    }

    /// The lines with their token classes. Computed once, on first use.
    pub fn styled(&self) -> &[Vec<Span>] {
        self.styled
            .get_or_init(|| highlight_fenced(self.lang.as_deref(), &self.lines, self.color))
    }

    pub fn is_highlighted(&self) -> bool {
        self.styled.get().is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// `Some` for a task (`- [ ]`, `- [x]`).
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Span>),
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    List {
        ordered: bool,
        start: u64,
        items: Vec<Item>,
    },
    Quote(Vec<Block>),
    Callout {
        kind: CalloutKind,
        blocks: Vec<Block>,
    },
    Code(Arc<CodeBlock>),
    Table(Table),
    /// `(number, content)` of each footnote, at the end of the document.
    Footnotes(Vec<(usize, Vec<Block>)>),
    Rule,
}

impl Block {
    /// A rough size in bytes, for bounding caches.
    pub fn approx_bytes(&self) -> usize {
        fn spans(s: &[Span]) -> usize {
            s.iter().map(|s| s.text.len() + 48).sum()
        }
        fn blocks(b: &[Block]) -> usize {
            b.iter().map(Block::approx_bytes).sum()
        }
        64 + match self {
            Block::Paragraph(s) | Block::Heading { spans: s, .. } => spans(s),
            Block::List { items, .. } => items.iter().map(|i| blocks(&i.blocks)).sum(),
            Block::Quote(b) | Block::Callout { blocks: b, .. } => blocks(b),
            Block::Code(c) => c.lines.iter().map(|l| l.len() + 24).sum::<usize>(),
            Block::Table(t) => {
                spans(&t.head.concat()) + t.rows.iter().map(|r| spans(&r.concat())).sum::<usize>()
            }
            Block::Footnotes(f) => f.iter().map(|(_, b)| blocks(b)).sum(),
            Block::Rule => 0,
        }
    }
}

/// What `[[entry-id]]` links in a document point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRef {
    pub id: String,
    /// The target's type and title; `None` when it does not exist.
    pub target: Option<(EntryType, String)>,
}

/// A parsed document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rendered {
    pub blocks: Vec<Block>,
    /// Each entry linked, once, in order of first appearance.
    pub links: Vec<LinkRef>,
}

/// Looks an entry up by ID: its type and title.
pub type Lookup<'a> = &'a dyn Fn(&str) -> Option<(EntryType, String)>;

/// What parsing needs to know about the world.
#[derive(Clone, Copy, Default)]
pub struct Ctx<'a> {
    /// Highlight code (false for `NO_COLOR`).
    pub color: bool,
    /// Treat `[[entry-id]]` as a link (notes do; other entries show it as text).
    pub entry_links: bool,
    /// Resolves `[[entry-id]]`; without it every link counts as broken.
    pub lookup: Option<Lookup<'a>>,
}

// Stand-ins for `[[entry-id]]` while the Markdown parser runs: private-use characters no
// Markdown syntax uses, so the parser leaves them alone and they come back out in a Text event.
const OPEN: char = '\u{E001}';
const CLOSE: char = '\u{E002}';

fn push(spans: &mut Vec<Span>, text: &str, class: Class) {
    push_linked(spans, text, class, None);
}

fn push_linked(spans: &mut Vec<Span>, text: &str, class: Class, url: Option<&Arc<str>>) {
    if text.is_empty() {
        return;
    }
    match spans.last_mut() {
        Some(last) if last.class == class && last.url.as_ref() == url => last.text.push_str(text),
        _ => spans.push(Span {
            text: text.to_string(),
            class,
            url: url.cloned(),
        }),
    }
}

enum Kind {
    Root,
    Quote(Option<CalloutKind>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<Item>,
    },
    Item {
        checked: Option<bool>,
    },
    Footnote(String),
}

struct Frame {
    kind: Kind,
    blocks: Vec<Block>,
}

struct Builder<'a> {
    ctx: Ctx<'a>,
    links: Vec<LinkRef>,
    /// The `[[entry-id]]`s taken out of the source, by placeholder number.
    placeholders: Vec<String>,
    frames: Vec<Frame>,
    spans: Vec<Span>,
    bold: u32,
    italic: u32,
    strike: u32,
    link: Option<Arc<str>>,
    heading: Option<u8>,
    code: Option<(Option<String>, String)>,
    image: Option<(String, String)>,
    table: Option<TableState>,
    footnote_refs: Vec<String>,
    footnotes: Vec<(String, Vec<Block>)>,
}

struct TableState {
    aligns: Vec<Align>,
    head: Vec<Vec<Span>>,
    rows: Vec<Vec<Vec<Span>>>,
    row: Vec<Vec<Span>>,
    in_head: bool,
}

impl Builder<'_> {
    fn top(&mut self) -> &mut Vec<Block> {
        &mut self.frames.last_mut().expect("the root frame").blocks
    }

    fn class(&self) -> Class {
        if self.link.is_some() {
            Class::Link
        } else if let Some(level) = self.heading {
            if level <= 3 {
                Class::Heading
            } else {
                Class::Muted
            }
        } else if self.strike > 0 {
            Class::Strike
        } else if self.bold > 0 {
            Class::Bold
        } else if self.italic > 0 {
            Class::Italic
        } else {
            Class::Plain
        }
    }

    fn flush(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        self.top().push(Block::Paragraph(spans));
    }

    fn text(&mut self, t: &str) {
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(t);
            return;
        }
        let class = self.class();
        let url = self.link.clone();
        let mut rest = t;
        while let Some(i) = rest.find(OPEN) {
            push_linked(&mut self.spans, &rest[..i], class, url.as_ref());
            let after = &rest[i + OPEN.len_utf8()..];
            let Some(j) = after.find(CLOSE) else {
                rest = after;
                break;
            };
            if let Some(id) = after[..j]
                .parse::<usize>()
                .ok()
                .and_then(|n| self.placeholders.get(n).cloned())
            {
                self.entry_link(&id);
            }
            rest = &after[j + CLOSE.len_utf8()..];
        }
        push_linked(&mut self.spans, rest, class, url.as_ref());
    }

    fn entry_link(&mut self, id: &str) {
        let target = self.ctx.lookup.and_then(|f| f(id));
        match &target {
            Some((ty, title)) => {
                push(
                    &mut self.spans,
                    &format!("[{}]", ty.name()),
                    Class::EntryBadge(*ty),
                );
                push(&mut self.spans, " ", Class::Plain);
                push(&mut self.spans, title, Class::Link);
            }
            None => push(&mut self.spans, &format!("[?] {id}"), Class::Broken),
        }
        if !self.links.iter().any(|l| l.id == id) {
            self.links.push(LinkRef {
                id: id.to_string(),
                target,
            });
        }
    }

    fn footnote_number(&mut self, label: &str) -> usize {
        match self.footnote_refs.iter().position(|l| l == label) {
            Some(i) => i + 1,
            None => {
                self.footnote_refs.push(label.to_string());
                self.footnote_refs.len()
            }
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::Text(t) => match &mut self.code {
                Some((_, buf)) => buf.push_str(&t),
                None => self.text(&t),
            },
            Event::Code(t) => {
                if let Some((_, alt)) = &mut self.image {
                    alt.push_str(&t);
                } else {
                    let url = self.link.clone();
                    push_linked(&mut self.spans, &t, Class::Code, url.as_ref());
                }
            }
            // raw HTML is never interpreted: it is shown as the text it is
            Event::Html(t) => match &mut self.code {
                Some((_, buf)) => buf.push_str(&t),
                None => {
                    // an HTML block keeps its lines, as a paragraph of its own
                    self.flush();
                    for (i, line) in t.trim_end_matches('\n').split('\n').enumerate() {
                        if i > 0 {
                            push(&mut self.spans, "\n", Class::Plain);
                        }
                        self.text(line);
                    }
                    self.flush();
                }
            },
            Event::InlineHtml(t) => match &mut self.code {
                Some((_, buf)) => buf.push_str(&t),
                None => self.text(&t),
            },
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => push(&mut self.spans, "\n", Class::Plain),
            Event::Rule => {
                self.flush();
                self.top().push(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                if let Some(Frame {
                    kind: Kind::Item { checked },
                    ..
                }) = self.frames.last_mut()
                {
                    *checked = Some(done);
                }
            }
            Event::FootnoteReference(label) => {
                let n = self.footnote_number(&label);
                push(&mut self.spans, &format!("[{n}]"), Class::Muted);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Strong => self.bold += 1,
            Tag::Emphasis => self.italic += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.link = Some(Arc::from(&*dest_url)),
            Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                });
            }
            Tag::BlockQuote(kind) => {
                self.flush();
                self.frames.push(Frame {
                    kind: Kind::Quote(kind.map(CalloutKind::of)),
                    blocks: Vec::new(),
                });
            }
            Tag::List(first) => {
                self.flush();
                self.frames.push(Frame {
                    kind: Kind::List {
                        ordered: first.is_some(),
                        start: first.unwrap_or(1),
                        items: Vec::new(),
                    },
                    blocks: Vec::new(),
                });
            }
            Tag::Item => self.frames.push(Frame {
                kind: Kind::Item { checked: None },
                blocks: Vec::new(),
            }),
            Tag::FootnoteDefinition(label) => {
                self.flush();
                self.frames.push(Frame {
                    kind: Kind::Footnote(label.to_string()),
                    blocks: Vec::new(),
                });
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .map(|w| w.trim_matches(['{', '}', '.']).to_string())
                        .filter(|w| !w.is_empty()),
                    CodeBlockKind::Indented => None,
                };
                self.code = Some((lang, String::new()));
            }
            Tag::Table(aligns) => {
                self.flush();
                self.table = Some(TableState {
                    aligns: aligns
                        .into_iter()
                        .map(|a| match a {
                            Alignment::Center => Align::Center,
                            Alignment::Right => Align::Right,
                            Alignment::None | Alignment::Left => Align::Left,
                        })
                        .collect(),
                    head: Vec::new(),
                    rows: Vec::new(),
                    row: Vec::new(),
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(t) = &mut self.table {
                    t.in_head = true;
                }
            }
            Tag::TableRow | Tag::TableCell => self.spans.clear(),
            _ => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => self.link = None,
            TagEnd::Image => {
                if let Some((url, alt)) = self.image.take() {
                    let url = url.trim();
                    let text = if url.is_empty() {
                        format!("[image: {alt}]")
                    } else {
                        format!("[image: {alt}] ({url})")
                    };
                    push(&mut self.spans, &text, Class::Muted);
                }
            }
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(_) => {
                let level = self.heading.take().unwrap_or(1);
                let spans = std::mem::take(&mut self.spans);
                self.top().push(Block::Heading { level, spans });
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                if let Some(Frame {
                    kind: Kind::Quote(kind),
                    blocks,
                }) = self.frames.pop()
                {
                    self.top().push(match kind {
                        Some(kind) => Block::Callout { kind, blocks },
                        None => Block::Quote(blocks),
                    });
                }
            }
            TagEnd::Item => {
                self.flush();
                if let Some(Frame {
                    kind: Kind::Item { checked },
                    blocks,
                }) = self.frames.pop()
                    && let Some(Frame {
                        kind: Kind::List { items, .. },
                        ..
                    }) = self.frames.last_mut()
                {
                    items.push(Item { checked, blocks });
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame {
                    kind:
                        Kind::List {
                            ordered,
                            start,
                            items,
                        },
                    ..
                }) = self.frames.pop()
                {
                    self.top().push(Block::List {
                        ordered,
                        start,
                        items,
                    });
                }
            }
            TagEnd::FootnoteDefinition => {
                self.flush();
                if let Some(Frame {
                    kind: Kind::Footnote(label),
                    blocks,
                }) = self.frames.pop()
                {
                    self.footnotes.push((label, blocks));
                }
            }
            TagEnd::CodeBlock => {
                if let Some((lang, text)) = self.code.take() {
                    let text = text.strip_suffix('\n').unwrap_or(&text);
                    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
                    let color = self.ctx.color;
                    self.top()
                        .push(Block::Code(Arc::new(CodeBlock::new(lang, lines, color))));
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.spans);
                if let Some(t) = &mut self.table {
                    t.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    t.head = std::mem::take(&mut t.row);
                    t.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.top().push(Block::Table(Table {
                        aligns: t.aligns,
                        head: t.head,
                        rows: t.rows,
                    }));
                }
            }
            _ => {}
        }
    }
}

/// Parses `text`: the body of a note, or the part of another entry outside its payload.
pub fn render_markdown(text: &str, ctx: &Ctx<'_>) -> Rendered {
    // 1. nothing a terminal could act on survives, and tabs become spaces
    let clean = kb_core::terminal::display(text);
    let mut src: String = clean
        .chars()
        .filter(|c| *c != OPEN && *c != CLOSE)
        .collect::<String>()
        .replace('\t', "    ");
    // 2. `[[entry-id]]` becomes a placeholder, resolved when the text comes out of the parser
    let found = if ctx.entry_links {
        extract_links(&src)
    } else {
        Vec::new()
    };
    let mut placeholders = vec![String::new(); found.len()];
    for (n, link) in found.iter().enumerate().rev() {
        placeholders[n].clone_from(&link.target_id);
        src.replace_range(link.span.clone(), &format!("{OPEN}{n}{CLOSE}"));
    }
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM;
    let mut b = Builder {
        ctx: *ctx,
        links: Vec::new(),
        placeholders,
        frames: vec![Frame {
            kind: Kind::Root,
            blocks: Vec::new(),
        }],
        spans: Vec::new(),
        bold: 0,
        italic: 0,
        strike: 0,
        link: None,
        heading: None,
        code: None,
        image: None,
        table: None,
        footnote_refs: Vec::new(),
        footnotes: Vec::new(),
    };
    for event in Parser::new_ext(&src, options) {
        b.event(event);
    }
    b.flush();
    // a frame left open by a parser quirk is closed rather than lost
    while b.frames.len() > 1 {
        let frame = b.frames.pop().expect("a frame");
        b.top().extend(frame.blocks);
    }
    let mut blocks = b.frames.pop().expect("the root").blocks;
    if !b.footnotes.is_empty() {
        let mut defs = std::mem::take(&mut b.footnotes);
        let order = &b.footnote_refs;
        defs.sort_by_key(|(label, _)| order.iter().position(|l| l == label).unwrap_or(usize::MAX));
        let numbered = defs
            .into_iter()
            .enumerate()
            .map(|(i, (_, blocks))| (i + 1, blocks))
            .collect();
        blocks.push(Block::Footnotes(numbered));
    }
    Rendered {
        blocks,
        links: b.links,
    }
}

/// Parses `text` with no entry lookup and no code highlighting.
pub fn render_notes(text: &str) -> Vec<Block> {
    render_markdown(text, &Ctx::default()).blocks
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
    let mut prev: Option<Class> = None;
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
                    // a space inside one style carries it (an unbroken underline); between two
                    // styles it is plain, so a badge's color stops at the badge
                    let between = if prev == Some(class) {
                        class
                    } else {
                        Class::Plain
                    };
                    push(&mut cur, " ", between);
                    used += 1;
                }
                used += width(&word);
                push(&mut cur, &word, class);
                prev = Some(class);
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

/// One line of laid-out Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteLine {
    Text(Vec<Span>),
    /// A line of a framed code block; its styled spans are made when the line is drawn.
    Code {
        block: Arc<CodeBlock>,
        line: usize,
        /// Columns between the frame's borders.
        inner: usize,
        /// Quote bars and list indentation in front of the frame.
        prefix: Vec<Span>,
    },
}

impl NoteLine {
    /// The spans to draw. For a code line this is where the block gets highlighted.
    pub fn spans(&self) -> Cow<'_, [Span]> {
        match self {
            NoteLine::Text(spans) => Cow::Borrowed(spans),
            NoteLine::Code {
                block,
                line,
                inner,
                prefix,
            } => {
                let mut out = prefix.clone();
                out.push(Span::new("│ ", Class::Muted));
                let styled = block.styled().get(*line).cloned().unwrap_or_default();
                let (mut body, used) = cut_spans(styled, *inner);
                out.append(&mut body);
                if used < *inner {
                    out.push(Span::new(" ".repeat(inner - used), Class::Plain));
                }
                out.push(Span::new(" │", Class::Muted));
                Cow::Owned(out)
            }
        }
    }

    /// The text of the line, without styling.
    pub fn text(&self) -> String {
        self.spans().iter().map(|s| s.text.as_str()).collect()
    }

    fn with_prefix(self, prefix: &[Span]) -> NoteLine {
        if prefix.is_empty() {
            return self;
        }
        match self {
            NoteLine::Text(mut spans) => {
                let mut out = prefix.to_vec();
                out.append(&mut spans);
                NoteLine::Text(out)
            }
            NoteLine::Code {
                block,
                line,
                inner,
                prefix: mut own,
            } => {
                let mut out = prefix.to_vec();
                out.append(&mut own);
                NoteLine::Code {
                    block,
                    line,
                    inner,
                    prefix: out,
                }
            }
        }
    }

    /// A rough size in bytes, for bounding caches.
    pub fn approx_bytes(&self) -> usize {
        match self {
            NoteLine::Text(s) => 32 + s.iter().map(|s| s.text.len() + 48).sum::<usize>(),
            NoteLine::Code { prefix, .. } => 64 + prefix.len() * 48,
        }
    }
}

/// `spans` cut to `max` columns, ending in `›` when something was cut; with the width used.
fn cut_spans(spans: Vec<Span>, max: usize) -> (Vec<Span>, usize) {
    let total: usize = spans.iter().map(|s| width(&s.text)).sum();
    if total <= max {
        return (spans, total);
    }
    let budget = max.saturating_sub(1);
    let mut out: Vec<Span> = Vec::new();
    let mut used = 0;
    'outer: for s in spans {
        let mut text = String::new();
        for c in s.text.chars() {
            let w = width(c.encode_utf8(&mut [0; 4]));
            if used + w > budget {
                if !text.is_empty() {
                    out.push(Span::new(text, s.class));
                }
                break 'outer;
            }
            text.push(c);
            used += w;
        }
        out.push(Span::new(text, s.class));
    }
    out.push(Span::new("›", Class::Muted));
    (out, used + 1)
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| width(&s.text)).sum()
}

fn text_line(spans: Vec<Span>) -> NoteLine {
    NoteLine::Text(spans)
}

fn rule(cols: usize, ch: &str, class: Class) -> NoteLine {
    text_line(vec![Span::new(ch.repeat(cols), class)])
}

/// Attaches `first` to the first of `child` and `rest` to the others, then adds them to `out`.
fn attach(out: &mut Vec<NoteLine>, child: Vec<NoteLine>, first: &[Span], rest: &[Span]) {
    for (i, line) in child.into_iter().enumerate() {
        out.push(line.with_prefix(if i == 0 { first } else { rest }));
    }
}

fn layout_blocks(out: &mut Vec<NoteLine>, blocks: &[Block], cols: usize) {
    layout_blocks_in(out, blocks, cols, false);
}

/// `in_item`: a list directly under the text of a list item sits right below it, as in the source.
fn layout_blocks_in(out: &mut Vec<NoteLine>, blocks: &[Block], cols: usize, in_item: bool) {
    for (n, block) in blocks.iter().enumerate() {
        let tight = in_item
            && matches!(block, Block::List { .. })
            && matches!(blocks.get(n.wrapping_sub(1)), Some(Block::Paragraph(_)));
        if n > 0 && !tight {
            out.push(text_line(Vec::new()));
        }
        layout_block(out, block, cols);
    }
}

fn layout_block(out: &mut Vec<NoteLine>, block: &Block, cols: usize) {
    match block {
        Block::Paragraph(spans) => out.extend(
            fill(spans, cols, "", "", Class::Plain)
                .into_iter()
                .map(text_line),
        ),
        Block::Heading { level, spans } => {
            let class = if *level <= 3 {
                Class::Heading
            } else {
                Class::Muted
            };
            let lines = fill(spans, cols, "", "", class);
            let longest = lines.iter().map(|l| spans_width(l)).max().unwrap_or(0);
            out.extend(lines.into_iter().map(text_line));
            match level {
                1 => out.push(rule(cols.min(longest.max(8)), "━", Class::Heading)),
                2 => out.push(rule(cols.min(longest.max(8)), "─", Class::Muted)),
                _ => {}
            }
        }
        Block::Rule => out.push(rule(cols.min(40), "─", Class::Muted)),
        Block::Quote(blocks) => {
            let bar = [Span::new("▏ ", Class::Quote)];
            let mut child = Vec::new();
            layout_blocks(&mut child, blocks, cols.saturating_sub(2).max(1));
            attach(out, child, &bar, &bar);
        }
        Block::Callout { kind, blocks } => {
            let class = Class::Callout(*kind);
            let bar = [Span::new("▌ ", class)];
            out.push(text_line(vec![
                Span::new("▌ ", class),
                Span::new(format!("{} {}", kind.icon(), kind.label()), class),
            ]));
            let mut child = Vec::new();
            layout_blocks(&mut child, blocks, cols.saturating_sub(2).max(1));
            attach(out, child, &bar, &bar);
        }
        Block::List {
            ordered,
            start,
            items,
        } => {
            for (i, item) in items.iter().enumerate() {
                let marker = match (item.checked, ordered) {
                    (Some(true), _) => "☑ ".to_string(),
                    (Some(false), _) => "☐ ".to_string(),
                    (None, true) => format!("{}. ", start + i as u64),
                    (None, false) => "• ".to_string(),
                };
                let hang = " ".repeat(width(&marker));
                let first = [Span::new(marker, Class::Muted)];
                let rest = [Span::new(hang.clone(), Class::Plain)];
                let mut child = Vec::new();
                let item_blocks;
                let blocks = if item.checked == Some(true) {
                    item_blocks = dim(&item.blocks);
                    &item_blocks
                } else {
                    &item.blocks
                };
                layout_blocks_in(
                    &mut child,
                    blocks,
                    cols.saturating_sub(width(&hang)).max(1),
                    true,
                );
                if child.is_empty() {
                    child.push(text_line(Vec::new()));
                }
                attach(out, child, &first, &rest);
            }
        }
        Block::Code(code) => layout_code(out, code, cols),
        Block::Table(table) => layout_table(out, table, cols),
        Block::Footnotes(notes) => {
            out.push(rule(cols.min(20), "─", Class::Muted));
            for (n, blocks) in notes {
                let marker = format!("[{n}] ");
                let hang = " ".repeat(width(&marker));
                let mut child = Vec::new();
                layout_blocks(&mut child, blocks, cols.saturating_sub(width(&hang)).max(1));
                attach(
                    out,
                    child,
                    &[Span::new(marker, Class::Muted)],
                    &[Span::new(hang, Class::Plain)],
                );
            }
        }
    }
}

/// A finished task's text, dimmed.
fn dim(blocks: &[Block]) -> Vec<Block> {
    fn dim_spans(spans: &[Span]) -> Vec<Span> {
        spans
            .iter()
            .map(|s| Span {
                text: s.text.clone(),
                class: if s.class == Class::Plain {
                    Class::Muted
                } else {
                    s.class
                },
                url: s.url.clone(),
            })
            .collect()
    }
    blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(s) => Block::Paragraph(dim_spans(s)),
            other => other.clone(),
        })
        .collect()
}

fn layout_code(out: &mut Vec<NoteLine>, code: &Arc<CodeBlock>, cols: usize) {
    if cols < 12 {
        // too narrow for a frame: the lines, cut
        for l in &code.lines {
            let (spans, _) = cut_spans(vec![Span::new(l.as_str(), Class::Code)], cols);
            out.push(text_line(spans));
        }
        return;
    }
    let head = match &code.lang {
        Some(lang) => format!("┌─ {lang} "),
        None => "┌─".to_string(),
    };
    let longest = code.lines.iter().map(|l| width(l)).max().unwrap_or(0);
    // `│ ` + content + ` │`
    let inner = longest
        .max(width(&head).saturating_sub(1))
        .clamp(8, cols - 4);
    let total = inner + 4;
    let top = format!(
        "{head}{}┐",
        "─".repeat(total.saturating_sub(width(&head) + 1))
    );
    out.push(text_line(vec![Span::new(top, Class::Muted)]));
    for line in 0..code.lines.len() {
        out.push(NoteLine::Code {
            block: Arc::clone(code),
            line,
            inner,
            prefix: Vec::new(),
        });
    }
    out.push(text_line(vec![Span::new(
        format!("└{}┘", "─".repeat(total - 2)),
        Class::Muted,
    )]));
}

/// The widest line of a cell, in columns.
fn natural_width(cell: &[Span]) -> usize {
    cell.iter()
        .flat_map(|s| s.text.split('\n').map(width))
        .max()
        .unwrap_or(0)
}

fn layout_table(out: &mut Vec<NoteLine>, table: &Table, cols: usize) {
    let n = table
        .head
        .len()
        .max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
    if n == 0 {
        return;
    }
    let cell =
        |row: &[Vec<Span>], c: usize| -> Vec<Span> { row.get(c).cloned().unwrap_or_default() };
    let all_rows: Vec<&[Vec<Span>]> = std::iter::once(table.head.as_slice())
        .chain(table.rows.iter().map(Vec::as_slice))
        .collect();
    // `│ a │ b │`: a border, then a space, the cell and a space for each column
    let overhead = 3 * n + 1;
    let mut widths: Vec<usize> = (0..n)
        .map(|c| {
            all_rows
                .iter()
                .map(|r| natural_width(&cell(r, c)))
                .max()
                .unwrap_or(0)
                .max(1)
        })
        .collect();
    let available = cols.saturating_sub(overhead).max(n);
    while widths.iter().sum::<usize>() > available {
        // take a column from the widest one; stop at one column each
        let (i, w) = widths
            .iter()
            .enumerate()
            .max_by_key(|(_, w)| **w)
            .map(|(i, w)| (i, *w))
            .expect("a column");
        if w <= 1 {
            break;
        }
        widths[i] -= 1;
    }
    let border = |left: &str, mid: &str, right: &str| {
        let mut s = String::from(left);
        for (i, w) in widths.iter().enumerate() {
            s.push_str(&"─".repeat(w + 2));
            s.push_str(if i + 1 == n { right } else { mid });
        }
        text_line(vec![Span::new(s, Class::Muted)])
    };
    let aligned = |lines: Vec<Vec<Span>>, c: usize, w: usize, bold: bool| -> Vec<Vec<Span>> {
        let align = table.aligns.get(c).copied().unwrap_or(Align::Left);
        lines
            .into_iter()
            .map(|mut line| {
                if bold {
                    for s in &mut line {
                        if s.class == Class::Plain {
                            s.class = Class::Bold;
                        }
                    }
                }
                let used = spans_width(&line);
                let pad = w.saturating_sub(used);
                let (left, right) = match align {
                    Align::Left => (0, pad),
                    Align::Right => (pad, 0),
                    Align::Center => (pad / 2, pad - pad / 2),
                };
                let mut out = vec![Span::new(" ".repeat(left + 1), Class::Plain)];
                out.extend(line);
                out.push(Span::new(" ".repeat(right + 1), Class::Plain));
                out
            })
            .collect()
    };
    out.push(border("┌", "┬", "┐"));
    for (r, row) in all_rows.iter().enumerate() {
        if r == 0 && table.head.is_empty() {
            continue;
        }
        let cells: Vec<Vec<Vec<Span>>> = (0..n)
            .map(|c| {
                let spans = cell(row, c);
                let lines = fill(&spans, widths[c], "", "", Class::Plain);
                // a cell that is still too wide (one long word at width 1) is cut
                let lines = lines
                    .into_iter()
                    .map(|l| cut_spans(l, widths[c]).0)
                    .collect();
                aligned(lines, c, widths[c], r == 0)
            })
            .collect();
        let height = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
        for y in 0..height {
            let mut spans = vec![Span::new("│", Class::Muted)];
            for (c, lines) in cells.iter().enumerate() {
                match lines.get(y) {
                    Some(line) => spans.extend(line.iter().cloned()),
                    None => spans.push(Span::new(" ".repeat(widths[c] + 2), Class::Plain)),
                }
                spans.push(Span::new("│", Class::Muted));
            }
            out.push(text_line(spans));
        }
        if r == 0 && !table.head.is_empty() {
            out.push(border("├", "┼", "┤"));
        }
    }
    out.push(border("└", "┴", "┘"));
}

/// The blocks as lines no wider than `cols`, with a blank line between blocks.
pub fn layout(blocks: &[Block], cols: usize) -> Vec<NoteLine> {
    let mut out = Vec::new();
    layout_blocks(&mut out, blocks, cols.max(4));
    out
}

/// The blocks as plain-styled lines of spans (for tests and callers that only need text).
pub fn wrap(blocks: &[Block], cols: usize) -> Vec<Vec<Span>> {
    layout(blocks, cols)
        .iter()
        .map(|l| l.spans().into_owned())
        .collect()
}

/// A rendered document with its layout kept for the last width asked for.
#[derive(Debug, Default)]
pub struct Notes {
    pub blocks: Vec<Block>,
    pub links: Vec<LinkRef>,
    cache: std::sync::Mutex<Option<(usize, Arc<Vec<NoteLine>>)>>,
}

impl Clone for Notes {
    fn clone(&self) -> Notes {
        Notes {
            blocks: self.blocks.clone(),
            links: self.links.clone(),
            cache: std::sync::Mutex::new(self.cache.lock().ok().and_then(|c| c.clone())),
        }
    }
}

impl PartialEq for Notes {
    fn eq(&self, other: &Notes) -> bool {
        self.blocks == other.blocks && self.links == other.links
    }
}

impl Notes {
    pub fn new(rendered: Rendered) -> Notes {
        Notes {
            blocks: rendered.blocks,
            links: rendered.links,
            cache: std::sync::Mutex::new(None),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Block> {
        self.blocks.iter()
    }

    /// The lines for `cols` columns. Laying a document out is cheap next to drawing it only when
    /// it is not done on every frame, so the last layout is kept.
    pub fn lines(&self, cols: usize) -> Arc<Vec<NoteLine>> {
        let Ok(mut cache) = self.cache.lock() else {
            return Arc::new(layout(&self.blocks, cols));
        };
        match &*cache {
            Some((c, lines)) if *c == cols => Arc::clone(lines),
            _ => {
                let lines = Arc::new(layout(&self.blocks, cols));
                *cache = Some((cols, Arc::clone(&lines)));
                lines
            }
        }
    }

    /// A rough size in bytes, for bounding caches.
    pub fn approx_bytes(&self) -> usize {
        let blocks: usize = self.blocks.iter().map(Block::approx_bytes).sum();
        let lines = self
            .cache
            .lock()
            .ok()
            .and_then(|c| {
                c.as_ref()
                    .map(|(_, l)| l.iter().map(NoteLine::approx_bytes).sum())
            })
            .unwrap_or(0);
        blocks + lines
    }
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

    fn lines(src: &str, cols: usize) -> Vec<String> {
        text_of(&wrap(&render_notes(src), cols))
    }

    fn with_links(src: &str) -> Rendered {
        let lookup = |id: &str| {
            (id == "sumo/5xx").then(|| (EntryType::Sumo, "5xx by authorizer".to_string()))
        };
        render_markdown(
            src,
            &Ctx {
                color: false,
                entry_links: true,
                lookup: Some(&lookup),
            },
        )
    }

    #[test]
    fn inline_styles_lists_quotes_and_code() {
        let src = "**b** *i* `c` [t](http://u) ~~s~~\n\n- one\n- two\n\n1. a\n2. b\n\n> q\n\n```\ncode\n```\n";
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
            vec![
                Class::Bold,
                Class::Italic,
                Class::Code,
                Class::Link,
                Class::Strike
            ]
        );
        let text: String = p.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "b i c t s", "the URL is not in the text");
        let link = p.iter().find(|s| s.class == Class::Link).unwrap();
        assert_eq!(link.url.as_deref(), Some("http://u"));
        assert!(
            matches!(&blocks[1], Block::List { ordered: false, items, .. } if items.len() == 2)
        );
        assert!(matches!(&blocks[2], Block::List { ordered: true, items, .. } if items.len() == 2));
        assert!(
            matches!(&blocks[3], Block::Quote(q) if matches!(&q[0], Block::Paragraph(p) if p[0].text == "q"))
        );
        assert!(matches!(&blocks[4], Block::Code(c) if c.lines == ["code"] && c.lang.is_none()));
    }

    #[test]
    fn headings_have_levels_and_rules() {
        let blocks = render_notes("# One\n\n## Two\n\n### Three\n\n#### Four\n");
        let levels: Vec<u8> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Heading { level, .. } => Some(*level),
                _ => None,
            })
            .collect();
        assert_eq!(levels, [1, 2, 3, 4]);
        let out = lines("# One\n\n## Two\n\n### Three\n", 40);
        assert_eq!(out[0], "One");
        assert!(out[1].starts_with('━'), "{out:?}");
        assert_eq!(out[3], "Two");
        assert!(out[4].starts_with('─'), "{out:?}");
        assert_eq!(out[6], "Three");
        assert_eq!(out.len(), 7, "no rule under level three: {out:?}");
    }

    #[test]
    fn unsupported_markup_and_html_become_text() {
        let out = lines("text with <b>html</b>\n\n<script>alert(1)</script>\n\n<div onclick=\"x()\">raw</div>\n", 80).join("\n");
        for want in [
            "<b>",
            "html",
            "<script>alert(1)</script>",
            "<div onclick=\"x()\">raw</div>",
        ] {
            assert!(out.contains(want), "{want}: {out}");
        }
        assert!(render_notes("").is_empty());
    }

    #[test]
    fn tables_get_box_borders_alignment_and_wrapping() {
        let src = "| Event | Action | TTL |\n|:------|:------:|----:|\n| Revoke | DEL | now |\n| Grant | SETEX | 300 s |\n";
        let out = lines(src, 60);
        assert_eq!(
            out,
            [
                "┌────────┬────────┬───────┐",
                "│ Event  │ Action │   TTL │",
                "├────────┼────────┼───────┤",
                "│ Revoke │  DEL   │   now │",
                "│ Grant  │ SETEX  │ 300 s │",
                "└────────┴────────┴───────┘",
            ]
        );
        // narrower: cells wrap, the table never exceeds the width
        let src = "| Name | Description |\n|---|---|\n| a | a long description that has to wrap across several lines |\n";
        let out = lines(src, 28);
        assert!(out.iter().all(|l| width(l) <= 28), "{out:?}");
        assert!(out.len() > 5, "wrapped: {out:?}");
        let joined = out.join("");
        assert!(
            joined.contains("long") && joined.contains("lines"),
            "{out:?}"
        );
        assert!(out[0].starts_with('┌') && out.last().unwrap().starts_with('└'));
        // wide characters count for two columns
        let out = lines("| 名前 | x |\n|---|---|\n| 日本語 | y |\n", 40);
        assert!(out.iter().all(|l| width(l) == width(&out[0])), "{out:?}");
    }

    #[test]
    fn a_table_too_wide_for_the_terminal_is_cut_not_overflowed() {
        let src = "| a | b | c | d | e | f |\n|---|---|---|---|---|---|\n| aaaaaaaaaaaa | bbbbbbbbbbbb | cccccccccccc | dddddddddddd | eeeeeeeeeeee | ffffffffffff |\n";
        for cols in [12usize, 20, 30, 80] {
            for l in lines(src, cols) {
                assert!(width(&l) <= cols.max(25), "{cols}: {l:?} ({})", width(&l));
            }
        }
    }

    #[test]
    fn task_lists_nested_lists_and_numbering() {
        let out = lines(
            "- [ ] open\n- [x] done *it*\n- plain\n  - nested\n\n3. three\n4. four\n",
            40,
        );
        assert_eq!(out[0], "☐ open");
        assert_eq!(out[1], "☑ done it");
        assert_eq!(out[2], "• plain");
        assert_eq!(out[3], "  • nested");
        assert!(
            out.contains(&"3. three".to_string()) && out.contains(&"4. four".to_string()),
            "{out:?}"
        );
        let blocks = render_notes("- [x] done\n");
        let wrapped = wrap(&blocks, 40);
        assert_eq!(
            wrapped[0][1].class,
            Class::Muted,
            "finished tasks are dimmed"
        );
    }

    #[test]
    fn callouts_have_an_icon_a_label_and_a_bar() {
        for (marker, label, icon) in [
            ("NOTE", "Note", "ℹ"),
            ("TIP", "Tip", "💡"),
            ("IMPORTANT", "Important", "❗"),
            ("WARNING", "Warning", "⚠"),
            ("CAUTION", "Caution", "🛑"),
        ] {
            let out = lines(&format!("> [!{marker}]\n> Be careful.\n"), 40);
            assert_eq!(out[0], format!("▌ {icon} {label}"), "{marker}");
            assert_eq!(out[1], "▌ Be careful.");
        }
        let blocks = render_notes("> [!WARNING]\n> x\n");
        assert!(matches!(
            &blocks[0],
            Block::Callout {
                kind: CalloutKind::Warning,
                ..
            }
        ));
        // a plain quote stays a quote
        assert_eq!(lines("> just a quote\n", 40), ["▏ just a quote"]);
    }

    #[test]
    fn code_is_framed_with_its_language() {
        let out = lines("```rust\nfn main() {}\nlet x = 1;\n```\n", 40);
        assert_eq!(out[0], "┌─ rust ───────┐");
        assert_eq!(out[1], "│ fn main() {} │");
        assert_eq!(out[2], "│ let x = 1;   │");
        assert_eq!(out[3], "└──────────────┘");
        let out = lines("```mermaid\ngraph TD; A-->B;\n```\n", 60);
        assert!(out[0].starts_with("┌─ mermaid "), "{out:?}");
        assert_eq!(out[1], "│ graph TD; A-->B; │");
        // long lines are cut inside the frame
        let long = format!("```\n{}\n```\n", "x".repeat(100));
        let out = lines(&long, 30);
        assert!(out.iter().all(|l| width(l) <= 30), "{out:?}");
        assert!(out[1].contains('›'), "{out:?}");
    }

    #[test]
    fn code_is_highlighted_only_when_a_line_is_drawn() {
        let blocks = render_markdown(
            "```python\nimport os\nprint(os.name)\n```\n",
            &Ctx {
                color: true,
                ..Ctx::default()
            },
        )
        .blocks;
        let Block::Code(code) = &blocks[0] else {
            panic!()
        };
        let laid_out = layout(&blocks, 40);
        assert!(!code.is_highlighted(), "laying out must not highlight");
        let spans = laid_out[1].spans();
        assert!(code.is_highlighted());
        assert!(spans.iter().any(|s| s.class == Class::Keyword), "{spans:?}");
        // the same text without color stays plain
        let blocks = render_notes("```python\nimport os\n```\n");
        let laid_out = layout(&blocks, 40);
        assert!(
            laid_out[1]
                .spans()
                .iter()
                .all(|s| s.class != Class::Keyword)
        );
    }

    #[test]
    fn footnotes_images_and_rules() {
        let out = lines(
            "Text[^a] and more[^b].\n\n[^b]: Second.\n[^a]: First.\n",
            40,
        );
        assert_eq!(out[0], "Text[1] and more[2].");
        let tail = out.join("\n");
        assert!(
            tail.contains("[1] First.") && tail.contains("[2] Second."),
            "{tail}"
        );
        let out = lines("![Alt text](path/to/img.png) and ![](x.png)\n", 60);
        assert_eq!(
            out[0],
            "[image: Alt text] (path/to/img.png) and [image: ] (x.png)"
        );
        assert!(
            lines("a\n\n---\n\nb\n", 40)
                .iter()
                .any(|l| l.starts_with('─'))
        );
    }

    #[test]
    fn entry_links_show_the_target_or_a_broken_marker() {
        let doc =
            with_links("See [[sumo/5xx]] and [[no/such]] and \\[[sumo/5xx]] and `[[sumo/5xx]]`.\n");
        let out = text_of(&wrap(&doc.blocks, 120));
        assert_eq!(
            out[0],
            "See [sumo] 5xx by authorizer and [?] no/such and [[sumo/5xx]] and [[sumo/5xx]]."
        );
        assert_eq!(doc.links.len(), 2);
        assert!(doc.links[0].target.is_some() && doc.links[1].target.is_none());
        let spans = wrap(&doc.blocks, 120).remove(0);
        assert!(
            spans
                .iter()
                .any(|s| s.class == Class::EntryBadge(EntryType::Sumo) && s.text == "[sumo]")
        );
        assert!(
            spans
                .iter()
                .any(|s| s.class == Class::Broken && s.text == "[?] no/such")
        );
        // other entries do not interpret the syntax
        let blocks = render_notes("[[sumo/5xx]]\n");
        assert_eq!(text_of(&wrap(&blocks, 40)), ["[[sumo/5xx]]"]);
        // a link repeated is listed once
        assert_eq!(with_links("[[sumo/5xx]] [[sumo/5xx]]").links.len(), 1);
    }

    #[test]
    fn control_characters_and_escape_sequences_never_reach_a_span() {
        let hostile = "# Title\x1b]0;pwned\x07\n\nClear \x1b[2J\x1b[H and \x1b]52;c;aGk=\x07 clip \u{9b}31m red.\n\n```\ncode \x1b[31mred\x1b[0m\n```\n\n| a\x1b[2J | b |\n|---|---|\n| \u{7}x | y |\n\n- item \x1b]8;;http://evil\x1b\\link\x1b]8;;\x1b\\\n\n[click](http://x\x1b[2J)\n";
        let doc = render_markdown(hostile, &Ctx::default());
        for line in layout(&doc.blocks, 60) {
            for s in line.spans().iter() {
                assert!(
                    s.text
                        .chars()
                        .all(|c| c == '\t' || c == '\n' || !c.is_control()),
                    "{:?}",
                    s.text
                );
            }
        }
        let all = wrap(&doc.blocks, 60);
        let text = text_of(&all).join("\n");
        assert!(
            text.contains('␛'),
            "sequences leave a visible marker: {text}"
        );
        assert!(!text.contains("pwned") || !text.contains('\u{1b}'));
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
        assert_eq!(lines("one\n\ntwo", 40), ["one", "", "two"]);
        assert_eq!(lines("a  \nb", 40), ["a", "b"]);
    }

    #[test]
    fn every_line_of_the_sample_note_fits_every_width() {
        let note = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/notes/role-cache-ttl.md"),
        )
        .unwrap();
        let body = note.split("---\n").nth(2).unwrap();
        let blocks = render_markdown(
            body,
            &Ctx {
                color: true,
                entry_links: true,
                lookup: None,
            },
        )
        .blocks;
        for cols in [10usize, 16, 24, 40, 80, 200] {
            for line in layout(&blocks, cols) {
                let text = line.text();
                assert!(
                    width(&text) <= cols.max(25),
                    "{cols}: {text:?} is {}",
                    width(&text)
                );
            }
        }
    }

    #[test]
    fn the_layout_is_kept_for_the_width_it_was_made_for() {
        let notes = Notes::new(render_markdown("one\n\ntwo\n", &Ctx::default()));
        let a = notes.lines(40);
        assert!(Arc::ptr_eq(&a, &notes.lines(40)));
        assert!(!Arc::ptr_eq(&a, &notes.lines(30)));
        assert!(notes.approx_bytes() > 0);
        assert_eq!(notes.clone(), notes);
    }
}
