//! `kb git install-hooks` / `uninstall-hooks`: the pre-commit hook blocks staged secrets, chains
//! with a user's own hook, and comes out cleanly.

mod support;

use std::fs;
use std::path::Path;
use std::process::Command;
use support::kb_in;

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

fn init(root: &Path) {
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "T"]);
    git(root, &["config", "user.email", "t@example.com"]);
}

const CLEAN: &str = "---\nid: ok\ntitle: Ok\ntype: cmd\n---\n```sh\nls\n```\n";
const SECRET: &str =
    "---\nid: bad\ntitle: Bad\ntype: cmd\n---\n```sh\necho AKIAIOSFODNN7EXAMPLE\n```\n";

#[test]
fn hook_blocks_secret_chains_and_uninstalls() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    init(&root);
    let hook = root.join(".git/hooks/pre-commit");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(&hook, "#!/bin/sh\ntouch user-hook-ran\n").unwrap();

    kb_in(&root)
        .args(["git", "install-hooks"])
        .assert()
        .success();

    fs::write(root.join("ok.md"), CLEAN).unwrap();
    git(&root, &["add", "ok.md"]);
    assert!(git(&root, &["commit", "-qm", "ok"]).status.success());
    assert!(root.join("user-hook-ran").exists(), "user hook still runs");

    fs::write(root.join("bad.md"), SECRET).unwrap();
    git(&root, &["add", "bad.md"]);
    let out = git(&root, &["commit", "-qm", "bad"]);
    assert!(!out.status.success(), "secret commit must be blocked");
    assert!(String::from_utf8_lossy(&out.stderr).contains("bad.md"));

    kb_in(&root)
        .args(["git", "uninstall-hooks"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&hook).unwrap(),
        "#!/bin/sh\ntouch user-hook-ran\n"
    );
    assert!(git(&root, &["commit", "-qm", "bad"]).status.success());
}

#[test]
fn uninstall_removes_hook_it_created() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    init(&root);
    kb_in(&root)
        .args(["git", "install-hooks"])
        .assert()
        .success();
    assert!(root.join(".git/hooks/pre-commit").exists());
    kb_in(&root)
        .args(["git", "uninstall-hooks"])
        .assert()
        .success();
    assert!(!root.join(".git/hooks/pre-commit").exists());
}
