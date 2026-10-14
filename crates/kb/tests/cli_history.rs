//! `kb history` and `kb restore` against a real git history, renames included.

mod support;

use std::fs;
use support::git_sync::{entry, git};
use support::kb_in;

/// An entry added, edited, then renamed, in a git repo.
fn repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "T"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    fs::write(root.join("old.md"), entry("Thing", "echo one")).unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "kb: add old"]);
    fs::write(root.join("old.md"), entry("Thing", "echo two")).unwrap();
    git(&root, &["commit", "-qam", "kb: edit old"]);
    git(&root, &["mv", "old.md", "new.md"]);
    git(&root, &["commit", "-qm", "kb: mv old new"]);
    (dir, root)
}

#[test]
fn history_follows_the_rename_and_names_what_changed() {
    let (_d, root) = repo();
    let out = kb_in(&root)
        .args(["history", "new", "--json"])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["id"], "new");
    let commits = v["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 3);
    assert_eq!(commits[0]["message"], "kb: mv old new");
    assert_eq!(commits[0]["change_categories"][0], "renamed");
    assert_eq!(commits[1]["change_categories"][0], "payload");

    let text = kb_in(&root).args(["history", "new"]).assert().success();
    assert!(String::from_utf8_lossy(&text.get_output().stdout).contains("kb: edit old"));
}

#[test]
fn diff_shows_the_change_at_a_revision() {
    let (_d, root) = repo();
    let out = kb_in(&root)
        .args(["history", "new", "--diff", "HEAD~1"])
        .assert()
        .success();
    let text = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
    assert!(
        text.contains("-echo one") && text.contains("+echo two"),
        "{text}"
    );
    kb_in(&root)
        .args(["history", "new", "--diff", "nope"])
        .assert()
        .code(1);
}

#[test]
fn restore_writes_the_old_text_and_commits_it() {
    let (_d, root) = repo();
    kb_in(&root)
        .args(["restore", "new", "--at", "HEAD~2"])
        .assert()
        .success();
    assert!(
        fs::read_to_string(root.join("new.md"))
            .unwrap()
            .contains("echo one")
    );
    let subject = git(&root, &["log", "-1", "--format=%s"]);
    assert!(subject.starts_with("kb: restore new from "), "{subject}");
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
}

#[test]
fn restore_refuses_a_revision_that_leaks_a_secret() {
    let (_d, root) = repo();
    fs::write(
        root.join("new.md"),
        entry("Thing", "echo AKIAIOSFODNN7EXAMPLE"),
    )
    .unwrap();
    git(&root, &["commit", "-qam", "oops"]);
    fs::write(root.join("new.md"), entry("Thing", "echo three")).unwrap();
    git(&root, &["commit", "-qam", "fixed"]);
    kb_in(&root)
        .args(["restore", "new", "--at", "HEAD~1"])
        .assert()
        .code(1);
    assert!(
        fs::read_to_string(root.join("new.md"))
            .unwrap()
            .contains("echo three")
    );
}
