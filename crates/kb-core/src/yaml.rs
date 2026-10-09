//! A YAML tree with byte spans, built from `saphyr-parser` events.
//!
//! kb accepts a strict subset of YAML 1.2: no anchors, aliases, explicit tags or duplicate keys,
//! so every value lives at exactly one place in the file.

use saphyr_parser::{Event, Parser, ScanError};

use crate::lint::RuleCode;
use crate::source::{CharMap, Span};

/// Collections nested deeper than this are rejected, which keeps recursion bounded.
const MAX_DEPTH: usize = 64;

/// How a scalar was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarStyle {
    Plain,
    Single,
    Double,
    Literal,
    Folded,
}

/// A mapping key and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub name: String,
    pub span: Span,
}

/// A YAML node. Spans are absolute byte offsets into the entry text.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Scalar {
        value: String,
        style: ScalarStyle,
        span: Span,
    },
    Seq {
        items: Vec<Node>,
        flow: bool,
        span: Span,
    },
    Map {
        entries: Vec<(Key, Node)>,
        flow: bool,
        span: Span,
    },
}

/// A problem found while building the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    pub code: RuleCode,
    /// Absolute byte offset.
    pub offset: usize,
    pub message: String,
}

/// The result of parsing a frontmatter block.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    /// The root mapping, when the YAML is valid and is a mapping.
    pub root: Option<Node>,
    pub errors: Vec<YamlError>,
}

/// Parses `text`, which starts at byte `base` of the entry text.
///
/// An empty document (or one with only comments) is an empty mapping.
pub fn parse(text: &str, base: usize) -> Parsed {
    let mut b = Builder {
        parser: Parser::new_from_str(text),
        text,
        map: CharMap::new(text),
        base,
        errors: Vec::new(),
    };
    match b.document() {
        Ok(Some(node @ Node::Map { .. })) => Parsed {
            root: Some(node),
            errors: b.errors,
        },
        Ok(Some(node)) if node.is_null() => Parsed {
            root: Some(Node::Map {
                entries: Vec::new(),
                flow: false,
                span: node.span().clone(),
            }),
            errors: b.errors,
        },
        Ok(None) => Parsed {
            root: Some(Node::Map {
                entries: Vec::new(),
                flow: false,
                span: base..base,
            }),
            errors: b.errors,
        },
        Ok(Some(node)) => {
            let error = YamlError {
                code: RuleCode::FrontmatterNotMapping,
                offset: node.span().start,
                message: format!(
                    "the frontmatter is a {}, not a set of `key: value` lines",
                    node.kind_name()
                ),
            };
            Parsed {
                root: None,
                errors: vec![error],
            }
        }
        Err(error) => Parsed {
            root: None,
            errors: vec![error],
        },
    }
}

struct Builder<'a> {
    parser: Parser<'a, saphyr_parser::StrInput<'a>>,
    text: &'a str,
    map: CharMap,
    base: usize,
    errors: Vec<YamlError>,
}

type Ev<'a> = (Event<'a>, saphyr_parser::Span);

impl<'a> Builder<'a> {
    fn byte(&self, char_idx: usize) -> usize {
        self.base + self.map.byte(char_idx)
    }

    fn span(&self, span: &saphyr_parser::Span) -> Span {
        self.byte(span.start.index())..self.byte(span.end.index())
    }

    fn syntax(&self, offset: usize, message: impl Into<String>) -> YamlError {
        YamlError {
            code: RuleCode::YamlSyntax,
            offset,
            message: message.into(),
        }
    }

    fn scan_error(&self, e: &ScanError) -> YamlError {
        self.syntax(self.byte(e.marker().index()), e.info().to_string())
    }

