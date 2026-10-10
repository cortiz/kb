//! PTY integration tests for TUI Screen 3 (Code View).

mod support;

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use support::path_with;
use support::pty::{CTRL_C, ESC, Session, Spec};

const CTRL_O: &[u8] = b"\x0f";
const ENTER: &[u8] = b"\r";

fn setup_code_pty_env() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, PathBuf, PathBuf, Value) {
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
"#
        , git_repo.display()),
    ).unwrap();

    let (opener_dir, opened_log) = fake_opener(dir.path());

    (dir, git_repo, kb_root, home, opener_dir, opened_log, manifest)
}

fn fake_opener(base: &Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = base.join("fake-opener-bin");
    fs::create_dir_all(&dir).unwrap();
    let opened = base.join("opened.log");
    for name in &["open", "xdg-open"] {
        let script = dir.join(name);
        fs::write(
            &script,
            format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 0\n", opened.display()),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    (dir, opened)
}

#[test]
fn tui_code_view_open_navigate_and_keys() {
    let (_dir, _git_repo, kb_root, home, opener_dir, opened_log, manifest) = setup_code_pty_env();
    let pinned_commit = manifest["pinned_commit"].as_str().unwrap();

    // Create a moved code entry
    fs::write(
        kb_root.join("code-auth.md"),
        format!(
            r#"---
kb_format: 1
type: code
title: Auth Handler Code
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

    let mut spec = Spec::new(&[], &home, &kb_root);
    spec.env.push(("PATH".into(), path_with(&opener_dir).into_string().unwrap()));
    let mut s = Session::spawn(&spec);

    // 1. In list view, select the entry
    s.wait_for("Auth Handler Code");

    // 2. Press Ctrl-O to open Code View (Screen 3)
    s.send(CTRL_O);

    // Verify Code View header, status badge, code lines, and keybar
    s.wait_for("Auth Handler Code (code)");
    s.wait_for("Repo: test/repo");
    s.wait_for("Path: src/AuthHandler.java:8");
    s.wait_for("Status:");
    s.wait_for("[Enter] open");
    s.wait_for("[Esc] back");

    // 3. Test Esc returns to main list
    s.send(ESC);
    s.wait_for("Auth Handler Code");

    // Re-open Code View with Ctrl-O
    s.send(CTRL_O);
    s.wait_for("Auth Handler Code (code)");

    // 4. Test 'y' copies path:line
    s.send(b"y");
    s.wait_for("Copied location");

    // 5. Test 'Y' copies permalink
    s.send(b"Y");
    s.wait_for("Copied permalink");

    // 6. Test 'g' opens web permalink
    s.send(b"g");
    s.wait_for("Opened in browser");
    let opened = fs::read_to_string(&opened_log).unwrap_or_default();
    assert!(
        opened.contains("github.com/test/repo/blob/"),
        "expected web permalink in opened.log: {opened}"
    );

    // 7. Test 'v' opens VS Code
    s.send(b"v");
    s.wait_for("Opened in VS Code");
    let opened = fs::read_to_string(&opened_log).unwrap_or_default();
    assert!(
        opened.contains("vscode://file/"),
        "expected vscode URL in opened.log: {opened}"
    );

    // 8. Test 'i' opens IntelliJ
    s.send(b"i");
    s.wait_for("Opened in IntelliJ");
    let opened = fs::read_to_string(&opened_log).unwrap_or_default();
    assert!(
        opened.contains("idea://open?file="),
        "expected idea URL in opened.log: {opened}"
    );

    // 9. Test 'Enter' opens default target (web)
    s.send(ENTER);
    s.wait_for("Opened in browser");

    // 10. Test 'u' re-pins the moved entry
    s.send(b"u");
    // If confirmation is requested, confirm with y
    s.send(b"y");
    s.wait_for("Re-pinned");

    // 11. Test 'F' fetches and re-checks
    s.send(b"F");
    s.wait_for("Fetched");

    // Exit cleanly
    s.send(ESC);
    s.wait_for("Auth Handler Code");
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn tui_code_view_missing_clone_state() {
    let dir = tempfile::tempdir().unwrap();
    let kb_root = dir.path().join("kb");
    let home = dir.path().join("home");
    fs::create_dir_all(&kb_root).unwrap();
    let config_dir = home.join(".config/kb");
    fs::create_dir_all(&config_dir).unwrap();

    let (opener_dir, opened_log) = fake_opener(dir.path());

    // Entry with unknown repo (no clone configured)
    fs::write(
        kb_root.join("code-remote.md"),
        r#"---
kb_format: 1
type: code
title: Remote GitHub Link
code:
  repo: acme/remote-repo
  path: src/Main.java
  line: 42
  ref: a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2
---
"#,
    )
    .unwrap();

    let mut spec = Spec::new(&[], &home, &kb_root);
    spec.env.push(("PATH".into(), path_with(&opener_dir).into_string().unwrap()));
    let mut s = Session::spawn(&spec);

    s.wait_for("Remote GitHub Link");
    s.send(CTRL_O);

    // Code view renders missing clone notice
    s.wait_for("Remote GitHub Link (code)");
    s.wait_for("No local clone found for repo 'acme/remote-repo'");
    s.wait_for("Configure [code.repos]");

    // 'v' and 'i' are disabled
    s.send(b"v");
    s.wait_for("No local clone found");
    s.send(b"i");
    s.wait_for("No local clone found");

    // 'g' still opens browser
    s.send(b"g");
    s.wait_for("Opened in browser");
    let opened = fs::read_to_string(&opened_log).unwrap_or_default();
    assert!(
        opened.contains("github.com/acme/remote-repo/blob/"),
        "expected web permalink in opened.log: {opened}"
    );

    // 'y' still copies path:line
    s.send(b"y");
    s.wait_for("Copied location");

    s.send(ESC);
    s.wait_for("Remote GitHub Link");
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

