//! Entries: parsing an entry file into typed fields, with findings for everything wrong.

pub mod link;

use crate::body::{Payload, parse_body};
use crate::frontmatter::{Split, SplitError, split};
use crate::lint::suggest::suggest;
use crate::lint::{Finding, RuleCode, Severity};
use crate::source::{Source, Span};
use crate::vars::{RefLocation, ScanTarget, Variable};
use crate::yaml::{self, Node};

/// Keys every entry may have.
const COMMON_KEYS: [&str; 7] = [
    "kb_format",
    "type",
    "title",
    "tags",
    "vars",
    "created",
    "updated",
];

/// Every key kb defines, for "did you mean" suggestions.
const ALL_KEYS: [&str; 16] = [
    "kb_format",
    "type",
    "title",
    "tags",
    "vars",
    "created",
    "updated",
    "dialect",
    "database",
    "range",
    "log_groups",
    "region",
    "shell",
    "cwd",
    "url",
    "code",
];

const TYPE_LIST: &str = "sql, sumo, cw, cmd, prompt, link, code, note";

/// The highest `kb_format` this version of kb reads and writes.
pub const MAX_KB_FORMAT: u32 = 1;

/// Longest allowed title, in characters.
pub const MAX_TITLE_CHARS: usize = 120;

/// The eight entry types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryType {
    Sql,
    Sumo,
    Cw,
    Cmd,
    Prompt,
    Link,
    Code,
    Note,
}

impl EntryType {
    /// How many entry types there are.
    pub const COUNT: usize = Self::ALL.len();

    /// Every entry type, in the order the format lists them.
    pub const ALL: [EntryType; 8] = [
        EntryType::Sql,
        EntryType::Sumo,
        EntryType::Cw,
        EntryType::Cmd,
        EntryType::Prompt,
        EntryType::Link,
        EntryType::Code,
        EntryType::Note,
    ];

    /// The name used in `type:`.
    pub fn name(self) -> &'static str {
        match self {
            EntryType::Sql => "sql",
            EntryType::Sumo => "sumo",
            EntryType::Cw => "cw",
            EntryType::Cmd => "cmd",
            EntryType::Prompt => "prompt",
            EntryType::Link => "link",
            EntryType::Code => "code",
            EntryType::Note => "note",
        }
    }

    /// The type named `name` in `type:`, if it is one of the eight.
    pub fn from_name(name: &str) -> Option<EntryType> {
        EntryType::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Whether the body's first fenced code block is the payload.
    pub fn has_payload(self) -> bool {
        !matches!(self, EntryType::Link | EntryType::Code | EntryType::Note)
    }

    /// The type-specific keys this type accepts.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            EntryType::Sql => &["dialect", "database"],
            EntryType::Sumo => &["range"],
            EntryType::Cw => &["log_groups", "range", "region"],
            EntryType::Cmd => &["shell", "cwd"],
            EntryType::Prompt => &[],
            EntryType::Link => &["url"],
            EntryType::Code => &["code"],
            EntryType::Note => &[],
        }
    }
}

/// The shell a `cmd` entry runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shell {
    #[default]
    Sh,
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    /// The values `shell:` accepts.
    pub const NAMES: [&'static str; 4] = ["sh", "bash", "zsh", "fish"];

    /// The shell named `name`, if kb supports it.
    pub fn from_name(name: &str) -> Option<Shell> {
        match name {
            "sh" => Some(Shell::Sh),
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            _ => None,
        }
    }

    /// The name of the shell as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Shell::Sh => "sh",
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
        }
    }
}

/// Where a `code` entry points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRef {
    pub repo: String,
    pub path: String,
    pub line: u32,
    pub git_ref: String,
    pub symbol: Option<String>,
    pub end_line: Option<u32>,
    pub branch: Option<String>,
}

/// Fields that depend on the entry type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeFields {
    Sql {
        dialect: Option<String>,
        database: Option<String>,
    },
    Sumo {
        range: Option<String>,
    },
    Cw {
        log_groups: Vec<String>,
        range: Option<String>,
        region: Option<String>,
    },
    Cmd {
        shell: Shell,
        cwd: Option<String>,
    },
    Prompt,
    Link {
        url: Option<String>,
    },
    Code {
        code: Option<CodeRef>,
    },
    Note,
    /// The type is missing or unknown.
    Unknown,
}

/// A parsed entry. It keeps the original text, so it can be written back byte for byte.
#[derive(Debug, Clone)]
pub struct Entry {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) source: Source,
    pub(crate) split: Split,
    pub(crate) tree: Node,
    pub(crate) kb_format: Option<u32>,
    pub(crate) entry_type: Option<EntryType>,
    pub(crate) title: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) created: Option<String>,
    pub(crate) updated: Option<String>,
    pub(crate) vars: Vec<Variable>,
    pub(crate) fields: TypeFields,
    pub(crate) payload: Option<Payload>,
    pub(crate) notes: String,
    pub(crate) unknown_keys: Vec<(String, Span)>,
}

/// The result of parsing one file.
#[derive(Debug, Clone)]
pub struct ParseOutcome {
    /// `None` when the file is structurally unreadable.
    pub entry: Option<Entry>,
    /// Structural and field findings (not secrets, not KB-wide checks).
    pub findings: Vec<Finding>,
}

/// Collects findings for one file, turning byte offsets into line and column.
pub(crate) struct Sink<'a> {
    pub path: &'a str,
    pub id: &'a str,
    pub source: &'a Source,
    pub findings: Vec<Finding>,
}

impl Sink<'_> {
    pub fn push(
        &mut self,
        code: RuleCode,
        offset: usize,
        message: impl Into<String>,
        fix: Option<String>,
    ) {
        let (line, column) = self.source.position(offset);
        self.findings.push(Finding {
            path: self.path.to_string(),
            id: Some(self.id.to_string()),
            line,
            column,
            severity: code.severity(),
            code,
            message: message.into(),
            fix,
        });
    }
}

/// Parses an entry file. `id` is the entry ID and `path` its path relative to the KB root.
pub fn parse_entry(id: &str, path: &str, bytes: Vec<u8>) -> ParseOutcome {
    match Source::from_bytes(bytes.clone()) {
        Ok(source) => parse_source(id, path, source),
        Err(e) => ParseOutcome {
            entry: None,
            findings: vec![invalid_utf8(id, path, &bytes, e.offset)],
        },
    }
}

