//! Turning a file into what the index stores (FR-005), or deciding it must stay out.

use crate::entry::{Entry, EntryType, TypeFields};
use crate::fold::fold;
use crate::lint::{Severity, check_file};
use crate::usage::content_hash;
use crate::walk::WalkEntry;

/// What the index stores for one valid entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    pub entry_type: &'static str,
    pub title: String,
    pub title_fold: String,
    pub tags: Vec<String>,
    pub vars: Vec<String>,
    pub code_repo: Option<String>,
    pub updated: Option<String>,
    /// The entries a note links to with `[[id]]`, without repeats.
    pub links: Vec<String>,
    /// Runbook steps: (step_index, step_id, entry_id) for runbook entries.
    pub runbook_steps: Vec<(usize, String, Option<String>)>,
    /// FTS columns: title, tags, id, meta, payload, notes.
    pub cols: [String; 6],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Indexed(Box<Doc>),
    /// The file has lint errors (or collides with another ID) and stays out of the index.
    Skipped,
}

/// The result of reading one file.
#[derive(Debug, Clone)]
pub struct Built {
    pub verdict: Verdict,
    pub hash: u64,
}

/// Checks `bytes` like `kb lint` does and builds the index document when there are no errors.
pub fn build_doc(file: &WalkEntry, bytes: Vec<u8>) -> Built {
    let hash = content_hash(&bytes);
    let check = check_file(file, bytes);
    let has_error = check.findings.iter().any(|f| f.severity == Severity::Error);
    let verdict = match check.entry {
        Some(entry) if !has_error => match doc_of(&entry) {
            Some(doc) => Verdict::Indexed(Box::new(doc)),
            None => Verdict::Skipped,
        },
        _ => Verdict::Skipped,
    };
    Built { verdict, hash }
}

/// Most of a note's body the index keeps, in bytes.
pub const NOTE_INDEX_BYTES: usize = 262_144;

/// What a note puts in the search columns: headings 1 to 3 and link hosts go to `meta`; the rest
/// of the text, with the Markdown syntax removed, goes to `payload`.
struct NoteColumns {
    meta: Vec<String>,
    payload: String,
}

fn note_columns(body: &str) -> NoteColumns {
    use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;
    let mut meta = Vec::new();
    let mut payload = String::new();
    let mut heading: Option<String> = None;
    let break_ = |payload: &mut String, blank: bool| {
        let want = if blank { "\n\n" } else { "\n" };
        let have = payload.len() - payload.trim_end_matches('\n').len();
        if !payload.is_empty() && have < want.len() {
            payload.push_str(&want[have..]);
        }
    };
    for event in Parser::new_ext(body, options) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading = (level <= HeadingLevel::H3).then(String::new);
                if heading.is_none() {
                    break_(&mut payload, true);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                match heading.take() {
                    Some(h) if !h.trim().is_empty() => meta.push(h),
                    _ => {}
                }
                break_(&mut payload, true);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let url = dest_url.split_once("://").map_or(&*dest_url, |(_, r)| r);
                if !url.starts_with('#') {
                    meta.push(url.to_string());
                }
            }
            Event::Start(Tag::CodeBlock(_)) | Event::End(TagEnd::CodeBlock) => {
                break_(&mut payload, true);
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(h) = heading.as_mut() {
                    h.push_str(&t);
                } else {
                    payload.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(h) = heading.as_mut() {
                    h.push(' ');
                } else {
                    break_(&mut payload, false);
                }
            }
            Event::End(TagEnd::Paragraph | TagEnd::BlockQuote(_) | TagEnd::Table) => {
                break_(&mut payload, true);
            }
            Event::End(TagEnd::Item | TagEnd::TableRow | TagEnd::TableHead) => {
                break_(&mut payload, false);
            }
            Event::End(TagEnd::TableCell) => payload.push(' '),
            _ => {}
        }
        if payload.len() >= NOTE_INDEX_BYTES {
            let mut cut = NOTE_INDEX_BYTES;
            while !payload.is_char_boundary(cut) {
                cut -= 1;
            }
            payload.truncate(cut);
            break;
        }
    }
    NoteColumns {
        meta,
        payload: payload.trim().to_string(),
    }
}