    fn next(&mut self) -> Result<Ev<'a>, YamlError> {
        match self.parser.next_event() {
            Some(Ok(ev)) => Ok(ev),
            Some(Err(e)) => Err(self.scan_error(&e)),
            None => Err(self.syntax(self.base + self.text.len(), "unexpected end of YAML")),
        }
    }

    /// Where an anchor (`&`) or tag (`!`) written before the node at `node_start` begins, so the
    /// finding points at it rather than at the value.
    fn property_offset(&self, node_start: usize, marker: char) -> usize {
        let rel = node_start - self.base;
        let line_start = self.text[..rel].rfind('\n').map_or(0, |i| i + 1);
        self.text[line_start..rel]
            .rfind(marker)
            .map_or(node_start, |i| self.base + line_start + i)
    }

    fn unsupported(&mut self, offset: usize, what: &str) {
        self.errors.push(YamlError {
            code: RuleCode::UnsupportedYaml,
            offset,
            message: format!("{what} are not supported in kb entries"),
        });
    }

    /// Reads the single document in the stream. `None` means the stream is empty.
    fn document(&mut self) -> Result<Option<Node>, YamlError> {
        let mut root = None;
        loop {
            let (ev, span) = self.next()?;
            match ev {
                Event::StreamStart | Event::Nothing => {}
                Event::StreamEnd => return Ok(root),
                Event::DocumentStart(_) if root.is_some() => {
                    let at = self.span(&span).start;
                    return Err(
                        self.syntax(at, "only one YAML document is allowed in the frontmatter")
                    );
                }
                Event::DocumentStart(_) => {
                    let (ev, span) = self.next()?;
                    root = Some(self.node(ev, span, 0)?);
                }
                Event::DocumentEnd => {}
                _ => {
                    let at = self.span(&span).start;
                    return Err(self.syntax(at, "unexpected YAML structure"));
                }
            }
        }
    }

    fn node(
        &mut self,
        ev: Event<'a>,
        span: saphyr_parser::Span,
        depth: usize,
    ) -> Result<Node, YamlError> {
        let sp = self.span(&span);
        if depth > MAX_DEPTH {
            return Err(self.syntax(sp.start, "values are nested too deeply"));
        }
        match ev {
            Event::Scalar(value, style, anchor, tag) => {
                if anchor != 0 {
                    self.unsupported(self.property_offset(sp.start, '&'), "anchors (`&name`)");
                }
                if tag.is_some() {
                    self.unsupported(
                        self.property_offset(sp.start, '!'),
                        "explicit tags (`!tag`)",
                    );
                }
                let style = match style {
                    saphyr_parser::ScalarStyle::Plain => ScalarStyle::Plain,
                    saphyr_parser::ScalarStyle::SingleQuoted => ScalarStyle::Single,
                    saphyr_parser::ScalarStyle::DoubleQuoted => ScalarStyle::Double,
                    saphyr_parser::ScalarStyle::Literal => ScalarStyle::Literal,
                    saphyr_parser::ScalarStyle::Folded => ScalarStyle::Folded,
                };
                let mut sp = sp;
                if let Some(end) = self.closing_quote(sp.start, style) {
                    // The parser's end marker can run past the closing quote, over trailing
                    // spaces and a comment; the value ends at the quote.
                    sp.end = end;
                }
                Ok(Node::Scalar {
                    value: value.into_owned(),
                    style,
                    span: sp,
                })
            }
            Event::Alias(_) => {
                self.unsupported(sp.start, "aliases (`*name`)");
                Ok(Node::Scalar {
                    value: String::new(),
                    style: ScalarStyle::Plain,
                    span: sp,
                })
            }
            Event::SequenceStart(anchor, tag) => {
                self.collection_props(sp.start, anchor, tag.is_some());
                let flow = self.text.as_bytes().get(sp.start - self.base) == Some(&b'[');
                let mut items = Vec::new();
                loop {
                    let (ev, span) = self.next()?;
                    if let Event::SequenceEnd = ev {
                        let end = self.end_of(flow, &span, items.last(), sp.start);
                        return Ok(Node::Seq {
                            items,
                            flow,
                            span: sp.start..end,
                        });
                    }
                    items.push(self.node(ev, span, depth + 1)?);
                }
            }
            Event::MappingStart(anchor, tag) => {
                self.collection_props(sp.start, anchor, tag.is_some());
                let flow = self.text.as_bytes().get(sp.start - self.base) == Some(&b'{');
                let mut entries: Vec<(Key, Node)> = Vec::new();
                loop {
                    let (ev, span) = self.next()?;
                    if let Event::MappingEnd = ev {
                        let end =
                            self.end_of(flow, &span, entries.last().map(|(_, v)| v), sp.start);
                        return Ok(Node::Map {
                            entries,
                            flow,
                            span: sp.start..end,
                        });
                    }
                    let key_node = self.node(ev, span, depth + 1)?;
                    let (ev, span) = self.next()?;
                    let value = self.node(ev, span, depth + 1)?;
                    let Node::Scalar {
                        value: name,
                        span: key_span,
                        ..
                    } = key_node
                    else {
                        self.unsupported(
                            key_node.span().start,
                            "complex keys (lists or maps used as keys)",
                        );
                        continue;
                    };
                    if entries.iter().any(|(k, _)| k.name == name) {
                        self.errors.push(YamlError {
                            code: RuleCode::DuplicateKey,
                            offset: key_span.start,
                            message: format!("key `{name}` appears more than once"),
                        });
                        continue;
                    }
                    entries.push((
                        Key {
                            name,
                            span: key_span,
                        },
                        value,
                    ));
                }
            }
            _ => Err(self.syntax(sp.start, "unexpected YAML structure")),
        }
    }

    /// Absolute offset just after the closing quote of a quoted scalar starting at `start`.
    fn closing_quote(&self, start: usize, style: ScalarStyle) -> Option<usize> {
        let rel = start - self.base;
        let bytes = self.text.as_bytes();
        let quote = match style {
            ScalarStyle::Single => b'\'',
            ScalarStyle::Double => b'"',
            _ => return None,
        };
        if bytes.get(rel) != Some(&quote) {
            return None;
        }
        let mut i = rel + 1;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' if quote == b'"' => i += 2,
                b'\'' if quote == b'\'' && bytes.get(i + 1) == Some(&b'\'') => i += 2,
                b if b == quote => return Some(self.base + i + 1),
                _ => i += 1,
            }
        }
        None
    }

    fn collection_props(&mut self, at: usize, anchor: usize, tagged: bool) {
        if anchor != 0 {
            self.unsupported(self.property_offset(at, '&'), "anchors (`&name`)");
        }
        if tagged {
            self.unsupported(self.property_offset(at, '!'), "explicit tags (`!tag`)");
        }
    }

    /// End of a collection: the closing bracket for flow style, else the end of its last child.
    fn end_of(
        &self,
        flow: bool,
        end_ev: &saphyr_parser::Span,
        last: Option<&Node>,
        start: usize,
    ) -> usize {
        if flow {
            self.span(end_ev).end.max(start)
        } else {
            last.map_or(start, |n| n.span().end)
        }
    }
}