/// The KB005 finding for a file that isn't UTF-8.
pub fn invalid_utf8(id: &str, path: &str, bytes: &[u8], offset: usize) -> Finding {
    let before = &bytes[..offset];
    let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
    let line_start = before
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let column = String::from_utf8_lossy(&before[line_start..])
        .trim_start_matches('\u{feff}')
        .chars()
        .count()
        + 1;
    Finding {
        path: path.to_string(),
        id: Some(id.to_string()),
        line: u32::try_from(line).unwrap_or(u32::MAX),
        column: u32::try_from(column).unwrap_or(u32::MAX),
        severity: Severity::Error,
        code: RuleCode::InvalidUtf8,
        message: format!("the file is not valid UTF-8 (first bad byte at offset {offset})"),
        fix: Some("re-save the file as UTF-8".into()),
    }
}

/// Parses an already-decoded entry.
pub fn parse_source(id: &str, path: &str, source: Source) -> ParseOutcome {
    let mut sink = Sink {
        path,
        id,
        source: &source,
        findings: Vec::new(),
    };
    let parsed = parse_parts(&mut sink);
    let mut findings = sink.findings;
    findings.sort_by_key(|f| (f.line, f.column, f.code));
    let Some(parts) = parsed else {
        return ParseOutcome {
            entry: None,
            findings,
        };
    };
    let entry = Entry {
        id: id.to_string(),
        path: path.to_string(),
        source,
        split: parts.split,
        tree: parts.tree,
        kb_format: parts.kb_format,
        entry_type: parts.entry_type,
        title: parts.title,
        tags: parts.tags,
        created: parts.created,
        updated: parts.updated,
        vars: parts.vars,
        fields: parts.fields,
        payload: parts.payload,
        notes: parts.notes,
        unknown_keys: parts.unknown_keys,
    };
    ParseOutcome {
        entry: Some(entry),
        findings,
    }
}

/// Everything in an [`Entry`] except the source, which the sink borrows while parsing.
struct Parts {
    split: Split,
    tree: Node,
    kb_format: Option<u32>,
    entry_type: Option<EntryType>,
    title: Option<String>,
    tags: Vec<String>,
    created: Option<String>,
    updated: Option<String>,
    vars: Vec<Variable>,
    fields: TypeFields,
    payload: Option<Payload>,
    notes: String,
    unknown_keys: Vec<(String, Span)>,
}

fn parse_parts(sink: &mut Sink<'_>) -> Option<Parts> {
    let text = sink.source.text();
    let split = match split(text) {
        Ok(split) => split,
        Err(e) => {
            let (message, fix) = match e {
                SplitError::NoFrontmatter => (
                    "the file has no frontmatter: its first line must be `---`",
                    "start the file with `---`, then `kb_format: 1`, `type:` and `title:` lines, then `---`",
                ),
                SplitError::Unclosed => (
                    "the frontmatter is never closed",
                    "add a `---` line after the frontmatter",
                ),
                SplitError::Empty => (
                    "the file is empty",
                    "copy one of the example entries from `kb init` and edit it",
                ),
            };
            sink.push(e.code(), 0, message, Some(fix.into()));
            return None;
        }
    };
    let parsed = yaml::parse(&text[split.frontmatter.clone()], split.frontmatter.start);
    for error in parsed.errors {
        let fix = match error.code {
            RuleCode::FrontmatterNotMapping => {
                Some("write the frontmatter as `key: value` lines".to_string())
            }
            RuleCode::UnsupportedYaml => Some("write the value out in full".to_string()),
            RuleCode::DuplicateKey => Some("remove one of them".to_string()),
            _ => None,
        };
        sink.push(error.code, error.offset, error.message, fix);
    }
    let tree = parsed.root?;

    let kb_format = kb_format(&tree, sink);
    let entry_type = entry_type(&tree, sink);
    let is_note = entry_type == Some(EntryType::Note);
    let title = if is_note {
        note_title(&tree, text, &split.body, sink)
    } else {
        title(&tree, sink)
    };
    let tags = tags(&tree, sink);
    let created = date(&tree, "created", sink);
    let updated = date(&tree, "updated", sink);
    let unknown_keys = check_keys(&tree, entry_type, sink);
    let fields = match entry_type {
        Some(t) => type_fields(t, &tree, sink),
        None => TypeFields::Unknown,
    };

    let has_payload = entry_type.is_some_and(EntryType::has_payload);
    let body = parse_body(text, split.body.clone(), has_payload);
    if has_payload {
        match &body.payload {
            None => sink.push(
                RuleCode::MissingPayload,
                split.body.start,
                format!(
                    "a {} entry needs its payload in a fenced code block",
                    entry_type.map_or("", EntryType::name)
                ),
                Some("add the query or command between ``` lines after the frontmatter".into()),
            ),
            Some(p) if p.text.trim().is_empty() => sink.push(
                RuleCode::EmptyPayload,
                p.fence_span.start,
                "the payload code block is empty",
                Some("put the query or command inside the code block".into()),
            ),
            Some(_) => {}
        }
    }

    if is_note {
        note_checks(&tree, &text[split.body.clone()], split.body.start, sink);
    }

    let mut targets = Vec::new();
    if let Some(p) = &body.payload {
        targets.push(ScanTarget {
            location: RefLocation::Payload,
            span: p.raw_span.clone(),
        });
    }
    if let Some(t) = entry_type {
        for key in ["range", "url", "cwd"] {
            if t.keys().contains(&key)
                && let Some(node) = tree.get(key).filter(|n| n.as_str().is_some())
            {
                targets.push(ScanTarget {
                    location: RefLocation::Field(key.into()),
                    span: node.span().clone(),
                });
            }
        }
        if t == EntryType::Cw
            && let Some(Node::Seq { items, .. }) = tree.get("log_groups")
        {
            for (i, item) in items
                .iter()
                .enumerate()
                .filter(|(_, n)| n.as_str().is_some())
            {
                targets.push(ScanTarget {
                    location: RefLocation::Field(format!("log_groups[{i}]")),
                    span: item.span().clone(),
                });
            }
        }
    }
    // Notes do not interpolate: `{{x}}` in a note is just text, and `vars:` is an error.
    let vars = if is_note {
        Vec::new()
    } else {
        crate::vars::collect(&tree, text, &targets, sink)
    };

    Some(Parts {
        split,
        tree,
        kb_format,
        entry_type,
        title,
        tags,
        created,
        updated,
        vars,
        fields,
        payload: body.payload,
        notes: body.notes,
        unknown_keys,
    })
}

