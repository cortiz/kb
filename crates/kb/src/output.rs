//! Human and JSON rendering. Only `kb search` colors its output, and only on a terminal (see
//! `color.rs`); JSON and `--ids` never contain escape codes.

use std::ops::Range;

use kb_core::index::{IndexState, IndexStatus};
use kb_core::init::InitReport;
use kb_core::lint::LintReport;
use kb_core::query::Field;
use kb_core::search::{ReindexReport, SearchResult};
use serde_json::json;

use crate::color::highlight;

/// An error that ends the command with exit code 2.
#[derive(Debug)]
pub struct CliError {
    /// `usage`, `config` or `io`.
    pub kind: &'static str,
    pub message: String,
}

impl CliError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> CliError {
        CliError {
            kind,
            message: message.into(),
        }
    }
}

/// Prints `kb: <message>` to stderr and, with `--json`, an error document to stdout.
pub fn print_error(error: &CliError, json: bool) {
    eprintln!("kb: {}", error.message);
    if json {
        println!(
            "{}",
            json!({ "error": { "kind": error.kind, "message": error.message } })
        );
    }
}

/// Human lint output: one block per finding, one line per suppression, then the summary.
pub fn print_lint(report: &LintReport, quiet: bool) {
    let mut out = String::new();
    if !quiet {
        for f in &report.findings {
            let severity = match f.severity {
                kb_core::lint::Severity::Error => "error",
                kb_core::lint::Severity::Warning => "warning",
                kb_core::lint::Severity::Info => "info",
            };
            out.push_str(&format!(
                "{}:{}:{}: {severity}[{}]: {}\n",
                f.path, f.line, f.column, f.code, f.message
            ));
            if let Some(fix) = &f.fix {
                out.push_str(&format!("  fix: {fix}\n"));
            }
        }
        for s in &report.suppressions {
            out.push_str(&format!("{}:{}: suppressed[{}]\n", s.path, s.line, s.code));
        }
        if !out.is_empty() {
            out.push('\n');
        }
    }
    out.push_str(&report.summary_line());
    println!("{out}");
}

/// Human init output: a headline, then one line per path.
pub fn print_init(report: &InitReport, hint: Option<&str>) {
    let mut out = if report.created.is_empty() {
        format!("KB at {} is up to date\n", report.root.display())
    } else {
        format!("Initialized KB at {}\n", report.root.display())
    };
    for (path, created) in &report.steps {
        out.push_str(&format!(
            "  {:<8} {path}\n",
            if *created { "created" } else { "exists" }
        ));
    }
    if let Some(hint) = hint {
        out.push_str(&format!("\nhint: {hint}\n"));
    }
    print!("{out}");
}

/// `kb init --json`, per contracts/init-report.schema.json.
pub fn print_init_json(report: &InitReport, hint: Option<&str>) {
    let doc = json!({
        "version": 1,
        "root": report.root.to_string_lossy(),
        "created": report.created,
        "existing": report.existing,
        "examples": report.examples.as_str(),
        "hint": hint,
    });
    println!("{doc}");
}

fn ranges_of(r: &SearchResult, field: Field) -> Vec<Range<usize>> {
    r.matches
        .iter()
        .find(|m| m.field == field)
        .map(|m| m.ranges.clone())
        .unwrap_or_default()
}

/// Tag ranges are over the tags joined by spaces; this maps them onto each `#tag`.
fn tags_line(r: &SearchResult, color: bool) -> String {
    let ranges = ranges_of(r, Field::Tags);
    let mut offset = 0;
    let mut out = Vec::new();
    for tag in &r.tags {
        let len = tag.chars().count();
        let local: Vec<Range<usize>> = ranges
            .iter()
            .filter(|x| x.start >= offset && x.end <= offset + len)
            .map(|x| x.start - offset..x.end - offset)
            .collect();
        out.push(format!("#{}", highlight(tag, &local, color)));
        offset += len + 1;
    }
    out.join(" ")
}

/// Human search output: one line per result (type, ID, title, tags).
pub fn print_search(results: &[SearchResult], color: bool) {
    print!("{}", render_search(results, color));
}

