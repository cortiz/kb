//! Integration tests for `kb code check` and `kb code repin`.

mod support;

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use support::kb_in;

fn setup_code_test_env() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, Value) {
    let dir = tempfile::tempdir().unwrap();
    let git_repo = dir.path().join("repo");
    let kb_root = dir.path().join("kb");
    let home = dir.path().join("home");
    fs::create_dir_all(&kb_root).unwrap();
    let config_dir = home.join(".config/kb");
    fs::create_dir_all(&config_dir).unwrap();

    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/git_repos/scripted_repo.sh");

    let output = std::process::Command::new("bash")
        .arg(script)
        .arg(&git_repo)
        .output()
        .expect("scripted_repo.sh run");
    assert!(
        output.status.success(),
        "script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest_bytes = fs::read(git_repo.join("repo_manifest.json")).expect("manifest");
    let manifest: Value = serde_json::from_slice(&manifest_bytes).expect("json");

    fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"[code.repos]
"test/repo" = "{}"
"#,
            git_repo.display()
        ),
    )
    .unwrap();

    (dir, git_repo, kb_root, home, manifest)
}

fn kb_cmd(kb_root: &Path, home: &Path) -> assert_cmd::Command {
    let mut cmd = kb_in(kb_root);
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"));
    cmd
}

