//! Integration tests for `kb review`: interactive and batch review of agent changes.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use kb_core::mcp::review::ReviewStateTracker;
use support::kb_in;
use support::pty::{Session, Spec};

fn setup_git_repo(root: &Path) {
    let _ = Command::new("git").args(["init"]).current_dir(root).output();
    let _ = Command::new("git").args(["config", "user.name", "Test User"]).current_dir(root).output();
    let _ = Command::new("git").args(["config", "user.email", "test@example.com"]).current_dir(root).output();
}

fn setup_review_kb() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let root = dir.path().join("kb");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&root).unwrap();
    setup_git_repo(&root);

    (dir, home, root)
}

#[test]
fn test_review_no_unreviewed_entries() {
    let (_dir, _home, root) = setup_review_kb();

    let assert = kb_in(&root)
        .arg("review")
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("No unreviewed entries."));
}

#[test]
fn test_review_json_output() {
    let (_dir, _home, root) = setup_review_kb();

    let entry_path = root.join("tail.md");
    fs::write(
        &entry_path,
        "---\ntitle: Tail Logs\ntype: cmd\nshell: sh\n---\nkubectl logs -f pod\n",
    )
    .unwrap();

    let _ = Command::new("git").args(["add", "tail.md"]).current_dir(&root).output();
    let _ = Command::new("git")
        .args(["commit", "-m", "kb: create tail\n\nKb-Source: mcp\nKb-Client: claude\n"])
        .current_dir(&root)
        .output();

    ReviewStateTracker::mark_unreviewed(&root, "tail").unwrap();

    let assert = kb_in(&root)
        .args(["review", "--json"])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let json_val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json_val.is_array());
    let arr = json_val.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], "tail");
    assert_eq!(arr[0]["unreviewed"], true);
    assert!(arr[0]["diff"].as_str().unwrap().contains("kubectl logs"));
}

#[test]
fn test_review_approve_all() {
    let (_dir, _home, root) = setup_review_kb();

    let entry_path = root.join("tail.md");
    fs::write(
        &entry_path,
        "---\ntitle: Tail Logs\ntype: cmd\nshell: sh\n---\nkubectl logs -f pod\n",
    )
    .unwrap();

    let _ = Command::new("git").args(["add", "tail.md"]).current_dir(&root).output();
    let _ = Command::new("git")
        .args(["commit", "-m", "kb: create tail\n\nKb-Source: mcp\nKb-Client: claude\n"])
        .current_dir(&root)
        .output();

    ReviewStateTracker::mark_unreviewed(&root, "tail").unwrap();
    assert!(ReviewStateTracker::is_unreviewed(&root, "tail"));

    let assert = kb_in(&root)
        .args(["review", "--approve-all"])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Approved tail"));
    assert!(!ReviewStateTracker::is_unreviewed(&root, "tail"));
}

#[test]
fn test_review_pty_approve_choice() {
    let (_dir, home, root) = setup_review_kb();

    let entry_path = root.join("tail.md");
    fs::write(
        &entry_path,
        "---\ntitle: Tail Logs\ntype: cmd\nshell: sh\n---\nkubectl logs -f pod\n",
    )
    .unwrap();

    let _ = Command::new("git").args(["add", "tail.md"]).current_dir(&root).output();
    let _ = Command::new("git")
        .args(["commit", "-m", "kb: create tail\n\nKb-Source: mcp\nKb-Client: claude\n"])
        .current_dir(&root)
        .output();

    ReviewStateTracker::mark_unreviewed(&root, "tail").unwrap();

    let spec = Spec::new(&["review"], &home, &root);
    let mut s = Session::spawn(&spec);

    s.wait_for("Reviewing: tail");
    s.wait_for("Review choice:");

    s.send(b"a\n");
    s.wait_for("Approved tail");
    assert_eq!(s.wait_exit(), 0);

    assert!(!ReviewStateTracker::is_unreviewed(&root, "tail"));
}

#[test]
fn test_review_pty_skip_choice() {
    let (_dir, home, root) = setup_review_kb();

    let entry_path = root.join("tail.md");
    fs::write(
        &entry_path,
        "---\ntitle: Tail Logs\ntype: cmd\nshell: sh\n---\nkubectl logs -f pod\n",
    )
    .unwrap();

    let _ = Command::new("git").args(["add", "tail.md"]).current_dir(&root).output();
    let _ = Command::new("git")
        .args(["commit", "-m", "kb: create tail\n\nKb-Source: mcp\nKb-Client: claude\n"])
        .current_dir(&root)
        .output();

    ReviewStateTracker::mark_unreviewed(&root, "tail").unwrap();

    let spec = Spec::new(&["review"], &home, &root);
    let mut s = Session::spawn(&spec);

    s.wait_for("Reviewing: tail");
    s.wait_for("Review choice:");

    s.send(b"s\n");
    s.wait_for("Skipped tail");
    assert_eq!(s.wait_exit(), 0);

    // Entry should still be unreviewed
    assert!(ReviewStateTracker::is_unreviewed(&root, "tail"));
}

#[test]
fn test_review_pty_revert_choice() {
    let (_dir, home, root) = setup_review_kb();

    // 1. Initial human commit
    let entry_path = root.join("tail.md");
    fs::write(
        &entry_path,
        "---\ntitle: Human Tail\ntype: cmd\nshell: sh\n---\necho human\n",
    )
    .unwrap();
    let _ = Command::new("git").args(["add", "tail.md"]).current_dir(&root).output();
    let _ = Command::new("git").args(["commit", "-m", "initial human commit"]).current_dir(&root).output();

    // 2. Agent update commit
    fs::write(
        &entry_path,
        "---\ntitle: Agent Tail\ntype: cmd\nshell: sh\n---\necho agent\n",
    )
    .unwrap();
    let _ = Command::new("git").args(["add", "tail.md"]).current_dir(&root).output();
    let _ = Command::new("git")
        .args(["commit", "-m", "kb: update tail\n\nKb-Source: mcp\nKb-Client: claude\n"])
        .current_dir(&root)
        .output();

    ReviewStateTracker::mark_unreviewed(&root, "tail").unwrap();

    let spec = Spec::new(&["review"], &home, &root);
    let mut s = Session::spawn(&spec);

    s.wait_for("Reviewing: tail");
    s.wait_for("Review choice:");

    s.send(b"r\n");
    s.wait_for("Reverted tail");
    assert_eq!(s.wait_exit(), 0);

    assert!(!ReviewStateTracker::is_unreviewed(&root, "tail"));

    // File content should have reverted to echo human
    let reverted_content = fs::read_to_string(&entry_path).unwrap();
    assert!(reverted_content.contains("echo human"));
}
