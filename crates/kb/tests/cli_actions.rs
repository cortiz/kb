//! Integration tests for `kb actions` CLI command.

mod support;

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use support::kb_in;

fn setup_actions_test_env() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let kb_root = dir.path().join("kb");
    let home = dir.path().join("home");
    fs::create_dir_all(&kb_root).unwrap();
    fs::create_dir_all(home.join(".config/kb")).unwrap();

    fs::write(
        kb_root.join("link-auth.md"),
        r#"---
kb_format: 1
type: link
title: Auth Dashboard
link:
  url: https://auth.example.com
---
"#,
    )
    .unwrap();

    fs::write(
        kb_root.join("sumo-logs.md"),
        r#"---
kb_format: 1
type: sumo
title: 5xx Errors
sumo:
  range: -15m
---
_sourceCategory=prod/api status>=500
"#,
    )
    .unwrap();

    fs::write(
        kb_root.join("cw-latency.md"),
        r#"---
kb_format: 1
type: cw
title: API Latency
cw:
  region: us-east-1
  log_groups:
    - /aws/lambda/api
  range: -1h
---
fields @timestamp, @duration | filter @duration > 1000
"#,
    )
    .unwrap();

    fs::write(
        kb_root.join("cmd-tail.md"),
        r#"---
kb_format: 1
type: cmd
title: Tail Logs
shell: bash
---
tail -f /var/log/syslog
"#,
    )
    .unwrap();

    fs::write(
        kb_root.join("code-auth.md"),
        r#"---
kb_format: 1
type: code
title: Auth Handler Code
code:
  repo: example/auth
  path: src/Auth.java
  line: 42
  ref: a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2
---
"#,
    )
    .unwrap();

    (dir, kb_root, home)
}

fn kb_cmd(kb_root: &Path, home: &Path) -> assert_cmd::Command {
    let mut cmd = kb_in(kb_root);
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"));
    cmd
}

#[test]
fn cli_actions_link_json_and_text() {
    let (_dir, kb_root, home) = setup_actions_test_env();

    // 1. Text output
    let out = kb_cmd(&kb_root, &home)
        .args(["actions", "link-auth"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Actions for Auth Dashboard (link):"));
    assert!(stdout.contains("[o] Open Link"));
    assert!(stdout.contains("[c] Copy URL"));

    // 2. JSON output
    let out_json = kb_cmd(&kb_root, &home)
        .args(["actions", "link-auth", "--json"])
        .output()
        .unwrap();
    assert_eq!(out_json.status.code(), Some(0));
    let json: Value = serde_json::from_slice(&out_json.stdout).unwrap();
    assert_eq!(json["id"], "link-auth");
    assert_eq!(json["type"], "link");
    assert_eq!(json["primary_action"], "open-link");
    let actions = json["actions"].as_array().expect("actions array");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0]["id"], "copy");
    assert_eq!(actions[0]["key"], "c");
    assert_eq!(actions[1]["id"], "open-link");
    assert_eq!(actions[1]["key"], "o");
    assert_eq!(actions[1]["is_primary"], true);
}

#[test]
fn cli_actions_sumo_and_cw_json() {
    let (_dir, kb_root, home) = setup_actions_test_env();

    // Sumo JSON
    let out_sumo = kb_cmd(&kb_root, &home)
        .args(["actions", "sumo-logs", "--json"])
        .output()
        .unwrap();
    assert_eq!(out_sumo.status.code(), Some(0));
    let sumo_json: Value = serde_json::from_slice(&out_sumo.stdout).unwrap();
    assert_eq!(sumo_json["primary_action"], "open-sumo");
    let actions = sumo_json["actions"].as_array().unwrap();
    assert!(
        actions
            .iter()
            .any(|a| a["id"] == "open-sumo" && a["key"] == "o")
    );

    // CloudWatch JSON
    let out_cw = kb_cmd(&kb_root, &home)
        .args(["actions", "cw-latency", "--json"])
        .output()
        .unwrap();
    assert_eq!(out_cw.status.code(), Some(0));
    let cw_json: Value = serde_json::from_slice(&out_cw.stdout).unwrap();
    assert_eq!(cw_json["primary_action"], "open-cw");
    let actions = cw_json["actions"].as_array().unwrap();
    assert!(
        actions
            .iter()
            .any(|a| a["id"] == "open-cw" && a["key"] == "o")
    );
}

#[test]
fn cli_actions_cmd_and_code() {
    let (_dir, kb_root, home) = setup_actions_test_env();

    // Cmd JSON
    let out_cmd = kb_cmd(&kb_root, &home)
        .args(["actions", "cmd-tail", "--json"])
        .output()
        .unwrap();
    assert_eq!(out_cmd.status.code(), Some(0));
    let cmd_json: Value = serde_json::from_slice(&out_cmd.stdout).unwrap();
    let actions = cmd_json["actions"].as_array().unwrap();
    assert!(
        actions
            .iter()
            .any(|a| a["id"] == "run" && a["key"] == "r" && a["needs_confirm"] == true)
    );

    // Code JSON
    let out_code = kb_cmd(&kb_root, &home)
        .args(["actions", "code-auth", "--json"])
        .output()
        .unwrap();
    assert_eq!(out_code.status.code(), Some(0));
    let code_json: Value = serde_json::from_slice(&out_code.stdout).unwrap();
    assert_eq!(code_json["primary_action"], "code-view");
    let actions = code_json["actions"].as_array().unwrap();
    assert!(
        actions
            .iter()
            .any(|a| a["id"] == "code-view" && a["key"] == "o")
    );
    assert!(
        actions
            .iter()
            .any(|a| a["id"] == "open-web" && a["key"] == "g")
    );
    assert!(
        actions
            .iter()
            .any(|a| a["id"] == "open-vscode" && a["key"] == "v")
    );
    assert!(
        actions
            .iter()
            .any(|a| a["id"] == "open-idea" && a["key"] == "i")
    );
    assert!(actions.iter().any(|a| a["id"] == "copy" && a["key"] == "y"));
    assert!(
        actions
            .iter()
            .any(|a| a["id"] == "copy-permalink" && a["key"] == "Y")
    );
    assert!(
        actions
            .iter()
            .any(|a| a["id"] == "repin" && a["key"] == "u")
    );
    assert!(
        actions
            .iter()
            .any(|a| a["id"] == "fetch" && a["key"] == "F")
    );
}

#[test]
fn cli_actions_not_found_exits_1() {
    let (_dir, kb_root, home) = setup_actions_test_env();

    let out = kb_cmd(&kb_root, &home)
        .args(["actions", "non-existent-entry"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}
