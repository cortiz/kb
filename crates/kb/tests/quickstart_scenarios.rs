//! The scenarios of `specs/009-git-sync/quickstart.md`, end to end through the `kb` binary.

mod support;

use std::fs;
use support::git_sync::{entry, git};
use support::kb_in;

fn identity(repo: &std::path::Path) {
    git(repo, &["config", "user.name", "Test User"]);
    git(repo, &["config", "user.email", "test@example.com"]);
}

fn add(root: &std::path::Path, id: &str, ty: &str, title: &str, body: &str, tags: &str) {
    kb_in(root)
        .args([
            "add", "--path", id, "--type", ty, "--title", title, "--tag", tags, "--stdin", "--yes",
        ])
        .write_stdin(body)
        .assert()
        .success();
}

/// Scenarios 1 and 2: isolated auto-commits, then two machines syncing through a bare remote.
#[test]
fn auto_commits_stay_isolated_and_two_machines_sync_linearly() {
    let dir = tempfile::tempdir().unwrap();
    let (one, two, bare) = (
        dir.path().join("one"),
        dir.path().join("two"),
        dir.path().join("bare.git"),
    );
    fs::create_dir_all(&one).unwrap();
    git(&one, &["init", "-q", "-b", "main"]);
    identity(&one);
    fs::write(one.join("README.md"), "# My Notes\n").unwrap();
    git(&one, &["add", "README.md"]);
    git(&one, &["commit", "-qm", "initial commit"]);
    fs::write(one.join("scratch.txt"), "staged scratch note\n").unwrap();
    git(&one, &["add", "scratch.txt"]);

    add(
        &one,
        "cw/latency",
        "cw",
        "Latency P99",
        "fields @timestamp, latency | filter latency > 100\n",
        "aws,p99",
    );
    assert_eq!(
        git(&one, &["log", "-1", "--format=%s"]).trim(),
        "kb: add cw/latency"
    );
    assert_eq!(
        git(&one, &["status", "--porcelain"]).trim(),
        "A  scratch.txt"
    );

    // scenario 2: the staged scratch file is not an entry, so commit it before syncing
    git(&one, &["commit", "-qm", "scratch"]);
    git(
        dir.path(),
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&one, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&one, &["push", "-q", "-u", "origin", "main"]);
    kb_in(&two)
        .args(["clone", bare.to_str().unwrap(), two.to_str().unwrap()])
        .assert()
        .success();
    identity(&two);

    add(
        &one,
        "sumo/5xx",
        "sumo",
        "5xx errors",
        "_sourceCategory=prod | count\n",
        "prod",
    );
    kb_in(&one).arg("sync").assert().success();
    add(
        &two,
        "sql/users",
        "sql",
        "User count",
        "SELECT count(*) FROM users\n",
        "db",
    );
    let out = kb_in(&two).arg("sync").assert().success();
    let text = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
    assert!(text.contains("in sync with origin/main"), "{text}");
    assert!(two.join("sumo/5xx.md").exists());
    kb_in(&one).arg("sync").assert().success();
    assert!(one.join("sql/users.md").exists());
    assert_eq!(git(&one, &["rev-list", "--merges", "HEAD"]), "");
    assert_eq!(
        git(&one, &["rev-parse", "HEAD"]),
        git(&two, &["rev-parse", "HEAD"])
    );
}

/// Scenario 4: a secret committed by hand and then deleted still stops the push.
#[test]
fn a_secret_in_unpushed_history_stops_the_push() {
    let (_d, remote, a, _b) = support::git_sync::two_clones();
    fs::write(
        a.join("leak.md"),
        entry("Leak", "echo AKIAIOSFODNN7EXAMPLE"),
    )
    .unwrap();
    git(&a, &["add", "leak.md"]);
    git(&a, &["commit", "-qm", "oops"]);
    git(&a, &["rm", "-q", "leak.md"]);
    git(&a, &["commit", "-qm", "remove it again"]);

    let out = kb_in(&a).arg("sync").assert().code(1);
    let err = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
    assert!(
        err.contains("leak.md") && err.contains("rewrite local history"),
        "{err}"
    );
    assert_ne!(
        git(&a, &["rev-parse", "HEAD"]),
        git(&remote, &["rev-parse", "main"]),
        "nothing was pushed"
    );
}