fn kb_format(tree: &Node, sink: &mut Sink<'_>) -> Option<u32> {
    let Some(node) = tree.get("kb_format") else {
        sink.push(
            RuleCode::MissingKbFormat,
            0,
            "missing `kb_format`",
            Some("add `kb_format: 1`".into()),
        );
        return None;
    };
    match node
        .as_int()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v >= 1)
    {
        Some(v) if v > MAX_KB_FORMAT => {
            sink.push(
                RuleCode::UnsupportedKbFormat,
                node.span().start,
                format!("`kb_format: {v}` is newer than this version of kb supports (up to {MAX_KB_FORMAT}); kb will not rewrite this file"),
                Some("upgrade kb to read this entry".into()),
            );
            Some(v)
        }
        Some(v) => Some(v),
        None => {
            sink.push(
                RuleCode::InvalidKbFormat,
                node.span().start,
                "`kb_format` must be a whole number, 1 or more",
                Some(format!("`kb_format: {MAX_KB_FORMAT}`")),
            );
            None
        }
    }
}

fn entry_type(tree: &Node, sink: &mut Sink<'_>) -> Option<EntryType> {
    let Some(node) = tree.get("type") else {
        sink.push(
            RuleCode::MissingType,
            0,
            "missing `type`",
            Some(format!("add `type:` with one of {TYPE_LIST}")),
        );
        return None;
    };
    let Some(name) = node.as_str() else {
        sink.push(
            RuleCode::WrongValueType,
            node.span().start,
            format!("`type` must be text, not a {}", node.kind_name()),
            Some(format!("use one of {TYPE_LIST}")),
        );
        return None;
    };
    let found = EntryType::from_name(name);
    if found.is_none() {
        let names: Vec<&str> = EntryType::ALL.iter().map(|t| t.name()).collect();
        let fix = match suggest(name, &names) {
            Some(s) => format!("use \"{s}\""),
            None => format!("use one of {TYPE_LIST}"),
        };
        sink.push(
            RuleCode::UnknownType,
            node.span().start,
            format!("unknown type \"{name}\"; expected one of {TYPE_LIST}"),
            Some(fix),
        );
    }
    found
}

fn title(tree: &Node, sink: &mut Sink<'_>) -> Option<String> {
    let Some(node) = tree.get("title") else {
        sink.push(
            RuleCode::MissingTitle,
            0,
            "missing `title`",
            Some("add `title:` with a short description".into()),
        );
        return None;
    };
    let at = node.span().start;
    let Some(title) = node.as_str() else {
        sink.push(
            RuleCode::InvalidTitle,
            at,
            format!("`title` must be text, not a {}", node.kind_name()),
            Some("quote it: `title: \"...\"`".into()),
        );
        return None;
    };
    let chars = title.chars().count();
    if title.trim().is_empty() {
        sink.push(
            RuleCode::InvalidTitle,
            at,
            "`title` is empty",
            Some("write a short title".into()),
        );
        None
    } else if chars > MAX_TITLE_CHARS {
        sink.push(
            RuleCode::InvalidTitle,
            at,
            format!("`title` is {chars} characters long; the limit is {MAX_TITLE_CHARS}"),
            Some("shorten it and move the details into the notes".into()),
        );
        None
    } else {
        // the title is drawn in lists and headers: no escape sequence may get that far
        Some(crate::terminal::display(title).into_owned())
    }
}

/// Largest note body, in bytes, before `KB072-note-oversized`.
pub const NOTE_OVERSIZED_BYTES: usize = 65_536;

/// A note's title: `title:` when present, else its first line when that is a level-1 heading.
fn note_title(tree: &Node, text: &str, body: &Span, sink: &mut Sink<'_>) -> Option<String> {
    // an empty `title:` (a fresh template) means "use the heading", like a missing one
    let given = tree
        .get("title")
        .filter(|n| !n.is_null() && n.as_str().is_none_or(|s| !s.trim().is_empty()));
    if given.is_some() {
        return title(tree, sink);
    }
    let found = note_title_from_body(&text[body.clone()]);
    if found.is_none() {
        sink.push(
            RuleCode::NoteTitle,
            0,
            "a note needs a `title` or a level-1 heading (`# Title`) at the start of its body",
            Some(
                "add `title: \"...\"` to the frontmatter, or start the body with `# Title`".into(),
            ),
        );
    }
    found
}

/// The plain-text title of a note body: its first non-empty line, when that is a level-1 heading,
/// with inline Markdown (code ticks, emphasis, links, strikethrough) stripped and whitespace
/// collapsed. `None` when there is no such heading or it has no text.
pub fn note_title_from_body(body: &str) -> Option<String> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let line = body.lines().find(|l| !l.trim().is_empty())?;
    let mut out = String::new();
    let mut in_h1 = false;
    for event in Parser::new_ext(line, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(Tag::Heading {
                level: pulldown_cmark::HeadingLevel::H1,
                ..
            }) => in_h1 = true,
            Event::End(TagEnd::Heading(_)) => break,
            Event::Text(t) | Event::Code(t) if in_h1 => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak if in_h1 => out.push(' '),
            _ if !in_h1 => return None,
            _ => {}
        }
    }
    let title = out.split_whitespace().collect::<Vec<_>>().join(" ");
    (!title.is_empty()).then_some(title)
}

/// `KB071`, `KB072` and `KB073`: the rules that only apply to notes.
fn note_checks(tree: &Node, body: &str, body_start: usize, sink: &mut Sink<'_>) {
    if let Some((key, _)) = tree.entry("vars") {
        sink.push(
            RuleCode::NoteVars,
            key.span.start,
            "a note cannot declare `vars` (notes do not support variable interpolation)",
            Some("remove `vars:`".into()),
        );
    }
    if body.len() > NOTE_OVERSIZED_BYTES {
        sink.push(
            RuleCode::NoteOversized,
            body_start,
            format!(
                "the note body is {} bytes; over {NOTE_OVERSIZED_BYTES} is hard to read and search",
                body.len()
            ),
            Some("split it into separate linked entries".into()),
        );
    }
    if body.trim().is_empty() {
        sink.push(
            RuleCode::NoteEmpty,
            body_start,
            "the note has no body",
            Some("add content below the frontmatter".into()),
        );
    }
}

