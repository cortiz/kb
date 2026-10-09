//! `kb vars`: an entry's variables and what would be pre-filled.

mod support;

use serde_json::Value;
use support::{kb_in, vars_kb};

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

fn vars(root: &std::path::Path, args: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = kb_in(root);
    cmd.arg("vars").args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

#[test]
fn lists_kind_options_default_and_what_would_be_prefilled() {
    let (_d, root) = vars_kb();
    let out = vars(&root, &["sumo/5xx-by-authorizer"], &[]);
    assert_eq!(out.status.code(), Some(0));
    let t = text(&out.stdout);
    assert!(
        t.contains("env") && t.contains("enum: prod|stage|dev") && t.contains("→ (will ask)"),
        "{t}"
    );
    assert!(
        t.contains("minutes") && t.contains("default: 15") && t.contains("→ 15  (default)"),
        "{t}"
    );
}

#[test]
fn shows_the_environment_and_remembered_sources_but_never_a_secret() {
    let (_d, root) = vars_kb();
    let used = kb_in(&root)
        .args(["use", "cmd/curl-token", "--var", "path=v1", "--print"])
        .env("KB_TEST_TOKEN", "S3CR3T-MARKER")
        .output()
        .unwrap();
    assert_eq!(used.status.code(), Some(0), "{}", text(&used.stderr));
    let out = vars(
        &root,
        &["cmd/curl-token"],
        &[("KB_TEST_TOKEN", "S3CR3T-MARKER")],
    );
    let t = text(&out.stdout);
    assert!(t.contains("env: KB_TEST_TOKEN (set)"), "{t}");
    assert!(t.contains("→ ••••  (from $KB_TEST_TOKEN)"), "{t}");
    assert!(
        t.contains("remembered: v1") && t.contains("→ v1  (remembered)"),
        "{t}"
    );
    assert!(!t.contains("S3CR3T"));
    let out = vars(
        &root,
        &["cmd/curl-token", "--json"],
        &[("KB_TEST_TOKEN", "S3CR3T-MARKER")],
    );
    assert!(!text(&out.stdout).contains("S3CR3T"));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["id"], "cmd/curl-token");
    let token = &doc["variables"][0];
    assert_eq!(token["name"], "token");
    assert_eq!(token["env"]["set"], true);
    assert_eq!(token["prefill"]["value"], Value::Null);
    assert_eq!(token["remembered"], Value::Null);
    let path = &doc["variables"][1];
    assert_eq!(path["prefill"]["source"], "remembered");
    assert_eq!(path["prefill"]["value"], "v1");
}

#[test]
fn marks_undeclared_escape_and_unusable_defaults() {
    let (_d, root) = vars_kb();
    let t = text(&vars(&root, &["cmd/undeclared"], &[]).stdout);
    assert!(t.contains("undeclared"), "{t}");
    let t = text(&vars(&root, &["cmd/escape-none"], &[]).stdout);
    assert!(t.contains("escape: none"), "{t}");
    let t = text(&vars(&root, &["cmd/bad-default"], &[]).stdout);
    assert!(
        t.contains("default: 1e3 (invalid, ignored)") && t.contains("→ (will ask)"),
        "{t}"
    );
}

#[test]
fn an_entry_without_variables_says_so_and_nothing_is_recorded() {
    let (_d, root) = vars_kb();
    let out = vars(&root, &["cmd/no-vars"], &[]);
    assert_eq!(text(&out.stdout), "no variables\n");
    let out = vars(&root, &["sumo/5xx-by-authorizer"], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!root.join(".kb/state.json").exists());
}

#[test]
fn not_found_is_exit_1() {
    let (_d, root) = vars_kb();
    assert_eq!(vars(&root, &["zzzz-nothing"], &[]).status.code(), Some(1));
}
