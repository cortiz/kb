//! Lints every fixture and compares the findings with its `.expect` sidecar.

use std::fs;
use std::path::{Path, PathBuf};

use kb_core::lint::{LintReport, lint};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .canonicalize()
        .unwrap()
}

fn md_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(md_files(&path));
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Findings as `.expect` lines: `line:col CODE`, then `suppressed line CODE`.
fn render(report: &LintReport) -> Vec<String> {
    let mut lines: Vec<String> = report
        .findings
        .iter()
        .map(|f| format!("{}:{} {}", f.line, f.column, f.code))
        .collect();
    lines.extend(
        report
            .suppressions
            .iter()
            .map(|s| format!("suppressed {} {}", s.line, s.code)),
    );
    lines
}

fn check_dir(dir: &str) -> usize {
    let root = fixtures().join(dir);
    let files = md_files(&root);
    let mut failures = Vec::new();
    for file in &files {
        let expect_path = file.with_extension("expect");
        let expected: Vec<String> = fs::read_to_string(&expect_path)
            .unwrap_or_else(|_| panic!("missing {}", expect_path.display()))
            .lines()
            .map(str::to_string)
            .collect();
        let report = lint(&root, std::slice::from_ref(file)).unwrap();
        assert_eq!(report.entries, 1, "{}", file.display());
        let got = render(&report);
        if got != expected {
            failures.push(format!(
                "{}\n  expected: {expected:?}\n  got:      {got:?}\n  {:#?}",
                file.display(),
                report.findings
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    files.len()
}

#[test]
fn valid_fixtures_have_no_findings() {
    let root = fixtures().join("valid");
    let report = lint(&root, &[]).unwrap();
    assert_eq!(report.entries, md_files(&root).len());
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
    assert!(report.suppressions.is_empty());
}

#[test]
fn search_fixture_kb_is_lint_clean() {
    let root = fixtures().join("search");
    let report = lint(&root, &[]).unwrap();
    assert_eq!(report.entries, md_files(&root).len());
    assert!(report.entries >= 30);
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
    assert!(report.suppressions.is_empty());
}

#[test]
fn tui_fixture_kb_is_lint_clean() {
    let root = fixtures().join("tui");
    let report = lint(&root, &[]).unwrap();
    assert_eq!(report.entries, md_files(&root).len());
    assert!(report.entries >= 7);
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
    assert!(report.suppressions.is_empty());
}

#[test]
fn invalid_fixtures_match_expectations() {
    assert!(check_dir("invalid") >= 40);
}

#[test]
fn secret_fixtures_match_expectations() {
    assert!(check_dir("secrets") >= 9);
}

/// Writes `bytes` as `x.md` in a fresh KB and returns the rendered findings.
fn lint_bytes(bytes: &[u8]) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::write(root.join("x.md"), bytes).unwrap();
    render(&lint(&root, &[]).unwrap())
}

#[test]
fn invalid_utf8_is_reported() {
    assert_eq!(
        lint_bytes(b"---\nkb_format: 1\ntitle: \xFF\n---\n"),
        vec!["3:8 KB005-invalid-utf8"]
    );
}

#[test]
fn mixed_line_endings_warn() {
    let text = "---\r\nkb_format: 1\r\ntype: link\ntitle: T\r\nurl: https://example.com\r\n---\r\n";
    assert_eq!(
        lint_bytes(text.as_bytes()),
        vec!["3:1 KB050-mixed-line-endings"]
    );
}

#[test]
fn large_files_warn_but_are_parsed() {
    let mut text = String::from("---\nkb_format: 1\ntype: prompt\ntitle: Big\n---\n\n```\n");
    while text.len() <= 1_048_576 {
        text.push_str("A long prompt line that repeats.\n");
    }
    text.push_str("```\n");
    assert_eq!(
        lint_bytes(text.as_bytes()),
        vec!["1:1 KB051-file-too-large"]
    );
    let mut bad = text.replace("type: prompt", "type: nope");
    bad.truncate(bad.len());
    assert_eq!(
        lint_bytes(bad.as_bytes()),
        vec!["1:1 KB051-file-too-large", "3:7 KB001-unknown-type"]
    );
}

#[test]
fn case_collisions_across_the_kb() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let entry = "---\nkb_format: 1\ntype: link\ntitle: T\nurl: https://example.com\n---\n";
    fs::create_dir_all(root.join("a")).unwrap();
    fs::write(root.join("a/Note.md"), entry).unwrap();
    if fs::write(root.join("a/note.md"), entry).is_err()
        || fs::read_dir(root.join("a")).unwrap().count() < 2
    {
        return; // case-insensitive file system: covered by the walk unit tests
    }
    let report = lint(&root, &[]).unwrap();
    let got: Vec<_> = report
        .findings
        .iter()
        .map(|f| (f.path.as_str(), f.code.to_string()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("a/Note.md", "KB053-id-case-collision".into()),
            ("a/note.md", "KB053-id-case-collision".into())
        ]
    );
}