fn tags(tree: &Node, sink: &mut Sink<'_>) -> Vec<String> {
    let mut tags = Vec::new();
    let Some(node) = tree.get("tags").filter(|n| !n.is_null()) else {
        return tags;
    };
    let Node::Seq { items, .. } = node else {
        sink.push(
            RuleCode::WrongValueType,
            node.span().start,
            format!("`tags` must be a list, not a {}", node.kind_name()),
            Some("write `tags: [first, second]`".into()),
        );
        return tags;
    };
    for item in items {
        let at = item.span().start;
        let Some(raw) = item.as_str() else {
            sink.push(
                RuleCode::InvalidTag,
                at,
                format!("tags must be text, not a {}", item.kind_name()),
                Some("remove it, or quote it".into()),
            );
            continue;
        };
        let tag = raw.trim();
        let tag =
            crate::terminal::display(tag.strip_prefix('#').unwrap_or(tag).trim()).to_lowercase();
        if tag.is_empty() {
            sink.push(
                RuleCode::InvalidTag,
                at,
                "the tag is empty",
                Some("remove it".into()),
            );
        } else if tags.contains(&tag) {
            sink.push(
                RuleCode::DuplicateTag,
                at,
                format!("tag `{tag}` appears more than once"),
                Some("remove the duplicate".into()),
            );
        } else {
            tags.push(tag);
        }
    }
    tags
}

fn date(tree: &Node, key: &str, sink: &mut Sink<'_>) -> Option<String> {
    let node = tree.get(key).filter(|n| !n.is_null())?;
    match node.as_str().filter(|s| is_valid_date(s)) {
        Some(s) => Some(s.to_string()),
        None => {
            sink.push(
                RuleCode::InvalidDate,
                node.span().start,
                format!("`{key}` is not a valid date"),
                Some(
                    "use `YYYY-MM-DD` or an RFC 3339 date-time such as `2026-10-08T14:30:00Z`"
                        .into(),
                ),
            );
            None
        }
    }
}

/// `YYYY-MM-DD`, or an RFC 3339 date-time (`YYYY-MM-DDTHH:MM[:SS[.f]]` plus `Z` or `±HH:MM`).
pub fn is_valid_date(s: &str) -> bool {
    let (date, time) = match s.split_once(['T', 't']) {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    valid_ymd(date) && time.is_none_or(valid_time)
}

fn num(s: &str) -> Option<u32> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

fn valid_ymd(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return false;
    };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return false;
    }
    let (Some(y), Some(m), Some(d)) = (num(y), num(m), num(d)) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

fn valid_time(s: &str) -> bool {
    let (clock, offset) = if let Some(c) = s.strip_suffix(['Z', 'z']) {
        (c, None)
    } else if let Some(i) = s.rfind(['+', '-']) {
        (&s[..i], Some(&s[i + 1..]))
    } else {
        return false;
    };
    if let Some(off) = offset {
        let Some((h, m)) = off.split_once(':') else {
            return false;
        };
        if h.len() != 2
            || m.len() != 2
            || !num(h).is_some_and(|h| h < 24)
            || !num(m).is_some_and(|m| m < 60)
        {
            return false;
        }
    }
    let (hms, frac) = match clock.split_once('.') {
        Some((a, f)) => (a, Some(f)),
        None => (clock, None),
    };
    if frac
        .is_some_and(|f| num(f).is_none() && !f.bytes().all(|b| b.is_ascii_digit()) || f.is_empty())
    {
        return false;
    }
    let parts: Vec<&str> = hms.split(':').collect();
    let two = |p: &str, max: u32| p.len() == 2 && num(p).is_some_and(|v| v <= max);
    match parts.as_slice() {
        [h, m] => frac.is_none() && two(h, 23) && two(m, 59),
        [h, m, sec] => two(h, 23) && two(m, 59) && two(sec, 60),
        _ => false,
    }
}

/// Reports unknown keys (KB016) and keys of another type (KB017). Returns the unknown keys.
fn check_keys(
    tree: &Node,
    entry_type: Option<EntryType>,
    sink: &mut Sink<'_>,
) -> Vec<(String, Span)> {
    let mut unknown = Vec::new();
    for (key, _) in tree.entries() {
        let name = key.name.as_str();
        if COMMON_KEYS.contains(&name) {
            continue;
        }
        let owners: Vec<&str> = EntryType::ALL
            .iter()
            .filter(|t| t.keys().contains(&name))
            .map(|t| t.name())
            .collect();
        if owners.is_empty() {
            let fix = suggest(name, &ALL_KEYS).map(|s| format!("did you mean `{s}`?"));
            sink.push(
                RuleCode::UnknownKey,
                key.span.start,
                format!("unknown key `{name}`; kb keeps it but doesn't use it"),
                fix,
            );
            unknown.push((key.name.clone(), key.span.clone()));
        } else if let Some(t) = entry_type
            && !t.keys().contains(&name)
        {
            sink.push(
                RuleCode::FieldNotForType,
                key.span.start,
                format!(
                    "`{name}` is used by {} entries, not {} entries",
                    owners.join(" and "),
                    t.name()
                ),
                Some("remove it, or change `type`".into()),
            );
        }
    }
    unknown
}

/// A text field: `None` when absent or null, KB018 when it isn't text.
fn text_field(tree: &Node, key: &str, sink: &mut Sink<'_>) -> Option<String> {
    let node = tree.get(key).filter(|n| !n.is_null())?;
    match node.as_str() {
        Some(s) => Some(s.to_string()),
        None => {
            sink.push(
                RuleCode::WrongValueType,
                node.span().start,
                format!("`{key}` must be text, not a {}", node.kind_name()),
                Some(format!("quote it: `{key}: \"...\"`")),
            );
            None
        }
    }
}

fn type_fields(t: EntryType, tree: &Node, sink: &mut Sink<'_>) -> TypeFields {
    match t {
        EntryType::Sql => TypeFields::Sql {
            dialect: text_field(tree, "dialect", sink),
            database: text_field(tree, "database", sink),
        },
        EntryType::Sumo => TypeFields::Sumo {
            range: text_field(tree, "range", sink),
        },
        EntryType::Cw => TypeFields::Cw {
            log_groups: log_groups(tree, sink),
            range: text_field(tree, "range", sink),
            region: text_field(tree, "region", sink),
        },
        EntryType::Cmd => TypeFields::Cmd {
            shell: shell(tree, sink),
            cwd: text_field(tree, "cwd", sink),
        },
        EntryType::Prompt => TypeFields::Prompt,
        EntryType::Link => TypeFields::Link {
            url: url(tree, sink),
        },
        EntryType::Code => TypeFields::Code {
            code: code_ref(tree, sink),
        },
        EntryType::Note => TypeFields::Note,
    }
}

