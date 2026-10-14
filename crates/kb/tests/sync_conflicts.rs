//! Conflict resolution and rollback in `kb sync`, driven by answers piped to stdin.

mod support;

use std::fs;
use support::git_sync::{entry, git, two_clones};
use support::kb_in;

/// Both clones edit `base.md`; `b` pushes first, so `a` conflicts when it syncs.
fn conflicted() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let (d, _remote, a, b) = two_clones();
    fs::write(a.join("base.md"), entry("Base", "echo from-a")).unwrap();
    fs::write(b.join("base.md"), entry("Base", "echo from-b")).unwrap();
    kb_in(&b).arg("sync").assert().success();
    (d, a, b)
}

#[test]
fn mine_keeps_local_and_theirs_keeps_remote() {
    let (_d, a, _b) = conflicted();
    kb_in(&a).arg("sync").write_stdin("m\n").assert().success();
    assert!(
        fs::read_to_string(a.join("base.md"))
            .unwrap()
            .contains("from-a")
    );
    assert_eq!(git(&a, &["status", "--porcelain"]), "");
    assert_eq!(git(&a, &["rev-list", "--merges", "HEAD"]), "");

    let (_d, a, _b) = conflicted();
    kb_in(&a).arg("sync").write_stdin("t\n").assert().success();
    assert!(
        fs::read_to_string(a.join("base.md"))
            .unwrap()
            .contains("from-b")
    );
}

#[test]
fn keep_both_saves_remote_under_a_conflict_name() {
    let (_d, a, _b) = conflicted();
    kb_in(&a).arg("sync").write_stdin("b\n").assert().success();
    assert!(
        fs::read_to_string(a.join("base.md"))
            .unwrap()
            .contains("from-a")
    );
    let theirs = fs::read_dir(&a)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with("base-conflict-"))
        .expect("a conflict copy");
    assert!(
        fs::read_to_string(a.join(theirs))
            .unwrap()
            .contains("from-b")
    );
}

#[test]
fn quitting_restores_the_repository() {
    let (_d, a, _b) = conflicted();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "local edit"]);
    let head = git(&a, &["rev-parse", "HEAD"]);
    let before = fs::read_to_string(a.join("base.md")).unwrap();
    kb_in(&a).arg("sync").write_stdin("q\n").assert().code(130);
    assert_eq!(git(&a, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read_to_string(a.join("base.md")).unwrap(), before);
    assert_eq!(git(&a, &["status", "--porcelain"]), "");
    assert!(!a.join(".kb/sync-state.json").exists());
}

#[test]
fn without_answers_the_conflict_waits_for_continue_or_abort() {
    let (_d, a, _b) = conflicted();
    kb_in(&a).arg("sync").assert().code(1);
    assert!(a.join(".kb/sync-state.json").exists());
    kb_in(&a).arg("sync").assert().code(1); // refuses while one is pending

    kb_in(&a)
        .args(["sync", "--continue"])
        .write_stdin("t\n")
        .assert()
        .success();
    assert!(
        fs::read_to_string(a.join("base.md"))
            .unwrap()
            .contains("from-b")
    );
    assert!(!a.join(".kb/sync-state.json").exists());
}

#[test]
fn abort_after_a_deferred_conflict_rolls_back() {
    let (_d, a, _b) = conflicted();
    kb_in(&a).arg("sync").assert().code(1);
    kb_in(&a).args(["sync", "--abort"]).assert().success();
    assert!(
        fs::read_to_string(a.join("base.md"))
            .unwrap()
            .contains("from-a")
    );
    assert_eq!(git(&a, &["status", "--porcelain"]), "");
    kb_in(&a).args(["sync", "--abort"]).assert().code(1);
}

#[test]
fn other_commands_point_at_the_pending_sync() {
    let (_d, a, _b) = conflicted();
    kb_in(&a).arg("sync").assert().code(1);
    let out = kb_in(&a).args(["lint", "--quiet"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("kb sync --continue"));
}
