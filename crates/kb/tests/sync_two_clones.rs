//! `kb sync` across two clones that share a bare remote.

mod support;

use std::fs;
use support::git_sync::{entry, git, two_clones};
use support::kb_in;

#[test]
fn both_clones_end_identical_with_linear_history() {
    let (_d, _remote, a, b) = two_clones();
    fs::write(a.join("from-a.md"), entry("From A", "echo a")).unwrap();
    fs::write(b.join("from-b.md"), entry("From B", "echo b")).unwrap();

    kb_in(&a).arg("sync").assert().success();
    kb_in(&b).arg("sync").assert().success();
    kb_in(&a).arg("sync").assert().success();

    for f in ["base.md", "from-a.md", "from-b.md"] {
        assert_eq!(
            fs::read_to_string(a.join(f)).unwrap(),
            fs::read_to_string(b.join(f)).unwrap()
        );
    }
    assert_eq!(
        git(&a, &["rev-parse", "HEAD"]),
        git(&b, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&a, &["rev-list", "--merges", "HEAD"]),
        "",
        "history is linear"
    );
    assert!(git(&a, &["log", "--format=%s"]).contains("kb: sync 1 entries"));
}

#[test]
fn dry_run_changes_nothing_and_no_push_keeps_commits_local() {
    let (_d, remote, a, b) = two_clones();
    fs::write(a.join("one.md"), entry("One", "echo 1")).unwrap();
    let head = git(&a, &["rev-parse", "HEAD"]);

    let out = kb_in(&a).args(["sync", "--dry-run"]).assert().success();
    assert!(String::from_utf8_lossy(&out.get_output().stdout).contains("would commit 1"));
    assert_eq!(git(&a, &["rev-parse", "HEAD"]), head);
    assert!(git(&a, &["status", "--porcelain"]).contains("one.md"));

    kb_in(&a).args(["sync", "--no-push"]).assert().success();
    assert_ne!(git(&a, &["rev-parse", "HEAD"]), head, "committed locally");
    assert_eq!(git(&remote, &["rev-parse", "main"]), head, "not pushed");
    drop(b);
}

#[test]
fn unmanaged_dirty_file_blocks_sync_with_exit_1() {
    let (_d, _remote, a, _b) = two_clones();
    fs::write(a.join("README.md"), "readme\n").unwrap();
    git(&a, &["add", "README.md"]);
    git(&a, &["commit", "-qm", "readme"]);
    fs::write(a.join("README.md"), "edited\n").unwrap();
    fs::write(a.join("one.md"), entry("One", "echo 1")).unwrap();
    kb_in(&a).arg("sync").assert().code(1);
    assert!(
        git(&a, &["status", "--porcelain"]).contains("?? one.md"),
        "nothing was committed"
    );
}

#[test]
fn secrets_block_the_sync() {
    let (_d, _remote, a, _b) = two_clones();
    fs::write(a.join("bad.md"), entry("Bad", "echo AKIAIOSFODNN7EXAMPLE")).unwrap();
    kb_in(&a).arg("sync").assert().code(1);
    assert!(git(&a, &["status", "--porcelain"]).contains("bad.md"));
}