fn log_groups(tree: &Node, sink: &mut Sink<'_>) -> Vec<String> {
    let Some(node) = tree.get("log_groups").filter(|n| !n.is_null()) else {
        return Vec::new();
    };
    let Node::Seq { items, .. } = node else {
        sink.push(
            RuleCode::WrongValueType,
            node.span().start,
            format!("`log_groups` must be a list, not a {}", node.kind_name()),
            Some("write `log_groups: [/aws/lambda/name]`".into()),
        );
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        match item.as_str() {
            Some(s) => out.push(s.to_string()),
            None => sink.push(
                RuleCode::WrongValueType,
                item.span().start,
                format!("log groups must be text, not a {}", item.kind_name()),
                Some("quote it".into()),
            ),
        }
    }
    out
}

fn shell(tree: &Node, sink: &mut Sink<'_>) -> Shell {
    let Some(name) = text_field(tree, "shell", sink) else {
        return Shell::Sh;
    };
    match Shell::from_name(&name) {
        Some(shell) => shell,
        None => {
            let at = tree.get("shell").map_or(0, |n| n.span().start);
            let fix = match suggest(&name, &Shell::NAMES) {
                Some(s) => format!("use \"{s}\""),
                None => "use one of sh, bash, zsh, fish".into(),
            };
            sink.push(
                RuleCode::InvalidShell,
                at,
                format!("unknown shell \"{name}\"; expected sh, bash, zsh or fish"),
                Some(fix),
            );
            Shell::Sh
        }
    }
}

fn url(tree: &Node, sink: &mut Sink<'_>) -> Option<String> {
    if tree.get("url").is_none_or(Node::is_null) {
        sink.push(
            RuleCode::MissingUrl,
            0,
            "a link entry needs a `url`",
            Some("add `url: https://…`".into()),
        );
        return None;
    }
    let url = text_field(tree, "url", sink)?;
    if is_absolute_http_url(&url) {
        Some(url)
    } else {
        let at = tree.get("url").map_or(0, |n| n.span().start);
        sink.push(
            RuleCode::InvalidUrl,
            at,
            "`url` must be an absolute http or https URL",
            Some("write it as `https://host/path`".into()),
        );
        None
    }
}

/// True for `http://host…` or `https://host…`. Variable references count as plain text.
pub fn is_absolute_http_url(url: &str) -> bool {
    let mut text = url.to_string();
    for brace in crate::vars::scan_refs(url).into_iter().rev() {
        if let crate::vars::Brace::Ref { span, .. } = brace {
            text.replace_range(span, "x");
        }
    }
    let lower = text.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    rest.is_some_and(|r| {
        let host = r.split(['/', '?', '#']).next().unwrap_or("");
        !host.is_empty() && !text.contains(char::is_whitespace)
    })
}

fn code_ref(tree: &Node, sink: &mut Sink<'_>) -> Option<CodeRef> {
    let Some((key, node)) = tree.entry("code").filter(|(_, n)| !n.is_null()) else {
        sink.push(
            RuleCode::MissingCodeRef,
            0,
            "a code entry needs a `code` map",
            Some("add `code:` with `repo: owner/name`, `path:`, `line:` and `ref:`".into()),
        );
        return None;
    };
    if !matches!(node, Node::Map { .. }) {
        sink.push(
            RuleCode::WrongValueType,
            node.span().start,
            format!("`code` must be a mapping, not a {}", node.kind_name()),
            Some("write `code:` with indented `repo:`, `path:`, `line:` and `ref:` lines".into()),
        );
        return None;
    }
    let key_at = key.span.start;
    let bad = |sink: &mut Sink<'_>, at: usize, message: String, fix: &str| {
        sink.push(RuleCode::InvalidCodeRef, at, message, Some(fix.to_string()));
    };
    let mut ok = true;

    let repo = match node.get("repo") {
        None => {
            bad(
                sink,
                key_at,
                "`code.repo` is missing".into(),
                "add `repo: owner/name`",
            );
            None
        }
        Some(n) => match n.as_str().filter(|r| is_valid_repo(r)) {
            Some(r) => Some(r.to_string()),
            None => {
                bad(
                    sink,
                    n.span().start,
                    "`code.repo` must be `owner/name` or a git remote URL".into(),
                    "write it like `owner/name`",
                );
                None
            }
        },
    };
    let path = match node.get("path") {
        None => {
            bad(
                sink,
                key_at,
                "`code.path` is missing".into(),
                "add `path:` relative to the repo root",
            );
            None
        }
        Some(n) => match n.as_str().filter(|p| is_valid_repo_path(p)) {
            Some(p) => Some(p.to_string()),
            None => {
                bad(
                    sink,
                    n.span().start,
                    "`code.path` must be relative to the repo root, without `..`".into(),
                    "write it like `src/main.rs`",
                );
                None
            }
        },
    };
    let line = match node.get("line") {
        None => {
            bad(
                sink,
                key_at,
                "`code.line` is missing".into(),
                "add `line:` with a line number",
            );
            None
        }
        Some(n) => match positive(n) {
            Some(l) => Some(l),
            None => {
                bad(
                    sink,
                    n.span().start,
                    "`code.line` must be a whole number, 1 or more".into(),
                    "use a line number such as `line: 1`",
                );
                None
            }
        },
    };
    let git_ref = match node.get("ref") {
        None => {
            bad(
                sink,
                key_at,
                "`code.ref` is missing".into(),
                "add `ref:` with a commit SHA, tag or branch",
            );
            None
        }
        Some(n) => match n.scalar().filter(|r| !n.is_null() && !r.trim().is_empty()) {
            Some(r) => Some(r.to_string()),
            None => {
                bad(
                    sink,
                    n.span().start,
                    "`code.ref` must be a commit SHA, tag or branch".into(),
                    "write it like `ref: main`",
                );
                None
            }
        },
    };
    let symbol = node
        .get("symbol")
        .filter(|n| !n.is_null())
        .and_then(Node::scalar)
        .map(str::to_string);
    let mut end_line = None;
    if let Some(n) = node.get("end_line").filter(|n| !n.is_null()) {
        match positive(n) {
            Some(e) if line.is_some_and(|l| e < l) => {
                bad(
                    sink,
                    n.span().start,
                    "`code.end_line` is before `code.line`".into(),
                    "make `end_line` equal to or after `line`",
                );
                ok = false;
            }
            Some(e) => end_line = Some(e),
            None => {
                bad(
                    sink,
                    n.span().start,
                    "`code.end_line` must be a whole number, 1 or more".into(),
                    "use a line number",
                );
                ok = false;
            }
        }
    }
    let branch = node
        .get("branch")
        .filter(|n| !n.is_null())
        .and_then(Node::scalar)
        .map(str::to_string);
    match (repo, path, line, git_ref) {
        (Some(repo), Some(path), Some(line), Some(git_ref)) if ok => Some(CodeRef {
            repo,
            path,
            line,
            git_ref,
            symbol,
            end_line,
            branch,
        }),
        _ => None,
    }
}