impl Node {
    /// Where the node is in the entry text.
    pub fn span(&self) -> &Span {
        match self {
            Node::Scalar { span, .. } | Node::Seq { span, .. } | Node::Map { span, .. } => span,
        }
    }

    /// Looks up a key in a mapping.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.entry(key).map(|(_, v)| v)
    }

    /// Looks up a key in a mapping, returning the key too.
    pub fn entry(&self, key: &str) -> Option<(&Key, &Node)> {
        match self {
            Node::Map { entries, .. } => entries
                .iter()
                .find(|(k, _)| k.name == key)
                .map(|(k, v)| (k, v)),
            _ => None,
        }
    }

    /// A mapping's entries; empty for other nodes.
    pub fn entries(&self) -> &[(Key, Node)] {
        match self {
            Node::Map { entries, .. } => entries,
            _ => &[],
        }
    }

    /// The scalar's text, whatever its type.
    pub fn scalar(&self) -> Option<&str> {
        match self {
            Node::Scalar { value, .. } => Some(value),
            _ => None,
        }
    }

    /// The value when this is a string: quoted, block, or plain and not a null, bool or number.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::Scalar {
                value,
                style: ScalarStyle::Plain,
                ..
            } => {
                if is_null(value) || is_bool(value) || is_int(value) || is_float(value) {
                    None
                } else {
                    Some(value)
                }
            }
            Node::Scalar { value, .. } => Some(value),
            _ => None,
        }
    }

    /// The value when this is a plain integer.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Node::Scalar {
                value,
                style: ScalarStyle::Plain,
                ..
            } if is_int(value) => value.trim_start_matches('+').parse().ok(),
            _ => None,
        }
    }

    /// True for a plain integer or float.
    pub fn is_number(&self) -> bool {
        matches!(self, Node::Scalar { value, style: ScalarStyle::Plain, .. } if is_int(value) || is_float(value))
    }

    /// True for a plain `null`, `~` or empty value.
    pub fn is_null(&self) -> bool {
        matches!(self, Node::Scalar { value, style: ScalarStyle::Plain, .. } if is_null(value))
    }

    /// A short description of the node's shape, for messages.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Node::Scalar {
                value,
                style: ScalarStyle::Plain,
                ..
            } if is_null(value) => "null",
            Node::Scalar {
                value,
                style: ScalarStyle::Plain,
                ..
            } if is_bool(value) => "boolean",
            Node::Scalar { .. } if self.is_number() => "number",
            Node::Scalar { .. } => "string",
            Node::Seq { .. } => "list",
            Node::Map { .. } => "mapping",
        }
    }
}

