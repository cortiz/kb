//! `kb search` end to end (User Stories 1, 3 and 4).

mod support;

use std::fs;

use serde_json::Value;
use support::{entry_text, kb_in, search_kb};

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

fn ids(root: &std::path::Path, args: &[&str]) -> Vec<String> {
    let out = kb_in(root)
        .arg("search")
        .args(args)
        .arg("--ids")
        .output()
        .unwrap();
    stdout(&out).lines().map(str::to_string).collect()
}

#[test]
fn finds_the_headline_entry() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["search", "5xx", "auth"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let first = stdout(&out).lines().next().unwrap().to_string();
    assert!(first.starts_with("sumo"), "{first}");
    assert!(first.contains("sumo/5xx-by-authorizer") && first.contains("5xx by authorizer"));
    assert!(first.contains("#auth #lambda"), "{first}");
    assert!(stderr(&out).contains("index rebuilt (32 entries)"));
    // the second run has nothing to report
    let out = kb_in(&root)
        .args(["search", "5xx", "auth"])
        .output()
        .unwrap();
    assert_eq!(stderr(&out), "");
}

#[test]
fn piped_output_has_no_escape_codes() {
    let (_d, root) = search_kb();
    let out = kb_in(&root).args(["search", "auth"]).output().unwrap();
    assert!(!stdout(&out).contains('\x1b'));
    let out = kb_in(&root)
        .args(["search", "auth", "--ids"])
        .output()
        .unwrap();
    let text = stdout(&out);
    assert!(!text.contains('\x1b'));
    assert!(
        text.lines().all(|l| !l.contains(' ') && l.contains('/')),
        "{text}"
    );
    assert!(text.lines().any(|l| l == "sumo/5xx-by-authorizer"));
}

#[test]
fn json_has_the_documented_shape() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["search", "5xx", "auth", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["query"], "5xx auth");
    assert!(doc["total"].as_u64().unwrap() >= 1);
    let r = &doc["results"][0];
    assert_eq!(r["id"], "sumo/5xx-by-authorizer");
    assert_eq!(r["type"], "sumo");
    assert_eq!(r["title"], "5xx by authorizer");
    assert_eq!(r["tags"], serde_json::json!(["auth", "lambda"]));
    assert_eq!(r["path"], "sumo/5xx-by-authorizer.md");
    assert!(r["score"].is_number());
    assert!(["title_exact", "exact", "typo"].contains(&r["tier"].as_str().unwrap()));
    let title = r["matches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["field"] == "title")
        .unwrap();
    assert_eq!(title["ranges"], serde_json::json!([[0, 3], [7, 17]]));
    let snippet = &r["snippet"];
    assert!(snippet["text"].as_str().unwrap().chars().count() <= 160);
    assert_eq!(snippet["field"], "title");
    // exactly the documented keys
    let mut keys: Vec<_> = r.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "id", "matches", "path", "score", "snippet", "tags", "tier", "title", "type"
        ]
    );
}