fn positive(node: &Node) -> Option<u32> {
    node.as_int()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v >= 1)
}

/// `owner/name`, or a git remote URL.
pub fn is_valid_repo(repo: &str) -> bool {
    const REMOTES: [&str; 6] = ["https://", "http://", "ssh://", "git://", "git@", "file://"];
    if REMOTES.iter().any(|p| repo.starts_with(p)) {
        return repo.len() > repo.find(['/', '@']).map_or(0, |i| i + 1)
            && !repo.contains(char::is_whitespace);
    }
    let part = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    matches!(repo.split_once('/'), Some((owner, name)) if part(owner) && part(name))
}

fn is_valid_repo_path(path: &str) -> bool {
    !path.trim().is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.split('/').any(|c| c == "..")
}

impl Entry {
    /// The entry ID: its path relative to the root, without `.md`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Path relative to the KB root.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The original text of the file.
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// The `kb_format` value, when it is a valid number.
    pub fn kb_format(&self) -> Option<u32> {
        self.kb_format
    }

    /// `None` when the type is missing or unknown.
    pub fn entry_type(&self) -> Option<EntryType> {
        self.entry_type
    }

    /// The title, when it is valid.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Tags, lowercased and without a leading `#`.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// The `created` date as written, when valid.
    pub fn created(&self) -> Option<&str> {
        self.created.as_deref()
    }

    /// The `updated` date as written, when valid.
    pub fn updated(&self) -> Option<&str> {
        self.updated.as_deref()
    }

    /// Declared variables, then undeclared references, in order of first appearance.
    pub fn vars(&self) -> &[Variable] {
        &self.vars
    }

    /// The type-specific fields.
    pub fn fields(&self) -> &TypeFields {
        &self.fields
    }

    /// The payload, for types that have one.
    pub fn payload(&self) -> Option<&Payload> {
        self.payload.as_ref()
    }

    /// The body text outside the payload.
    pub fn notes(&self) -> &str {
        &self.notes
    }

    /// Top-level keys kb doesn't define, kept as they are.
    pub fn unknown_keys(&self) -> impl Iterator<Item = &str> {
        self.unknown_keys.iter().map(|(k, _)| k.as_str())
    }

