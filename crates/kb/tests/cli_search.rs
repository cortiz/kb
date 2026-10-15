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

// ---- notes ----

fn write_note(root: &std::path::Path, name: &str, body: &str, tags: &str) {
    let dir = root.join("note");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("{name}.md")),
        format!("---\nkb_format: 1\ntype: note\ntitle: {name}\ntags: [{tags}]\n---\n\n{body}"),
    )
    .unwrap();
}

#[test]
fn type_note_finds_only_notes_and_the_fixture_notes_are_searchable() {
    let (_d, root) = support::notes_kb();
    let found = ids(&root, &["type:note"]);
    assert!(
        found.contains(&"note/role-cache-ttl".to_string()),
        "{found:?}"
    );
    assert!(found.iter().all(|id| id.starts_with("note/")), "{found:?}");
    let all = ids(&root, &["redis"]);
    assert_eq!(
        all.first().map(String::as_str),
        Some("note/role-cache-ttl"),
        "{all:?}"
    );
    let by_flag = ids(&root, &["--type", "note", "ttl"]);
    assert!(
        by_flag.iter().all(|id| id.starts_with("note/")),
        "{by_flag:?}"
    );
}

#[test]
fn a_heading_outranks_the_body() {
    let (_d, root) = search_kb();
    write_note(
        &root,
        "alpha",
        "Nothing relevant here, except that the zebrafish is mentioned once.\n",
        "",
    );
    write_note(
        &root,
        "beta",
        "# Beta\n\n## Zebrafish handling\n\nSome text.\n",
        "",
    );
    write_note(
        &root,
        "gamma",
        "Another body mention of zebrafish, and a second zebrafish.\n",
        "",
    );
    let found = ids(&root, &["zebrafish", "type:note"]);
    assert_eq!(
        found.first().map(String::as_str),
        Some("note/beta"),
        "{found:?}"
    );
    assert_eq!(found.len(), 3, "{found:?}");
}

#[test]
fn in_body_searches_the_body_only() {
    let (_d, root) = search_kb();
    write_note(
        &root,
        "a",
        "# A\n\n## Zebrafish heading\n\nplain text.\n",
        "",
    );
    write_note(&root, "b", "# B\n\nthe zebrafish is in the body.\n", "");
    let found = ids(&root, &["zebrafish", "in:body"]);
    assert_eq!(found, ["note/b"]);
    let found = ids(&root, &["zebrafish", "in:payload", "type:note"]);
    assert_eq!(found, ["note/b"], "in:body is the payload column");
    let out = kb_in(&root)
        .args(["search", "zebrafish", "in:nothing"])
        .output()
        .unwrap();
    assert!(stderr(&out).contains("in:nothing"), "{}", stderr(&out));
}

#[test]
fn a_note_snippet_is_plain_text_from_the_best_paragraph() {
    let (_d, root) = search_kb();
    write_note(
        &root,
        "ttl",
        "# TTL\n\nIntro with **no** match at all.\n\nThe `authorizer` checks Redis with a **cache TTL** of 300 s to protect the [database](https://example.com/db). Cache TTL tuning matters.\n\n```\nunrelated\n```\n",
        "",
    );
    let out = kb_in(&root)
        .args(["search", "cache", "ttl", "in:body", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let r = &doc["results"][0];
    assert_eq!(r["id"], "note/ttl");
    let snippet = &r["snippet"];
    assert_eq!(snippet["field"], "payload");
    let text = snippet["text"].as_str().unwrap();
    assert!(text.contains("cache TTL of 300 s"), "{text}");
    assert!(
        !text.contains("**") && !text.contains('`') && !text.contains("]("),
        "{text}"
    );
    assert!(
        !text.contains("Intro"),
        "the best paragraph, not the first: {text}"
    );
    let ranges = snippet["ranges"].as_array().unwrap();
    assert!(!ranges.is_empty());
    let chars: Vec<char> = text.chars().collect();
    let first = &ranges[0];
    let hit: String = chars
        [first[0].as_u64().unwrap() as usize..first[1].as_u64().unwrap() as usize]
        .iter()
        .collect();
    assert!(
        hit.eq_ignore_ascii_case("cache") || hit.eq_ignore_ascii_case("ttl"),
        "{hit}"
    );
}

#[test]
fn note_body_words_are_not_typo_candidates_but_titles_and_headings_are() {
    let (_d, root) = search_kb();
    write_note(
        &root,
        "plain",
        "# Plain\n\nthe quokkaword lives only in the body.\n\n## Wombatheading\n\ntext\n",
        "",
    );
    // exact search finds body words
    assert_eq!(ids(&root, &["quokkaword"]), ["note/plain"]);
    // a typo of a heading word is corrected
    assert_eq!(ids(&root, &["wombatheadng"]), ["note/plain"]);
    // a typo of a body-only word is not
    assert!(ids(&root, &["quokkaward"]).is_empty());
}

#[test]
fn runbook_search_indexing_and_filters() {
    let (_d, root) = search_kb();
    fs::create_dir_all(root.join("deploy")).unwrap();
    fs::write(
        root.join("deploy/authorizer.md"),
        r#"---
kb_format: 1
type: runbook
title: Deploy Authorizer Runbook
tags: [deployrunbook]
steps:
  - id: step-1
    title: Check baseline
    entry: sumo/5xx-by-authorizer
  - id: step-2
    title: Deploy command
    entry: cmd/deploy-service
  - id: step-3
    title: Manual verification
    do: Verify telemetry dashboards
---
Special instructions for deployment operations.
"#,
    )
    .unwrap();

    // 1. Matches on title
    let found = ids(&root, &["Deploy", "Authorizer", "Runbook"]);
    assert!(found.contains(&"deploy/authorizer".to_string()), "matches on title: {found:?}");

    // 2. Matches on tag
    let found = ids(&root, &["tag:deployrunbook"]);
    assert_eq!(found, vec!["deploy/authorizer"]);

    // 3. Matches on step title
    let found = ids(&root, &["baseline"]);
    assert!(found.contains(&"deploy/authorizer".to_string()), "matches on step title: {found:?}");

    // 4. Matches on manual step text
    let found = ids(&root, &["telemetry", "dashboards"]);
    assert!(found.contains(&"deploy/authorizer".to_string()), "matches on do text: {found:?}");

    // 5. Matches on notes
    let found = ids(&root, &["special", "deployment", "operations"]);
    assert!(found.contains(&"deploy/authorizer".to_string()), "matches on notes: {found:?}");

    // 6. type:runbook filter
    let found = ids(&root, &["type:runbook"]);
    assert_eq!(found, vec!["deploy/authorizer"]);

    // 7. runbook:<id> filter returns referenced entries in step order
    let found = ids(&root, &["runbook:deploy/authorizer"]);
    assert_eq!(found, vec!["sumo/5xx-by-authorizer", "cmd/deploy-service"]);

    // 8. used-by:runbook filter returns entries referenced by runbooks
    let found = ids(&root, &["used-by:runbook"]);
    assert!(found.contains(&"sumo/5xx-by-authorizer".to_string()));
    assert!(found.contains(&"cmd/deploy-service".to_string()));
    assert!(!found.contains(&"deploy/authorizer".to_string()));
}