#[test]
fn cli_code_check_json_and_exit_codes() {
    let (_dir, _git_repo, kb_root, home, manifest) = setup_code_test_env();
    let pinned_commit = manifest["pinned_commit"].as_str().unwrap();

    // 1. Current entry
    fs::write(
        kb_root.join("code-current.md"),
        format!(
            r#"---
kb_format: 1
type: code
title: Auth Current
code:
  repo: test/repo
  path: src/AuthHandler.java
  line: 8
  ref: {pinned_commit}
  branch: main
---
"#
        ),
    )
    .unwrap();

    let out = kb_cmd(&kb_root, &home)
        .args(["code", "check", "code-current", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "check current should exit 0");
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json[0]["status"], "current");

    // 2. Moved entry (clean, should exit 0)
    fs::write(
        kb_root.join("code-moved.md"),
        format!(
            r#"---
kb_format: 1
type: code
title: Auth Moved
code:
  repo: test/repo
  path: src/AuthHandler.java
  line: 8
  ref: {pinned_commit}
  branch: branch-moved
---
"#
        ),
    )
    .unwrap();

    let out_moved = kb_cmd(&kb_root, &home)
        .args(["code", "check", "code-moved", "--json"])
        .output()
        .unwrap();
    assert!(out_moved.status.success(), "check moved should exit 0");
    let json_moved: Value = serde_json::from_slice(&out_moved.stdout).unwrap();
    assert_eq!(json_moved[0]["status"], "moved");
    assert_eq!(json_moved[0]["proposed_line"], 13);

    // 3. Changed entry (exit 1)
    fs::write(
        kb_root.join("code-changed.md"),
        format!(
            r#"---
kb_format: 1
type: code
title: Auth Changed
code:
  repo: test/repo
  path: src/AuthHandler.java
  line: 8
  end_line: 13
  ref: {pinned_commit}
  branch: branch-changed
---
"#
        ),
    )
    .unwrap();

    let out_changed = kb_cmd(&kb_root, &home)
        .args(["code", "check", "code-changed", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        out_changed.status.code(),
        Some(1),
        "check changed should exit 1"
    );
    let json_changed: Value = serde_json::from_slice(&out_changed.stdout).unwrap();
    assert_eq!(json_changed[0]["status"], "changed");
}

#[test]
fn cli_code_repin_updates_entry_format_preserving() {
    let (_dir, _git_repo, kb_root, home, manifest) = setup_code_test_env();
    let pinned_commit = manifest["pinned_commit"].as_str().unwrap();
    let moved_commit = manifest["moved_commit"].as_str().unwrap();

    let initial = format!(
        r#"---
kb_format: 1
type: code
# Important note about authentication
title: Auth Handler Link
code:
  repo: test/repo
  path: src/AuthHandler.java
  line: 8
  ref: {pinned_commit}
  branch: branch-moved
---

Preserved documentation body.
"#
    );

    fs::write(kb_root.join("code-auth.md"), &initial).unwrap();

    let out = kb_cmd(&kb_root, &home)
        .args(["code", "repin", "code-auth", "-y"])
        .output()
        .unwrap();
    assert!(out.status.success(), "repin should exit 0");

    let updated = fs::read_to_string(kb_root.join("code-auth.md")).unwrap();
    assert!(updated.contains(&format!("ref: {moved_commit}")));
    assert!(updated.contains("line: 13"));
    assert!(updated.contains("# Important note about authentication"));
    assert!(updated.contains("Preserved documentation body."));

    // After re-pin, check should report current
    let check_out = kb_cmd(&kb_root, &home)
        .args(["code", "check", "code-auth", "--json"])
        .output()
        .unwrap();
    assert!(check_out.status.success());
    let json: Value = serde_json::from_slice(&check_out.stdout).unwrap();
    assert_eq!(json[0]["status"], "current");
}

#[test]
fn cli_code_repin_all_and_skips_gone() {
    let (_dir, _git_repo, kb_root, home, manifest) = setup_code_test_env();
    let pinned_commit = manifest["pinned_commit"].as_str().unwrap();

    fs::write(
        kb_root.join("code-moved.md"),
        format!(
            r#"---
kb_format: 1
type: code
title: Auth Moved
code:
  repo: test/repo
  path: src/AuthHandler.java
  line: 8
  ref: {pinned_commit}
  branch: branch-moved
---
"#
        ),
    )
    .unwrap();

    fs::write(
        kb_root.join("code-gone.md"),
        format!(
            r#"---
kb_format: 1
type: code
title: Auth Gone
code:
  repo: test/repo
  path: src/AuthHandler.java
  line: 8
  ref: {pinned_commit}
  branch: branch-gone
---
"#
        ),
    )
    .unwrap();

    let out = kb_cmd(&kb_root, &home)
        .args(["code", "repin", "--all", "-y"])
        .output()
        .unwrap();
    assert!(out.status.success(), "repin --all should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("re-pinned") || stdout.contains("code-moved"));
    assert!(stdout.contains("skipped") || stdout.contains("code-gone"));
}

#[test]
fn cli_code_show_text_and_json() {
    let (_dir, _git_repo, kb_root, home, manifest) = setup_code_test_env();
    let pinned_commit = manifest["pinned_commit"].as_str().unwrap();

    fs::write(
        kb_root.join("code-auth.md"),
        format!(
            r#"---
kb_format: 1
type: code
title: Auth Handler Link
code:
  repo: test/repo
  path: src/AuthHandler.java
  line: 8
  ref: {pinned_commit}
  branch: main
---
"#
        ),
    )
    .unwrap();

    // 1. Text mode
    let out_text = kb_cmd(&kb_root, &home)
        .args(["code", "show", "code-auth"])
        .output()
        .unwrap();
    assert!(out_text.status.success(), "code show should exit 0");
    let text = String::from_utf8_lossy(&out_text.stdout);
    assert!(text.contains("Title:  Auth Handler Link"));
    assert!(text.contains("Repo:   test/repo"));
    assert!(text.contains("File:   src/AuthHandler.java:8"));
    assert!(text.contains(&format!("Ref:    {pinned_commit}")));
    assert!(text.contains("Status: current"));
    assert!(text.contains("Link:   https://github.com/test/repo/blob/"));
    assert!(text.contains("▶    8 |     public boolean handleRequest(String token) {"));

    // 2. JSON mode
    let out_json = kb_cmd(&kb_root, &home)
        .args(["code", "show", "code-auth", "--json"])
        .output()
        .unwrap();
    assert!(out_json.status.success(), "code show --json should exit 0");
    let json: Value = serde_json::from_slice(&out_json.stdout).unwrap();
    assert_eq!(json["id"], "code-auth");
    assert_eq!(json["title"], "Auth Handler Link");
    assert_eq!(json["repo"], "test/repo");
    assert_eq!(json["path"], "src/AuthHandler.java");
    assert_eq!(json["line"], 8);
    assert_eq!(json["ref"], pinned_commit);
    assert_eq!(json["status"], "current");
    assert!(
        json["permalink"]
            .as_str()
            .unwrap()
            .contains("https://github.com/test/repo/blob/")
    );
    assert!(json["snippet"]["lines"].is_array());
    assert_eq!(json["snippet"]["target_line"], 8);
}