fn doc_of(entry: &Entry) -> Option<Doc> {
    let ty = entry.entry_type()?;
    let title = entry.title()?.to_string();
    let mut meta: Vec<String> = Vec::new();
    for v in entry.vars() {
        meta.push(v.name.clone());
        if let Some(d) = &v.description {
            meta.push(d.clone());
        }
    }
    let mut code_repo = None;
    match entry.fields() {
        TypeFields::Sql { dialect, database } => {
            meta.extend(dialect.iter().chain(database.iter()).cloned());
        }
        TypeFields::Cw {
            log_groups, region, ..
        } => {
            meta.extend(log_groups.iter().cloned());
            meta.extend(region.iter().cloned());
        }
        TypeFields::Link { url: Some(url) } => {
            meta.push(
                url.split_once("://")
                    .map_or(url.as_str(), |(_, r)| r)
                    .into(),
            );
        }
        TypeFields::Code { code: Some(c) } => {
            meta.push(c.repo.clone());
            meta.push(c.path.clone());
            meta.extend(c.symbol.iter().cloned());
            code_repo = Some(fold(&c.repo));
        }
        TypeFields::Runbook { steps } => {
            for step in steps {
                meta.push(step.id.clone());
                meta.push(step.title.clone());
                if let crate::runbook::StepTarget::Entry(ref entry_id) = step.target {
                    meta.push(entry_id.clone());
                }
            }
        }
        _ => {}
    }
    let id = entry.id();
    let tags: Vec<String> = entry.tags().to_vec();
    let mut links: Vec<String> = Vec::new();
    let (payload, notes, runbook_steps) = if ty == EntryType::Note {
        for link in crate::entry::link::extract_links(entry.notes()) {
            if !links.contains(&link.target_id) {
                links.push(link.target_id);
            }
        }
        let note = note_columns(entry.notes());
        meta.extend(note.meta);
        (note.payload, String::new(), Vec::new())
    } else if ty == EntryType::Runbook {
        let mut step_payload = Vec::new();
        let mut rb_steps = Vec::new();
        if let TypeFields::Runbook { steps } = entry.fields() {
            for (idx, step) in steps.iter().enumerate() {
                if let crate::runbook::StepTarget::Do(ref text) = step.target {
                    step_payload.push(text.clone());
                }
                if let Some(ref note) = step.note {
                    step_payload.push(note.clone());
                }
                rb_steps.push((
                    idx,
                    step.id.clone(),
                    step.target.entry_id().map(str::to_string),
                ));
            }
        }
        (step_payload.join("\n"), entry.notes().to_string(), rb_steps)
    } else {
        (
            entry.payload().map(|p| p.text.clone()).unwrap_or_default(),
            entry.notes().to_string(),
            Vec::new(),
        )
    };
    Some(Doc {
        entry_type: ty.name(),
        title_fold: fold(&title)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        cols: [
            title.clone(),
            tags.join(" "),
            id.replace(['/', '-', '_', '.'], " "),
            meta.join(" "),
            payload,
            notes,
        ],
        title,
        tags,
        vars: entry.vars().iter().map(|v| v.name.clone()).collect(),
        code_repo,
        updated: entry.updated().map(str::to_string),
        links,
        runbook_steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn build(id: &str, text: &str) -> Built {
        let file = WalkEntry {
            id: id.into(),
            rel_path: format!("{id}.md"),
            abs_path: PathBuf::new(),
        };
        build_doc(&file, text.as_bytes().to_vec())
    }

    fn doc(id: &str, text: &str) -> Doc {
        match build(id, text).verdict {
            Verdict::Indexed(d) => *d,
            Verdict::Skipped => panic!("skipped: {id}"),
        }
    }

    #[test]
    fn sql_entry_columns() {
        let d = doc(
            "sql/active-connections",
            "---\nkb_format: 1\ntype: sql\ntitle: Active connections\ntags: [db, Perf]\ndialect: postgres\ndatabase: orders\nvars:\n  limit: { kind: number, default: 5, description: Row cap }\n---\n\nSome notes.\n\n```sql\nselect 1 limit {{limit}};\n```\n",
        );
        assert_eq!(d.entry_type, "sql");
        assert_eq!(d.cols[0], "Active connections");
        assert_eq!(d.cols[1], "db perf");
        assert_eq!(d.cols[2], "sql active connections");
        assert_eq!(d.cols[3], "limit Row cap postgres orders");
        assert!(d.cols[4].contains("select 1 limit {{limit}}"));
        assert!(d.cols[5].contains("Some notes."));
        assert_eq!(d.vars, vec!["limit"]);
        assert_eq!(d.title_fold, "active connections");
    }

    #[test]
    fn type_specific_fields() {
        let d = doc(
            "cw/errs",
            "---\nkb_format: 1\ntype: cw\ntitle: Errs\nlog_groups: [/aws/lambda/app]\nregion: eu-west-1\n---\n\n```\nfields @message\n```\n",
        );
        assert_eq!(d.cols[3], "/aws/lambda/app eu-west-1");
        let d = doc(
            "link/rb",
            "---\nkb_format: 1\ntype: link\ntitle: Runbook\nurl: https://example.com/docs/a?x=1\n---\n",
        );
        assert_eq!(d.cols[3], "example.com/docs/a?x=1");
        let d = doc(
            "code/h",
            "---\nkb_format: 1\ntype: code\ntitle: Handler\ncode:\n  repo: Acme/Api\n  path: src/h.rs\n  line: 3\n  ref: main\n  symbol: handle\n---\n",
        );
        assert_eq!(d.cols[3], "Acme/Api src/h.rs handle");
        assert_eq!(d.code_repo.as_deref(), Some("acme/api"));
    }

    #[test]
    fn errors_skip_and_warnings_do_not() {
        let ok = "---\nkb_format: 1\ntype: prompt\ntitle: P\n---\n\n```\nhi\n```\n";
        assert!(matches!(build("p", ok).verdict, Verdict::Indexed(_)));
        let bad_type = ok.replace("prompt", "nope");
        assert_eq!(build("p", &bad_type).verdict, Verdict::Skipped);
        let secret = ok.replace("hi", "key = AKIAABCDEFGHIJKLMNOP");
        assert_eq!(build("p", &secret).verdict, Verdict::Skipped);
        let file = WalkEntry {
            id: "p".into(),
            rel_path: "p.md".into(),
            abs_path: PathBuf::new(),
        };
        let bad = build_doc(&file, vec![0xff, 0xfe, 0x00]);
        assert_eq!(bad.verdict, Verdict::Skipped);
        // KB051 is a warning: a file over 1 MiB is still indexed.
        let big = format!("{ok}\n{}\nzebrafish\n", "padding line\n".repeat(90_000));
        assert!(big.len() > crate::lint::rules::MAX_FILE_BYTES);
        assert!(matches!(build("big", &big).verdict, Verdict::Indexed(_)));
    }

    const NOTE: &str = "---\nkb_format: 1\ntype: note\ntags: [Redis]\n---\n\n# Role cache `TTL`\n\nThe **authorizer** caches roles. See [docs](https://example.com/docs/ttl).\n\n## Invalidation\n\n- revoke: DEL\n- grant: SETEX\n\n#### Deep heading\n\n| Event | TTL |\n|:--|--:|\n| grant | 300 s |\n\n```sh\nredis-cli ttl role:42\n```\n";

    #[test]
    fn note_columns() {
        let d = doc("note/role-cache", NOTE);
        assert_eq!(d.entry_type, "note");
        assert_eq!(
            d.cols[0], "Role cache TTL",
            "title from the heading, stripped"
        );
        assert_eq!(d.cols[1], "redis");
        assert_eq!(
            d.cols[3],
            "Role cache TTL example.com/docs/ttl Invalidation"
        );
        let body = &d.cols[4];
        for want in [
            "The authorizer caches roles.",
            "revoke: DEL\ngrant: SETEX",
            "Deep heading",
            "grant 300 s",
            "redis-cli ttl role:42",
        ] {
            assert!(body.contains(want), "{want:?} not in {body:?}");
        }
        assert!(
            !body.contains("**") && !body.contains("```") && !body.contains("##"),
            "{body:?}"
        );
        assert!(
            !body.contains("Invalidation"),
            "headings 1-3 live in meta: {body:?}"
        );
        assert_eq!(d.cols[5], "", "notes column stays empty");
        assert!(d.vars.is_empty());
    }

    #[test]
    fn note_payload_is_capped() {
        let big = format!(
            "---\nkb_format: 1\ntype: note\ntitle: Big\n---\n\n{}",
            "word \n\n".repeat(80_000)
        );
        let d = doc("note/big", &big);
        assert!(d.cols[4].len() <= NOTE_INDEX_BYTES);
        assert!(d.cols[4].len() > NOTE_INDEX_BYTES - 16);
    }
}
