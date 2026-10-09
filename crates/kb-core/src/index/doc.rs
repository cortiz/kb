//! Turning a file into what the index stores (FR-005), or deciding it must stay out.

use crate::entry::{Entry, TypeFields};
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
        _ => {}
    }
    let id = entry.id();
    let tags: Vec<String> = entry.tags().to_vec();
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
            entry.payload().map(|p| p.text.clone()).unwrap_or_default(),
            entry.notes().to_string(),
        ],
        title,
        tags,
        vars: entry.vars().iter().map(|v| v.name.clone()).collect(),
        code_repo,
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
}
