//! Integration tests for `kb runbook new`.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use support::kb_in;

const NOW: &str = "2026-10-15T12:00:00Z";

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

fn fixture_kb() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("kb");
    fs::create_dir_all(root.join("sumo")).unwrap();
    fs::create_dir_all(root.join("cmd")).unwrap();
    fs::create_dir_all(root.join("cw")).unwrap();

    fs::write(
        root.join("sumo/5xx.md"),
        r#"---
kb_format: 1
type: sumo
title: 5xx errors by service
vars:
  env: { kind: enum, options: [dev, stage, prod], default: stage }
---

_sourceCategory=api/{{env}} status >= 500
"#,
    )
    .unwrap();

    fs::write(
        root.join("cmd/deploy.md"),
        r#"---
kb_format: 1
type: cmd
title: Deploy service
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  version: { kind: text }
---

deploy.sh --env {{env}} --version {{version}}
"#,
    )
    .unwrap();

    fs::write(
        root.join("cw/p99.md"),
        r#"---
kb_format: 1
type: cw
title: P99 latency
log_groups: ["/aws/lambda/service"]
---

fields @timestamp, @message
| filter @message like /REPORT/
"#,
    )
    .unwrap();

    (d, root)
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    kb_in(root)
        .args(args)
        .env("KB_TEST_NOW", NOW)
        .output()
        .unwrap()
}

#[test]
fn runbook_new_creates_runbook_with_yes() {
    let (_d, root) = fixture_kb();
    let out = run(
        &root,
        &[
            "runbook",
            "new",
            "Deploy authorizer",
            "sumo/5xx",
            "cmd/deploy",
            "cw/p99",
            "--tag",
            "deploy",
            "--yes",
        ],
    );

    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "runbook/deploy-authorizer\n");

    let file_content = fs::read_to_string(root.join("runbook/deploy-authorizer.md")).unwrap();
    assert!(file_content.contains("type: runbook"));
    assert!(file_content.contains("title: Deploy authorizer"));
    assert!(file_content.contains("tags: [deploy]"));
    assert!(file_content.contains("entry: sumo/5xx"));
    assert!(file_content.contains("entry: cmd/deploy"));
    assert!(file_content.contains("entry: cw/p99"));
    // env variable lifted
    assert!(file_content.contains("env:"));
    assert!(file_content.contains("options: [stage, prod]"));
}

#[test]
fn runbook_new_custom_path() {
    let (_d, root) = fixture_kb();
    let out = run(
        &root,
        &[
            "runbook",
            "new",
            "Custom Path Runbook",
            "sumo/5xx",
            "--path",
            "ops/my-runbook",
            "--yes",
        ],
    );

    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "ops/my-runbook\n");
    assert!(root.join("ops/my-runbook.md").exists());
}

#[test]
fn runbook_new_json_output() {
    let (_d, root) = fixture_kb();
    let out = run(
        &root,
        &[
            "runbook",
            "new",
            "JSON Runbook",
            "sumo/5xx",
            "--yes",
            "--json",
        ],
    );

    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(json["id"], "runbook/json-runbook");
    assert_eq!(json["type"], "runbook");
}

#[test]
fn runbook_new_missing_entry_fails() {
    let (_d, root) = fixture_kb();
    let out = run(
        &root,
        &[
            "runbook",
            "new",
            "Broken Runbook",
            "sumo/5xx",
            "missing/entry",
            "--yes",
        ],
    );

    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("entry 'missing/entry' does not exist"));
}

#[test]
fn runbook_new_without_yes_in_non_terminal_fails() {
    let (_d, root) = fixture_kb();
    let out = support::kb_without_terminal(
        &["--root", root.to_str().unwrap(), "runbook", "new", "Interactive Runbook", "sumo/5xx"],
        &[("KB_TEST_NOW", NOW)],
    );

    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("kb runbook new needs --yes when there is no terminal"));
}
