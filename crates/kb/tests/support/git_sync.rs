//! Scripted git repositories for the sync tests: a bare remote and two clones of it.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

pub fn entry(title: &str, body: &str) -> String {
    format!("---\nkb_format: 1\ntype: cmd\ntitle: {title}\n---\n\n```\n{body}\n```\n")
}

/// A bare remote and two clones of it (`a` made the first commit).
pub fn two_clones() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let remote = dir.path().join("remote.git");
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    git(
        dir.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    for (p, who) in [(&a, "A"), (&b, "B")] {
        git(
            dir.path(),
            &["clone", "-q", remote.to_str().unwrap(), p.to_str().unwrap()],
        );
        git(p, &["config", "user.name", who]);
        git(p, &["config", "user.email", "t@example.com"]);
        git(p, &["checkout", "-q", "-B", "main"]);
    }
    fs::write(a.join("base.md"), entry("Base", "echo base")).unwrap();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "kb: add base"]);
    git(&a, &["push", "-q", "-u", "origin", "main"]);
    git(&b, &["pull", "-q", "origin", "main"]);
    git(
        &b,
        &["branch", "-q", "--set-upstream-to=origin/main", "main"],
    );
    (dir, remote, a, b)
}