fn is_null(v: &str) -> bool {
    matches!(v, "" | "~" | "null" | "Null" | "NULL")
}

fn is_bool(v: &str) -> bool {
    matches!(v, "true" | "True" | "TRUE" | "false" | "False" | "FALSE")
}

fn is_int(v: &str) -> bool {
    let d = v.strip_prefix(['+', '-']).unwrap_or(v);
    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
}

fn is_float(v: &str) -> bool {
    let d = v.strip_prefix(['+', '-']).unwrap_or(v);
    if matches!(d, ".inf" | ".Inf" | ".INF") || matches!(v, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    let (mantissa, exp) = match d.find(['e', 'E']) {
        Some(i) => (&d[..i], Some(&d[i + 1..])),
        None => (d, None),
    };
    let (int, frac) = match mantissa.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (mantissa, None),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match frac {
        Some(f) => digits(int) && digits(f) && !(int.is_empty() && f.is_empty()),
        None => !int.is_empty() && digits(int),
    };
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exp_ok && (frac.is_some() || exp.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(text: &str) -> Node {
        let p = parse(text, 0);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        p.root.unwrap()
    }

    fn slice<'a>(text: &'a str, node: &Node) -> &'a str {
        &text[node.span().clone()]
    }

    #[test]
    fn scalar_spans_match_source() {
        let text = "a: plain\nb: 'single'\nc: \"double\"\nd: 147\n";
        let r = root(text);
        assert_eq!(slice(text, r.get("a").unwrap()), "plain");
        assert_eq!(slice(text, r.get("b").unwrap()), "'single'");
        assert_eq!(slice(text, r.get("c").unwrap()), "\"double\"");
        assert_eq!(slice(text, r.get("d").unwrap()), "147");
        assert_eq!(r.get("b").unwrap().as_str(), Some("single"));
        assert_eq!(r.get("d").unwrap().as_int(), Some(147));
        let (k, _) = r.entry("c").unwrap();
        assert_eq!(&text[k.span.clone()], "c");
    }

    #[test]
    fn quoted_spans_stop_at_the_closing_quote() {
        let text = "a: \"H \\\" i\"   # c\nb: 'it''s'  # c\nc: { d: \"q\" }  # c\n";
        let r = root(text);
        assert_eq!(slice(text, r.get("a").unwrap()), "\"H \\\" i\"");
        assert_eq!(r.get("a").unwrap().as_str(), Some("H \" i"));
        assert_eq!(slice(text, r.get("b").unwrap()), "'it''s'");
        assert_eq!(slice(text, r.get("c").unwrap().get("d").unwrap()), "\"q\"");
    }

    #[test]
    fn spans_after_non_ascii_are_bytes() {
        let text = "title: \"héllo wörld\"\nnext: x\n";
        let r = root(text);
        assert_eq!(slice(text, r.get("title").unwrap()), "\"héllo wörld\"");
        assert_eq!(slice(text, r.get("next").unwrap()), "x");
    }

    #[test]
    fn base_offset_is_added() {
        let full = "---\nk: v\n---\n";
        let p = parse(&full[4..9], 4);
        let r = p.root.unwrap();
        assert_eq!(&full[r.get("k").unwrap().span().clone()], "v");
    }

    #[test]
    fn block_scalars() {
        let text = "lit: |\n  one\n  two\nfold: >\n  a\n  b\nend: 1\n";
        let r = root(text);
        assert_eq!(r.get("lit").unwrap().as_str(), Some("one\ntwo\n"));
        assert_eq!(r.get("fold").unwrap().as_str(), Some("a b\n"));
        assert!(matches!(
            r.get("lit").unwrap(),
            Node::Scalar {
                style: ScalarStyle::Literal,
                ..
            }
        ));
        assert_eq!(slice(text, r.get("end").unwrap()), "1");
    }

    #[test]
    fn flow_and_nested_collections() {
        let text = "tags: [a, 'b']\nvars:\n  env: { kind: enum, options: [prod, dev] }\ncode:\n  line: 147\n";
        let r = root(text);
        let tags = r.get("tags").unwrap();
        assert!(matches!(tags, Node::Seq { flow: true, items, .. } if items.len() == 2));
        let env = r.get("vars").unwrap().get("env").unwrap();
        assert!(matches!(env, Node::Map { flow: true, .. }));
        assert_eq!(slice(text, env.get("kind").unwrap()), "enum");
        let line = r.get("code").unwrap().get("line").unwrap();
        assert_eq!(slice(text, line), "147");
        assert!(matches!(
            r.get("code").unwrap(),
            Node::Map { flow: false, .. }
        ));
    }

    #[test]
    fn crlf_input() {
        let text = "a: 1\r\nb: two\r\n";
        let r = root(text);
        assert_eq!(slice(text, r.get("b").unwrap()), "two");
    }

    #[test]
    fn empty_and_comment_only_are_empty_maps() {
        assert!(root("").entries().is_empty());
        assert!(root("# just a comment\n").entries().is_empty());
    }

    #[test]
    fn yaml_1_2_scalars() {
        let r = root("a: no\nb: on\nc: 1.5\nd: ~\ne: true\n");
        assert_eq!(r.get("a").unwrap().as_str(), Some("no"));
        assert_eq!(r.get("b").unwrap().as_str(), Some("on"));
        assert!(r.get("c").unwrap().is_number());
        assert!(r.get("d").unwrap().is_null());
        assert_eq!(r.get("e").unwrap().kind_name(), "boolean");
    }

    #[test]
    fn anchor_error_points_at_the_anchor() {
        let p = parse("title: &t T\n", 0);
        assert_eq!(p.errors[0].offset, 7);
        let p = parse("a: !x 1\n", 0);
        assert_eq!(p.errors[0].offset, 3);
    }

    #[test]
    fn anchors_aliases_and_tags_are_rejected() {
        for text in ["a: &x 1\nb: *x\n", "a: !custom 1\n", "a: &x [1]\n"] {
            let p = parse(text, 0);
            assert!(
                p.errors.iter().any(|e| e.code == RuleCode::UnsupportedYaml),
                "{text:?}: {:?}",
                p.errors
            );
        }
    }

    #[test]
    fn duplicate_key_reported_at_second() {
        let text = "a: 1\nb: 2\na: 3\n";
        let p = parse(text, 0);
        assert_eq!(p.errors.len(), 1);
        assert_eq!(p.errors[0].code, RuleCode::DuplicateKey);
        assert_eq!(p.errors[0].offset, 10);
        assert_eq!(p.root.unwrap().get("a").unwrap().scalar(), Some("1"));
    }

    #[test]
    fn root_must_be_mapping() {
        for text in ["- a\n- b\n", "just text\n"] {
            let p = parse(text, 0);
            assert!(p.root.is_none());
            assert_eq!(p.errors[0].code, RuleCode::FrontmatterNotMapping);
        }
    }

    #[test]
    fn syntax_error_has_position() {
        let text = "a: 1\nb: [unclosed\n";
        let p = parse(text, 0);
        assert!(p.root.is_none());
        assert_eq!(p.errors[0].code, RuleCode::YamlSyntax);
        assert!(p.errors[0].offset >= 5, "{:?}", p.errors[0]);
        assert!(!p.errors[0].message.is_empty());
    }
}
