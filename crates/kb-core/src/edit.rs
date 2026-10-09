//! Editing entries without disturbing anything else in the file (FR-017, FR-018).

use std::fmt;
use std::path::Path;

use crate::entry::{Entry, MAX_KB_FORMAT, parse_source};
use crate::lint::{Finding, RuleCode};
use crate::source::Source;
use crate::yaml::{Node, ScalarStyle};

/// A new value for a frontmatter field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    Str(String),
    Int(i64),
}

/// Why an edit was refused. The entry is unchanged in every case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The entry's `kb_format` is missing, invalid or newer than this kb supports.
    UnsupportedFormat,
    /// The edit can't be made without rewriting more than the value (for example a flow map).
    Unsupported(String),
    /// The edited file would not parse.
    Invalid(String),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::UnsupportedFormat => write!(
                f,
                "this entry's kb_format is not supported by this version of kb"
            ),
            EditError::Unsupported(why) | EditError::Invalid(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for EditError {}

/// Why a write was refused.
#[derive(Debug)]
pub enum WriteError {
    UnsupportedFormat,
    /// The entry contains likely secrets (constitution VI).
    Secret(Vec<Finding>),
    Io(std::io::Error),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::UnsupportedFormat => write!(
                f,
                "refusing to write an entry whose kb_format is not supported"
            ),
            WriteError::Secret(found) => write!(
                f,
                "refusing to write an entry with {} likely secret(s)",
                found.len()
            ),
            WriteError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl Entry {
    /// The exact bytes of the file. Unmodified entries return their input (FR-016).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.source.to_bytes()
    }

    fn format_supported(&self) -> bool {
        self.kb_format.is_some_and(|v| v <= MAX_KB_FORMAT)
    }

    /// Sets a frontmatter field, changing only that value's bytes. A missing key is inserted:
    /// top-level keys just before the closing `---`, nested keys after their last sibling.
    pub fn set_field(&mut self, path: &[&str], value: FieldValue) -> Result<(), EditError> {
        if !self.format_supported() {
            return Err(EditError::UnsupportedFormat);
        }
        let Some((&key, parents)) = path.split_last() else {
            return Err(EditError::Unsupported("empty field path".into()));
        };
        let mut parent = &self.tree;
        for segment in parents {
            parent = match parent.get(segment) {
                Some(node @ Node::Map { .. }) => node,
                _ => {
                    return Err(EditError::Unsupported(format!(
                        "`{}` is not a mapping in this entry",
                        path.join(".")
                    )));
                }
            };
        }
        if !matches!(parent, Node::Map { .. }) {
            return Err(EditError::Unsupported(format!(
                "`{}` is not a mapping in this entry",
                parents.join(".")
            )));
        }
        let text = self.source.text();
        let (range, replacement) = match parent.get(key) {
            Some(Node::Scalar { style, span, .. }) => (span.clone(), render(&value, Some(*style))?),
            Some(_) => {
                return Err(EditError::Unsupported(format!(
                    "`{}` is a list or mapping",
                    path.join(".")
                )));
            }
            None => {
                let eol = self.source.line_ending().as_str();
                let rendered = render(&value, None)?;
                if parents.is_empty() {
                    let at = self.split.close_line;
                    (at..at, format!("{key}: {rendered}{eol}"))
                } else {
                    let Node::Map {
                        entries,
                        flow: false,
                        ..
                    } = parent
                    else {
                        return Err(EditError::Unsupported(
                            "cannot add a key to a flow mapping (`{ … }`)".into(),
                        ));
                    };
                    let (first_key, _) = entries.first().expect("block mappings are never empty");
                    let line_start = text[..first_key.span.start]
                        .rfind('\n')
                        .map_or(0, |i| i + 1);
                    let indent = &text[line_start..first_key.span.start];
                    let last_end = entries
                        .last()
                        .map_or(first_key.span.end, |(_, v)| v.span().end);
                    let line_end = text[last_end..]
                        .find('\n')
                        .map_or(text.len(), |i| last_end + i + 1);
                    (
                        line_end..line_end,
                        format!("{indent}{key}: {rendered}{eol}"),
                    )
                }
            }
        };
        let mut new_text = String::with_capacity(text.len() + replacement.len());
        new_text.push_str(&text[..range.start]);
        new_text.push_str(&replacement);
        new_text.push_str(&text[range.end..]);
        let outcome = parse_source(
            &self.id,
            &self.path,
            Source::from_text(new_text, self.source.bom()),
        );
        let structural = [
            RuleCode::YamlSyntax,
            RuleCode::FrontmatterNotMapping,
            RuleCode::UnsupportedYaml,
            RuleCode::DuplicateKey,
        ];
        if let Some(f) = outcome
            .findings
            .iter()
            .find(|f| structural.contains(&f.code))
        {
            return Err(EditError::Invalid(f.message.clone()));
        }
        match outcome.entry {
            Some(entry) => {
                *self = entry;
                Ok(())
            }
            None => Err(EditError::Invalid(
                "the edited entry would not parse".into(),
            )),
        }
    }
}

/// Writes an entry atomically (temporary file, then rename). Refuses unsupported formats and
/// content with unsuppressed secrets.
pub fn write_entry(path: &Path, entry: &Entry) -> Result<(), WriteError> {
    if !entry.format_supported() {
        return Err(WriteError::UnsupportedFormat);
    }
    let scan = crate::lint::secrets::scan_secrets(entry.source().text());
    if !scan.hits.is_empty() {
        let found = scan
            .hits
            .into_iter()
            .map(|hit| {
                let (line, column) = entry.source().position(hit.offset);
                Finding {
                    path: entry.path().to_string(),
                    id: Some(entry.id().to_string()),
                    line,
                    column,
                    severity: hit.code.severity(),
                    code: hit.code,
                    message: hit.message,
                    fix: Some(hit.fix),
                }
            })
            .collect();
        return Err(WriteError::Secret(found));
    }
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.kb-tmp-{}", std::process::id()));
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&entry.to_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(WriteError::Io)
}

/// Writes a value for YAML, keeping `style` when the value can be written in it.
fn render(value: &FieldValue, style: Option<ScalarStyle>) -> Result<String, EditError> {
    let s = match value {
        FieldValue::Int(n) => return Ok(n.to_string()),
        FieldValue::Str(s) => s,
    };
    match style {
        Some(ScalarStyle::Literal | ScalarStyle::Folded) => Err(EditError::Unsupported(
            "block scalars (`|` and `>`) can't be edited in place".into(),
        )),
        Some(ScalarStyle::Single) if !s.contains(['\n', '\r']) => {
            Ok(format!("'{}'", s.replace('\'', "''")))
        }
        Some(ScalarStyle::Plain) | None if is_plain_safe(s) => Ok(s.clone()),
        _ => Ok(double_quoted(s)),
    }
}

pub(crate) fn double_quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// True when `s` reads back as the same string if written without quotes.
pub(crate) fn is_plain_safe(s: &str) -> bool {
    let Some(first) = s.chars().next() else {
        return false;
    };
    let reads_as_other = matches!(
        Node::Scalar {
            value: s.to_string(),
            style: ScalarStyle::Plain,
            span: 0..0
        }
        .kind_name(),
        "null" | "boolean" | "number"
    );
    !reads_as_other
        && !"-?:,[]{}#&*!|>'\"%@`~".contains(first)
        && s.trim() == s
        && !s.contains(": ")
        && !s.contains(" #")
        && !s.ends_with(':')
        && !s.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::parse_entry;

    fn entry(text: &str) -> Entry {
        let out = parse_entry("x", "x.md", text.as_bytes().to_vec());
        out.entry.unwrap()
    }

    fn text(e: &Entry) -> String {
        String::from_utf8(e.to_bytes()).unwrap()
    }

    const CODE: &str = "---\n# a comment\nkb_format: 1\ntype: code\ntitle: \"Handler\"   # trailing comment\ncode:\n  repo: org/svc\n  path: src/a.rs\n  line: 147\n  ref: a1b9c3e\nowner: me\n---\n\nNotes.\n";

    #[test]
    fn to_bytes_is_identity() {
        assert_eq!(text(&entry(CODE)), CODE);
    }

    #[test]
    fn changes_only_the_value_bytes() {
        let mut e = entry(CODE);
        e.set_field(&["code", "line"], FieldValue::Int(150))
            .unwrap();
        assert_eq!(text(&e), CODE.replace("line: 147", "line: 150"));
        e.set_field(&["code", "ref"], FieldValue::Str("deadbeef".into()))
            .unwrap();
        assert_eq!(
            text(&e),
            CODE.replace("line: 147", "line: 150")
                .replace("a1b9c3e", "deadbeef")
        );
        assert!(
            matches!(e.fields(), crate::entry::TypeFields::Code { code: Some(c) } if c.line == 150)
        );
    }

    #[test]
    fn keeps_quoting_style() {
        let mut e = entry(CODE);
        e.set_field(&["title"], FieldValue::Str("New \"title\"".into()))
            .unwrap();
        assert!(
            text(&e).contains("title: \"New \\\"title\\\"\"   # trailing comment\n"),
            "{}",
            text(&e)
        );
        let mut e = entry(&CODE.replace("\"Handler\"", "'Handler'"));
        e.set_field(&["title"], FieldValue::Str("it's".into()))
            .unwrap();
        assert!(text(&e).contains("title: 'it''s'   #"), "{}", text(&e));
        assert_eq!(e.title(), Some("it's"));
        let mut e = entry(CODE);
        e.set_field(&["code", "path"], FieldValue::Str("src/b: c.rs".into()))
            .unwrap();
        assert!(
            text(&e).contains("  path: \"src/b: c.rs\"\n"),
            "{}",
            text(&e)
        );
    }

    #[test]
    fn inserts_missing_top_level_key() {
        let mut e = entry(CODE);
        e.set_field(&["updated"], FieldValue::Str("2026-10-09".into()))
            .unwrap();
        assert_eq!(
            text(&e),
            CODE.replace("owner: me\n---", "owner: me\nupdated: 2026-10-09\n---")
        );
        assert_eq!(e.updated(), Some("2026-10-09"));
    }

    #[test]
    fn inserts_with_crlf_in_crlf_files() {
        let crlf = CODE.replace('\n', "\r\n");
        let mut e = entry(&crlf);
        e.set_field(&["updated"], FieldValue::Str("2026-10-09".into()))
            .unwrap();
        assert_eq!(
            text(&e),
            crlf.replace(
                "owner: me\r\n---",
                "owner: me\r\nupdated: 2026-10-09\r\n---"
            )
        );
    }

    #[test]
    fn inserts_missing_nested_key_after_last_sibling() {
        let mut e = entry(CODE);
        e.set_field(&["code", "end_line"], FieldValue::Int(160))
            .unwrap();
        assert_eq!(
            text(&e),
            CODE.replace("  ref: a1b9c3e\n", "  ref: a1b9c3e\n  end_line: 160\n")
        );
    }

    #[test]
    fn flow_maps_are_unsupported_for_inserts() {
        let flow = "---\nkb_format: 1\ntype: code\ntitle: T\ncode: { repo: o/r, path: a, line: 1, ref: main }\n---\n";
        let mut e = entry(flow);
        assert!(matches!(
            e.set_field(&["code", "symbol"], FieldValue::Str("f".into())),
            Err(EditError::Unsupported(_))
        ));
        assert_eq!(text(&e), flow);
        e.set_field(&["code", "line"], FieldValue::Int(2)).unwrap();
        assert_eq!(text(&e), flow.replace("line: 1", "line: 2"));
    }

    #[test]
    fn invalid_results_are_rolled_back() {
        let mut e = entry(CODE);
        let lit = "---\nkb_format: 1\ntype: prompt\ntitle: |\n  multi\n---\n\n```\nx\n```\n";
        let mut b = entry(lit);
        assert!(matches!(
            b.set_field(&["title"], FieldValue::Str("y".into())),
            Err(EditError::Unsupported(_))
        ));
        assert_eq!(text(&b), lit);
        assert!(matches!(
            e.set_field(&["code", "line", "deeper"], FieldValue::Int(1)),
            Err(EditError::Unsupported(_))
        ));
        assert_eq!(text(&e), CODE);
    }

    #[test]
    fn unsupported_format_is_never_edited() {
        let newer = CODE.replace("kb_format: 1", "kb_format: 2");
        let mut e = entry(&newer);
        assert_eq!(
            e.set_field(&["code", "line"], FieldValue::Int(1)),
            Err(EditError::UnsupportedFormat)
        );
        assert_eq!(text(&e), newer);
    }

    #[test]
    fn unknown_keys_and_comments_survive() {
        let mut e = entry(CODE);
        e.set_field(&["title"], FieldValue::Str("Other".into()))
            .unwrap();
        let t = text(&e);
        assert!(
            t.contains("# a comment\n")
                && t.contains("# trailing comment")
                && t.contains("owner: me\n")
        );
        assert_eq!(e.unknown_keys().collect::<Vec<_>>(), vec!["owner"]);
    }

    #[test]
    fn write_entry_is_atomic_and_exact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.md");
        let mut e = entry(CODE);
        e.set_field(&["code", "line"], FieldValue::Int(9)).unwrap();
        write_entry(&path, &e).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), e.to_bytes());
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|d| d.unwrap().file_name())
            .collect();
        assert_eq!(names, vec!["x.md"], "no temporary file left behind");
    }

    #[test]
    fn write_entry_refuses_unsupported_formats() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.md");
        std::fs::write(&path, "original").unwrap();
        for text in [
            CODE.replace("kb_format: 1", "kb_format: 2"),
            CODE.replace("kb_format: 1", "kb_format: x"),
        ] {
            let e = entry(&text);
            assert!(matches!(
                write_entry(&path, &e),
                Err(WriteError::UnsupportedFormat)
            ));
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    }

    #[test]
    fn write_entry_refuses_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.md");
        std::fs::write(&path, "original").unwrap();
        let leaky = "---\nkb_format: 1\ntype: cmd\ntitle: T\n---\n\n```\naws s3 ls --key AKIAIOSFODNN7EXAMPLE\n```\n";
        match write_entry(&path, &entry(leaky)) {
            Err(WriteError::Secret(found)) => {
                assert_eq!(found[0].code, crate::lint::RuleCode::SecretAwsAccessKey)
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        let allowed = leaky.replace("EXAMPLE\n", "EXAMPLE # kb:allow-secret\n");
        write_entry(&path, &entry(&allowed)).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), allowed);
    }
}
