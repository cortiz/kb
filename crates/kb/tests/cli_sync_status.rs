//! `kb sync status`: local, offline, and JSON.

mod support;

use std::fs;
use support::git_sync::{entry, git, two_clones};
use support::kb_in;

#[test]
fn json_reports_ahead_uncommitted_and_never_touches_the_network() {
    let (_d, remote, a, _b) = two_clones();
    fs::write(a.join("one.md"), entry("One", "echo 1")).unwrap();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "local"]);
    fs::write(a.join("two.md"), entry("Two", "echo 2")).unwrap();
    // an unreachable remote: status must not care
    git(
        &a,
        &["remote", "set-url", "origin", "/nonexistent/remote.git"],
    );

    let out = kb_in(&a)
        .args(["sync", "status", "--json"])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["is_git"], true);
    assert_eq!(v["branch"], "main");
    assert_eq!(v["upstream"], "origin/main");
    assert_eq!(v["ahead"], 1);
    assert_eq!(v["behind"], 0);
    assert_eq!(v["uncommitted"][0]["id"], "two");
    assert_eq!(v["uncommitted"][0]["status"], "added");
    assert!(v["in_progress"].is_null());
    drop(remote);
}

#[test]
fn fetch_flag_sees_remote_commits() {
    let (_d, _remote, a, b) = two_clones();
    fs::write(b.join("new.md"), entry("New", "echo n")).unwrap();
    kb_in(&b).arg("sync").assert().success();

    let v = |extra: &[&str]| -> serde_json::Value {
        let mut args = vec!["sync", "status", "--json"];
        args.extend_from_slice(extra);
        serde_json::from_slice(&kb_in(&a).args(args).assert().success().get_output().stdout)
            .unwrap()
    };
    assert_eq!(v(&[])["behind"], 0, "not fetched yet");
    let fetched = v(&["--fetch"]);
    assert_eq!(fetched["behind"], 1);
    assert!(fetched["last_fetch"].is_u64());
}

#[test]
fn outside_git_it_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let out = kb_in(dir.path())
        .args(["sync", "status"])
        .assert()
        .success();
    assert!(String::from_utf8_lossy(&out.get_output().stdout).contains("not a git repository"));
}