pub fn render_search(results: &[SearchResult], color: bool) -> String {
    let width = results
        .iter()
        .map(|r| r.entry_type.name().len())
        .max()
        .unwrap_or(0);
    let id_width = results
        .iter()
        .map(|r| r.id.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for r in results {
        let pad = " ".repeat(id_width - r.id.chars().count());
        out.push_str(&format!(
            "{:<width$}  {}{pad}  {}  {}\n",
            r.entry_type.name(),
            highlight(&r.id, &ranges_of(r, Field::Id), color),
            highlight(&r.title, &ranges_of(r, Field::Title), color),
            tags_line(r, color),
        ));
    }
    out.replace("  \n", "\n")
}

pub fn print_ids(results: &[SearchResult]) {
    for r in results {
        println!("{}", r.id);
    }
}

fn ranges_json(ranges: &[Range<usize>]) -> serde_json::Value {
    json!(ranges.iter().map(|r| [r.start, r.end]).collect::<Vec<_>>())
}

/// `kb search --json`, per contracts/search-report.schema.json.
pub fn print_search_json(query: &str, total: usize, results: &[SearchResult]) {
    let results: Vec<_> = results
        .iter()
        .map(|r| {
            let mut obj = serde_json::Map::new();
            obj.insert("id".into(), json!(r.id));
            obj.insert("type".into(), json!(r.entry_type.name()));
            obj.insert("title".into(), json!(r.title));
            obj.insert("tags".into(), json!(r.tags));
            obj.insert("path".into(), json!(r.path));
            obj.insert("score".into(), json!(r.score));
            obj.insert("tier".into(), json!(r.tier.name()));
            if r.unreviewed {
                obj.insert("unreviewed".into(), json!(true));
            }
            obj.insert(
                "matches".into(),
                json!(r.matches.iter().map(|m| json!({
                    "field": m.field.name(),
                    "ranges": ranges_json(&m.ranges),
                })).collect::<Vec<_>>()),
            );
            obj.insert(
                "snippet".into(),
                json!(r.snippet.as_ref().map(|s| json!({
                    "field": s.field.name(),
                    "text": s.text,
                    "ranges": ranges_json(&s.ranges),
                }))),
            );
            serde_json::Value::Object(obj)
        })
        .collect();
    println!(
        "{}",
        json!({ "query": query, "total": total, "results": results })
    );
}

fn mib(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// `YYYY-MM-DD HH:MM:SS UTC` from Unix seconds (civil-from-days, no date crate).
pub fn utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn state_name(s: &IndexState) -> &'static str {
    match s {
        IndexState::Ok => "ok",
        IndexState::Missing => "missing",
        IndexState::Unusable(_) => "unusable",
    }
}

fn status_json(s: &IndexStatus, duration_ms: Option<u128>) -> serde_json::Value {
    let mut doc = json!({
        "path": s.path.to_string_lossy(),
        "state": state_name(&s.state),
        "entries_indexed": s.entries_indexed,
        "entries_skipped": s.entries_skipped,
        "size_bytes": s.size_bytes,
        "index_version": s.index_version,
        "last_full_build": s.last_full_build,
    });
    if let IndexState::Unusable(reason) = &s.state {
        doc["reason"] = json!(reason);
    }
    if let Some(ms) = duration_ms {
        doc["duration_ms"] = json!(ms);
    }
    doc
}

/// `kb index status`.
pub fn print_index_status(s: &IndexStatus, json: bool) {
    if json {
        println!("{}", status_json(s, None));
        return;
    }
    match &s.state {
        IndexState::Missing => {
            println!("Index: none (it is built on the next search, or run kb reindex)");
        }
        IndexState::Unusable(reason) => {
            println!("Index: unusable ({reason}); it is rebuilt on the next search");
        }
        IndexState::Ok => {
            println!("Index:      {}", s.path.display());
            println!("Version:    {}", s.index_version);
            println!(
                "Entries:    {} indexed, {} skipped due to errors",
                s.entries_indexed, s.entries_skipped
            );
            println!("Size:       {}", mib(s.size_bytes));
            println!(
                "Last build: {}",
                s.last_full_build.map_or("never".into(), utc)
            );
        }
    }
}

/// `kb reindex`.
pub fn print_reindex(r: &ReindexReport, s: &IndexStatus, json: bool) {
    if json {
        println!("{}", status_json(s, Some(r.duration.as_millis())));
    } else {
        println!(
            "Reindexed {} entries ({} skipped due to errors) in {:.2} s",
            r.indexed,
            r.skipped,
            r.duration.as_secs_f64()
        );
    }
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::{render_search, utc};
    use kb_core::entry::EntryType;
    use kb_core::query::Field;
    use kb_core::search::{FieldMatch, SearchResult, Tier};

    fn result() -> SearchResult {
        SearchResult {
            id: "sumo/5xx-by-authorizer".into(),
            use_count: 0,
            updated: None,
            modified: None,
            entry_type: EntryType::Sumo,
            title: "5xx by authorizer".into(),
            tags: vec!["auth".into(), "lambda".into()],
            path: "sumo/5xx-by-authorizer.md".into(),
            score: 1.0,
            tier: Tier::Exact,
            matches: vec![
                FieldMatch {
                    field: Field::Title,
                    ranges: vec![0..3, 7..17],
                },
                FieldMatch {
                    field: Field::Tags,
                    ranges: vec![5..11],
                },
            ],
            snippet: None,
            unreviewed: false,
        }
    }

    #[test]
    fn colors_matched_ranges_only_when_asked() {
        let plain = render_search(&[result()], false);
        assert_eq!(
            plain,
            "sumo  sumo/5xx-by-authorizer  5xx by authorizer  #auth #lambda\n"
        );
        let colored = render_search(&[result()], true);
        assert!(
            colored.contains("\x1b[1;33m5xx\x1b[0m by \x1b[1;33mauthorizer\x1b[0m"),
            "{colored:?}"
        );
        assert!(
            colored.contains("#auth #\x1b[1;33mlambda\x1b[0m"),
            "{colored:?}"
        );
        assert_eq!(
            colored.replace("\x1b[1;33m", "").replace("\x1b[0m", ""),
            plain
        );
    }

    #[test]
    fn formats_utc_without_a_date_crate() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(1_791_462_605), "2026-10-08 12:30:05 UTC");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00:00 UTC");
    }
}