    /// The frontmatter tree.
    pub fn tree(&self) -> &Node {
        &self.tree
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses `text` as `x.md` and returns (line, column, code) for every finding.
    fn codes(text: &str) -> Vec<(u32, u32, RuleCode)> {
        parse_entry("x", "x.md", text.as_bytes().to_vec())
            .findings
            .iter()
            .map(|f| (f.line, f.column, f.code))
            .collect()
    }

    fn one(text: &str) -> Finding {
        let out = parse_entry("x", "x.md", text.as_bytes().to_vec());
        assert_eq!(out.findings.len(), 1, "{:#?}", out.findings);
        out.findings.into_iter().next().unwrap()
    }

    fn clean(text: &str) -> Entry {
        let out = parse_entry("x", "x.md", text.as_bytes().to_vec());
        assert!(out.findings.is_empty(), "{:#?}", out.findings);
        out.entry.unwrap()
    }

    const SQL_BODY: &str = "\n```\nselect 1;\n```\n";

    fn sql(front: &str) -> String {
        format!("---\nkb_format: 1\ntype: sql\ntitle: T\n{front}---\n{SQL_BODY}")
    }

    #[test]
    fn minimal_entry() {
        let e = clean(&sql(""));
        assert_eq!(e.kb_format(), Some(1));
        assert_eq!(e.entry_type(), Some(EntryType::Sql));
        assert_eq!(e.title(), Some("T"));
        assert_eq!(e.payload().unwrap().text, "select 1;\n");
        assert_eq!(
            e.fields(),
            &TypeFields::Sql {
                dialect: None,
                database: None
            }
        );
    }

    #[test]
    fn structural_errors_have_no_entry() {
        let out = parse_entry("x", "x.md", b"no frontmatter\n".to_vec());
        assert!(out.entry.is_none());
        assert_eq!(
            codes("no frontmatter\n"),
            vec![(1, 1, RuleCode::NoFrontmatter)]
        );
        assert_eq!(
            codes("---\na: 1\n"),
            vec![(1, 1, RuleCode::UnclosedFrontmatter)]
        );
        assert_eq!(codes(" \n"), vec![(1, 1, RuleCode::EmptyFile)]);
    }

    #[test]
    fn invalid_utf8_position() {
        let out = parse_entry("x", "x.md", b"---\nab\xFF\n".to_vec());
        let f = &out.findings[0];
        assert_eq!((f.line, f.column, f.code), (2, 3, RuleCode::InvalidUtf8));
        assert!(f.message.contains("offset 6"), "{}", f.message);
    }

    #[test]
    fn yaml_errors_are_positioned() {
        assert_eq!(
            codes("---\nkb_format: 1\nkb_format: 1\ntype: sql\ntitle: T\n---\n\n```\nx\n```\n"),
            vec![(3, 1, RuleCode::DuplicateKey)]
        );
        let f = one("---\n- a\n---\n");
        assert_eq!((f.line, f.code), (2, RuleCode::FrontmatterNotMapping));
    }

    #[test]
    fn kb_format_rules() {
        let f = one("---\ntype: link\ntitle: T\nurl: https://example.com\n---\n");
        assert_eq!(
            (f.line, f.column, f.code),
            (1, 1, RuleCode::MissingKbFormat)
        );
        assert_eq!(f.fix.as_deref(), Some("add `kb_format: 1`"));
        for bad in ["\"1\"", "0", "-1", "1.5", "one"] {
            let f = one(&format!(
                "---\nkb_format: {bad}\ntype: link\ntitle: T\nurl: https://example.com\n---\n"
            ));
            assert_eq!(
                (f.line, f.column, f.code),
                (2, 12, RuleCode::InvalidKbFormat),
                "{bad}"
            );
        }
        let f = one("---\nkb_format: 2\ntype: link\ntitle: T\nurl: https://example.com\n---\n");
        assert_eq!(
            (f.line, f.column, f.code),
            (2, 12, RuleCode::UnsupportedKbFormat)
        );
        assert!(f.fix.unwrap().contains("upgrade kb"));
    }

    #[test]
    fn unknown_type_lists_types_and_suggests() {
        let f = one("---\nkb_format: 1\ntype: sumologic\ntitle: T\n---\n");
        assert_eq!((f.line, f.column, f.code), (3, 7, RuleCode::UnknownType));
        assert!(
            f.message.contains("sql, sumo, cw, cmd, prompt, link, code"),
            "{}",
            f.message
        );
        assert_eq!(f.fix.as_deref(), Some("use \"sumo\""));
    }

    #[test]
    fn missing_type_and_title() {
        assert_eq!(
            codes("---\nkb_format: 1\ntitle: T\n---\n"),
            vec![(1, 1, RuleCode::MissingType)]
        );
        assert_eq!(
            codes("---\nkb_format: 1\ntype: link\nurl: https://example.com\n---\n"),
            vec![(1, 1, RuleCode::MissingTitle)]
        );
    }

    #[test]
    fn title_rules() {
        let long = "x".repeat(MAX_TITLE_CHARS + 1);
        for (title, col) in [
            ("\"\"", 8),
            ("\"  \"", 8),
            (long.as_str(), 8),
            ("404", 8),
            ("[a]", 8),
        ] {
            let f = one(&format!(
                "---\nkb_format: 1\ntype: link\ntitle: {title}\nurl: https://example.com\n---\n"
            ));
            assert_eq!(
                (f.line, f.column, f.code),
                (4, col, RuleCode::InvalidTitle),
                "{title}"
            );
        }
        let exact = "é".repeat(MAX_TITLE_CHARS);
        clean(&format!(
            "---\nkb_format: 1\ntype: link\ntitle: {exact}\nurl: https://example.com\n---\n"
        ));
    }

    #[test]
    fn tags_are_normalized() {
        let e = clean(&sql("tags: [\"#Auth\", Lambda, prod]\n"));
        assert_eq!(e.tags(), &["auth", "lambda", "prod"]);
    }

    #[test]
    fn tag_rules() {
        assert_eq!(
            codes(&sql("tags: lambda\n")),
            vec![(5, 7, RuleCode::WrongValueType)]
        );
        assert_eq!(
            codes(&sql("tags: [auth, \"#AUTH\"]\n")),
            vec![(5, 14, RuleCode::DuplicateTag)]
        );
        assert_eq!(
            codes(&sql("tags: [auth, 42]\n")),
            vec![(5, 14, RuleCode::InvalidTag)]
        );
        assert_eq!(
            codes(&sql("tags: [\"#\"]\n")),
            vec![(5, 8, RuleCode::InvalidTag)]
        );
    }

    #[test]
    fn dates() {
        clean(&sql("created: 2024-02-29\nupdated: 2026-10-08T14:30:00Z\n"));
        clean(&sql("updated: 2026-10-08T14:30:00.123+02:00\n"));
        clean(&sql("updated: 2026-10-08T14:30-05:00\n"));
        for bad in [
            "2026-02-30",
            "2023-02-29",
            "2026-13-01",
            "26-10-08",
            "2026-10-08T25:00Z",
            "2026-10-08T10:00",
            "yesterday",
        ] {
            assert_eq!(
                codes(&sql(&format!("updated: {bad}\n"))),
                vec![(5, 10, RuleCode::InvalidDate)],
                "{bad}"
            );
        }
    }

    #[test]
    fn unknown_key_is_info_and_kept() {
        let out = parse_entry("x", "x.md", sql("tilte: oops\nowner: me\n").into_bytes());
        let found: Vec<_> = out
            .findings
            .iter()
            .map(|f| (f.line, f.code, f.fix.clone()))
            .collect();
        assert_eq!(
            found,
            vec![
                (
                    5,
                    RuleCode::UnknownKey,
                    Some("did you mean `title`?".to_string())
                ),
                (6, RuleCode::UnknownKey, None),
            ]
        );
        assert_eq!(out.findings[0].severity, Severity::Info);
        let e = out.entry.unwrap();
        assert_eq!(e.unknown_keys().collect::<Vec<_>>(), vec!["tilte", "owner"]);
    }

    #[test]
    fn field_for_other_type_is_warning() {
        let f = one(&sql("url: https://example.com\n"));
        assert_eq!(
            (f.line, f.column, f.code, f.severity),
            (5, 1, RuleCode::FieldNotForType, Severity::Warning)
        );
        assert!(f.message.contains("link"), "{}", f.message);
    }

    #[test]
    fn type_fields() {
        let e = clean(&sql("dialect: postgres\ndatabase: app\n"));
        assert_eq!(
            e.fields(),
            &TypeFields::Sql {
                dialect: Some("postgres".into()),
                database: Some("app".into())
            }
        );
        let e = clean(
            "---\nkb_format: 1\ntype: cw\ntitle: T\nlog_groups: [/aws/a, /aws/b]\nregion: us-east-1\nrange: -1h\n---\n\n```\nfields @message\n```\n",
        );
        assert_eq!(
            e.fields(),
            &TypeFields::Cw {
                log_groups: vec!["/aws/a".into(), "/aws/b".into()],
                range: Some("-1h".into()),
                region: Some("us-east-1".into())
            }
        );
        let e = clean(
            "---\nkb_format: 1\ntype: cmd\ntitle: T\nshell: zsh\ncwd: /tmp\n---\n\n```\nls\n```\n",
        );
        assert_eq!(
            e.fields(),
            &TypeFields::Cmd {
                shell: Shell::Zsh,
                cwd: Some("/tmp".into())
            }
        );
        let e = clean("---\nkb_format: 1\ntype: cmd\ntitle: T\n---\n\n```\nls\n```\n");
        assert_eq!(
            e.fields(),
            &TypeFields::Cmd {
                shell: Shell::Sh,
                cwd: None
            }
        );
        assert_eq!(
            codes(
                "---\nkb_format: 1\ntype: cw\ntitle: T\nlog_groups: /aws/a\n---\n\n```\nx\n```\n"
            ),
            vec![(5, 13, RuleCode::WrongValueType)]
        );
    }

    #[test]
    fn shell_rule() {
        let f = one("---\nkb_format: 1\ntype: cmd\ntitle: T\nshell: bsh\n---\n\n```\nls\n```\n");
        assert_eq!((f.line, f.column, f.code), (5, 8, RuleCode::InvalidShell));
        assert_eq!(f.fix.as_deref(), Some("use \"sh\""));
    }

    #[test]
    fn link_rules() {
        let e = clean("---\nkb_format: 1\ntype: link\ntitle: T\nurl: https://example.com/a\n---\n");
        assert_eq!(
            e.fields(),
            &TypeFields::Link {
                url: Some("https://example.com/a".into())
            }
        );
        assert!(e.payload().is_none());
        assert_eq!(
            codes("---\nkb_format: 1\ntype: link\ntitle: T\n---\n"),
            vec![(1, 1, RuleCode::MissingUrl)]
        );
        for bad in ["ftp://x", "example.com", "https://", "http:///x"] {
            assert_eq!(
                codes(&format!(
                    "---\nkb_format: 1\ntype: link\ntitle: T\nurl: {bad}\n---\n"
                )),
                vec![(5, 6, RuleCode::InvalidUrl)],
                "{bad}"
            );
        }
    }

    fn code_entry(map: &str) -> String {
        format!("---\nkb_format: 1\ntype: code\ntitle: T\ncode:\n{map}---\n\nNotes.\n")
    }

    #[test]
    fn code_rules() {
        let e = clean(&code_entry(
            "  repo: org/svc\n  path: src/a.rs\n  line: 10\n  ref: 1234567\n  end_line: 12\n  symbol: f\n",
        ));
        assert_eq!(
            e.fields(),
            &TypeFields::Code {
                code: Some(CodeRef {
                    repo: "org/svc".into(),
                    path: "src/a.rs".into(),
                    line: 10,
                    git_ref: "1234567".into(),
                    symbol: Some("f".into()),
                    end_line: Some(12),
                    branch: None,
                })
            }
        );
        assert_eq!(e.notes(), "\nNotes.\n");
        clean(&code_entry(
            "  repo: git@github.com:org/svc.git\n  path: a\n  line: 1\n  ref: main\n",
        ));
        assert_eq!(
            codes("---\nkb_format: 1\ntype: code\ntitle: T\n---\n"),
            vec![(1, 1, RuleCode::MissingCodeRef)]
        );
        assert_eq!(
            codes(&code_entry(
                "  path: /abs\n  line: 0\n  ref: main\n  end_line: 0\n"
            )),
            vec![
                (5, 1, RuleCode::InvalidCodeRef),
                (6, 9, RuleCode::InvalidCodeRef),
                (7, 9, RuleCode::InvalidCodeRef),
                (9, 13, RuleCode::InvalidCodeRef),
            ]
        );
        assert_eq!(
            codes(&code_entry(
                "  repo: not a repo\n  path: a/../b\n  line: 5\n  ref: main\n  end_line: 4\n"
            )),
            vec![
                (6, 9, RuleCode::InvalidCodeRef),
                (7, 9, RuleCode::InvalidCodeRef),
                (10, 13, RuleCode::InvalidCodeRef)
            ]
        );
        assert_eq!(
            codes("---\nkb_format: 1\ntype: code\ntitle: T\ncode: x\n---\n"),
            vec![(5, 7, RuleCode::WrongValueType)]
        );
    }

    #[test]
    fn payload_rules() {
        let f = one("---\nkb_format: 1\ntype: sql\ntitle: T\n---\n\nJust notes.\n");
        assert_eq!((f.line, f.column, f.code), (6, 1, RuleCode::MissingPayload));
        let f = one("---\nkb_format: 1\ntype: prompt\ntitle: T\n---\n\n```\n  \n```\n");
        assert_eq!((f.line, f.column, f.code), (7, 1, RuleCode::EmptyPayload));
        let f = one("---\nkb_format: 1\ntype: sql\ntitle: T\n---");
        assert_eq!((f.line, f.code), (5, RuleCode::MissingPayload));
    }

    #[test]
    fn crlf_and_bom_entries_parse() {
        let text = "\u{feff}---\r\nkb_format: 1\r\ntype: sql\r\ntitle: T\r\n---\r\n\r\n```\r\nselect 1;\r\n```\r\n";
        let e = clean(text);
        // Payload text always uses LF, so a command never carries a stray `\r`.
        assert_eq!(e.payload().unwrap().text, "select 1;\n");
    }

    #[test]
    fn unknown_type_still_gives_entry() {
        let out = parse_entry(
            "x",
            "x.md",
            b"---\nkb_format: 1\ntype: nope\ntitle: T\n---\n".to_vec(),
        );
        let e = out.entry.unwrap();
        assert_eq!(e.entry_type(), None);
        assert_eq!(e.fields(), &TypeFields::Unknown);
    }
    fn note(front: &str, body: &str) -> String {
        format!("---\nkb_format: 1\ntype: note\n{front}---\n{body}")
    }

    #[test]
    fn note_title_comes_from_frontmatter_or_the_first_heading() {
        let e = clean(&note("title: Explicit\n", "\n# Heading\n\ntext\n"));
        assert_eq!(e.title(), Some("Explicit"));
        assert_eq!(e.entry_type(), Some(EntryType::Note));
        assert_eq!(e.fields(), &TypeFields::Note);
        let e = clean(&note(
            "",
            "\n\n# Role `cache` **TTL** ~~old~~ [docs](http://x)\n\nbody\n",
        ));
        assert_eq!(e.title(), Some("Role cache TTL old docs"));
        assert!(e.payload().is_none());
        assert!(e.notes().contains("body"));
    }

    #[test]
    fn note_without_a_title_or_level_one_heading_is_an_error() {
        for body in [
            "\ntext only\n",
            "\n## Level two\n",
            "\ntext\n\n# late heading\n",
            "",
        ] {
            let found = codes(&note("", body));
            assert!(
                found.iter().any(|f| f.2 == RuleCode::NoteTitle),
                "{body:?}: {found:?}"
            );
        }
    }

    #[test]
    fn note_vars_are_an_error_and_braces_are_plain_text() {
        let text = note("title: T\nvars:\n  a: { kind: text }\n", "\ntext {{a}}\n");
        assert_eq!(one(&text).code, RuleCode::NoteVars);
        let e = clean(&note("title: T\n", "\ntext {{a}}\n"));
        assert!(e.vars().is_empty());
    }

    #[test]
    fn note_size_and_emptiness_findings() {
        let big = note(
            "title: T\n",
            &format!("\n{}\n", "x".repeat(NOTE_OVERSIZED_BYTES)),
        );
        assert_eq!(one(&big).code, RuleCode::NoteOversized);
        let at_limit = note("title: T\n", &"x".repeat(NOTE_OVERSIZED_BYTES));
        assert!(codes(&at_limit).is_empty());
        let empty = note("title: T\n", "\n  \n");
        assert_eq!(one(&empty).code, RuleCode::NoteEmpty);
    }
}
