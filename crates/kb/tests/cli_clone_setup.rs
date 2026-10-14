//! `kb sync setup`, `kb clone` and `kb init --git`.

mod support;

use std::fs;
use support::git_sync::{entry, git};
use support::kb_in;

fn bare(dir: &std::path::Path) -> std::path::PathBuf {
    let remote = dir.join("remote.git");
    git(
        dir,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    remote
}

fn identity(repo: &std::path::Path) {
    git(repo, &["config", "user.name", "T"]);
    git(repo, &["config", "user.email", "t@example.com"]);
}

#[test]
fn setup_then_clone_gives_a_second_machine_the_same_entries() {
    let dir = tempfile::tempdir().unwrap();
    let remote = bare(dir.path());
    let url = remote.to_str().unwrap();

    let a = dir.path().join("a");
    fs::create_dir_all(a.join("cw")).unwrap();
    fs::write(a.join("cw/one.md"), entry("One", "echo 1")).unwrap();
    git(&a, &["init", "-q", "-b", "main"]);
    identity(&a);
    kb_in(&a).args(["sync", "setup", url]).assert().success();
    assert!(
        fs::read_to_string(a.join(".gitignore"))
            .unwrap()
            .contains(".kb/")
    );
    assert_eq!(
        git(&a, &["log", "--format=%s"]).trim(),
        "kb: initial import"
    );
    assert_eq!(
        git(&a, &["rev-parse", "main"]),
        git(&remote, &["rev-parse", "main"])
    );
    kb_in(&a).args(["sync", "setup", url]).assert().success(); // idempotent

    let b = dir.path().join("b");
    kb_in(&b)
        .args(["clone", url, b.to_str().unwrap()])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(b.join("cw/one.md")).unwrap(),
        fs::read_to_string(a.join("cw/one.md")).unwrap()
    );
    assert!(b.join(".kb/index.db").exists(), "index built");
}

#[test]
fn clone_of_an_empty_remote_says_how_to_fix_it() {
    let dir = tempfile::tempdir().unwrap();
    let remote = bare(dir.path());
    let b = dir.path().join("b");
    let out = kb_in(&b)
        .args(["clone", remote.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .code(1);
    assert!(String::from_utf8_lossy(&out.get_output().stderr).contains("kb sync setup"));
}

#[test]
fn setup_refuses_to_commit_a_secret() {
    let dir = tempfile::tempdir().unwrap();
    let remote = bare(dir.path());
    let a = dir.path().join("a");
    fs::create_dir_all(&a).unwrap();
    fs::write(a.join("bad.md"), entry("Bad", "echo AKIAIOSFODNN7EXAMPLE")).unwrap();
    git(&a, &["init", "-q", "-b", "main"]);
    identity(&a);
    kb_in(&a)
        .args(["sync", "setup", remote.to_str().unwrap()])
        .assert()
        .code(1);
    assert_eq!(
        git(&a, &["rev-list", "--all", "--count"]).trim(),
        "0",
        "nothing committed"
    );
}

#[test]
fn init_git_makes_the_first_commit() {
    let dir = tempfile::tempdir().unwrap();
    let kb = dir.path().join("kb");
    let mut cmd = kb_in(&kb);
    cmd.env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .args(["init", "--git"])
        .assert()
        .success();
    assert_eq!(
        git(&kb, &["log", "--format=%s"]).trim(),
        "kb: initial import"
    );
    assert_eq!(git(&kb, &["status", "--porcelain"]), "");
}