#[test]
fn ids_and_json_conflict() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["search", "auth", "--ids", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn no_match_is_exit_1_with_a_message_on_stderr() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["search", "zzzznotaword"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert!(stderr(&out).contains("kb: no matches"));
    let out = kb_in(&root)
        .args(["search", "zzzznotaword", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["results"], serde_json::json!([]));
}

#[test]
fn an_empty_kb_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let out = kb_in(dir.path()).args(["search", "x"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert!(
        stderr(&out).contains("no entries in KB (run kb init)"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn entries_with_errors_are_skipped_and_counted() {
    let (_d, root) = search_kb();
    fs::write(root.join("bad.md"), "no frontmatter at all").unwrap();
    fs::write(
        root.join("secret.md"),
        entry_text("Leaky", "", "key = AKIAABCDEFGHIJKLMNOP", ""),
    )
    .unwrap();
    let out = kb_in(&root).args(["search", "leaky"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("2 entries skipped due to errors (run kb lint)"),
        "{}",
        stderr(&out)
    );
    let out = kb_in(&root).args(["search", "auth"]).output().unwrap();
    assert!(
        stderr(&out).contains("2 entries skipped"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn limit_defaults_to_20_and_zero_means_all() {
    let (_d, root) = search_kb();
    assert_eq!(ids(&root, &[]).len(), 20);
    assert_eq!(ids(&root, &["--limit", "3"]).len(), 3);
    assert_eq!(ids(&root, &["--limit", "0"]).len(), 32);
    let out = kb_in(&root)
        .args(["search", "--limit", "abc"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn search_never_records_usage_or_writes_outside_kb_dir() {
    let (_d, root) = search_kb();
    let before: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    for _ in 0..3 {
        kb_in(&root).args(["search", "auth"]).output().unwrap();
    }
    assert!(!root.join(".kb/state.json").exists());
    assert_eq!(
        fs::read_to_string(root.join(".kb/.gitignore")).unwrap(),
        "*\n"
    );
    let mut after: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let mut before = before;
    before.push(".kb".into());
    before.sort();
    after.sort();
    assert_eq!(before, after);
}

#[test]
fn a_missing_root_is_exit_2() {
    let out = kb_in(std::path::Path::new("/no/such/kb"))
        .args(["search", "x"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).starts_with("kb: "));
    let out = kb_in(std::path::Path::new("/no/such/kb"))
        .args(["search", "x", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["error"]["kind"], "io");
}

// ---- filters (US3) ----

#[test]
fn flags_equal_syntax() {
    let (_d, root) = search_kb();
    let a = ids(&root, &["--type", "cw", "--tag", "prod"]);
    let b = ids(&root, &["type:cw tag:prod"]);
    assert!(!a.is_empty());
    assert_eq!(a, b);
    assert_eq!(
        ids(&root, &["latency", "--not-tag", "stage"]),
        ids(&root, &["latency -tag:stage"])
    );
    let both = ids(&root, &["--type", "cw", "--type", "link"]);
    assert!(
        both.iter().any(|i| i.starts_with("cw/")) && both.iter().any(|i| i.starts_with("link/"))
    );
    assert!(
        both.iter()
            .all(|i| i.starts_with("cw/") || i.starts_with("link/"))
    );
}

#[test]
fn minus_tag_works_anywhere_on_the_command_line() {
    let (_d, root) = search_kb();
    let want = ids(&root, &["latency", "--not-tag", "stage"]);
    assert!(!want.is_empty() && !want.contains(&"cw/latency-stage".to_string()));
    assert_eq!(ids(&root, &["latency", "-tag:stage"]), want);
    assert_eq!(ids(&root, &["-tag:stage", "latency"]), want);
    assert_eq!(ids(&root, &["latency -tag:stage"]), want);
    let out = kb_in(&root)
        .args(["search", "latency", "-tag:stage", "--limit", "1", "--ids"])
        .output()
        .unwrap();
    assert_eq!(stdout(&out).lines().count(), 1);
}

#[test]
fn filters_and_phrases() {
    let (_d, root) = search_kb();
    assert_eq!(
        ids(&root, &["tag:prod", "tag:auth"]),
        ["cw/authorizer-latency", "sumo/login-failures"]
    );
    assert_eq!(ids(&root, &["\"role cache\""]), ["prompt/role-cache-notes"]);
    assert_eq!(
        ids(&root, &["var:region"]),
        ["cmd/deploy-service", "cmd/kubectl-pods"]
    );
}

#[test]
fn bad_type_flag_is_a_usage_error() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["search", "--type", "bogus"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("sql, sumo, cw, cmd, prompt, link, code"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn unknown_filter_key_gets_a_hint_once() {
    let (_d, root) = search_kb();
    let out = kb_in(&root).args(["search", "foo:bar"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert_eq!(err.matches("unknown filter \"foo:\"").count(), 1, "{err}");
    assert!(err.contains("valid filters: type, tag, -tag, var, repo, in"));
}

#[test]
fn no_arguments_lists_everything() {
    let (_d, root) = search_kb();
    let out = kb_in(&root).arg("search").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out).lines().count(), 20);
}

// ---- forgiving search (US4) ----

#[test]
fn typos_prefixes_and_accents() {
    let (_d, root) = search_kb();
    for q in ["authroizer", "authorizr", "authori"] {
        let found = ids(&root, &[q]);
        assert!(
            found.contains(&"sumo/5xx-by-authorizer".to_string()),
            "{q}: {found:?}"
        );
    }
    assert_eq!(ids(&root, &["configuracion"])[0], "sql/configuracion-red");
    let out = kb_in(&root)
        .args(["search", "authroizer", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        doc["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["tier"] == "typo")
    );
    let out = kb_in(&root)
        .args(["search", "authorizer", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["results"][0]["tier"], "exact");
}

// ---- color (FR-019) ----

#[test]
fn print_search_colors_only_when_asked() {
    // The binary is tested through pipes (never a terminal); the renderer is the same code path.
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .env("NO_COLOR", "1")
        .args(["search", "auth"])
        .output()
        .unwrap();
    assert!(!stdout(&out).contains('\x1b'));
}
