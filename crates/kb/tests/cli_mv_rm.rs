//! `kb mv` and `kb rm`: renames and deletes that keep usage, use git when the KB is a tracked
//! work tree, and refresh the index.

mod support;

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use support::pty::{ENTER, Session, Spec, kb_home};
use support::{kb_in, search_kb};

const A: &str = "sumo/5xx-by-authorizer";

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

fn ids(root: &Path) -> Vec<String> {
    let out = kb_in(root)
        .args(["search", "--ids", "--limit", "0"])
        .output()
        .unwrap();
    text(&out.stdout).lines().map(str::to_string).collect()
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", text(&out.stderr));
    text(&out.stdout)
}

fn have_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

#[test]
fn mv_renames_creates_folders_and_cleans_up() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["mv", A, "new/deep/place"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!("renamed {A} -> new/deep/place (rename)\n")
    );
    assert!(root.join("new/deep/place.md").is_file() && !root.join(format!("{A}.md")).exists());
    assert!(ids(&root).contains(&"new/deep/place".to_string()));
    assert!(!ids(&root).contains(&A.to_string()));
}

#[test]
fn mv_keeps_the_position_in_an_empty_query_search() {
    let (_d, root) = search_kb();
    for _ in 0..3 {
        kb_in(&root)
            .args(["show", "--raw", "link/payments-runbook"])
            .output()
            .unwrap();
        let _ = kb_in(&root)
            .args(["copy", "--method", "stdout", "link/payments-runbook"])
            .output()
            .unwrap();
    }
    let before = ids(&root);
    let at = before
        .iter()
        .position(|i| i == "link/payments-runbook")
        .unwrap();
    kb_in(&root)
        .args(["mv", "link/payments-runbook", "link/runbook-payments"])
        .output()
        .unwrap();
    let after = ids(&root);
    assert_eq!(
        after.iter().position(|i| i == "link/runbook-payments"),
        Some(at),
        "{before:?}\n{after:?}"
    );
}

#[test]
fn mv_carries_usage_and_values_even_after_an_edit() {
    let (_d, root) = support::vars_kb();
    let out = kb_in(&root)
        .args([
            "use",
            A,
            "--var",
            "env=prod",
            "--var",
            "minutes=45",
            "-y",
            "--method",
            "stdout",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let p = root.join(format!("{A}.md"));
    fs::write(
        &p,
        format!("{}\nExtra note.\n", fs::read_to_string(&p).unwrap()),
    )
    .unwrap();
    kb_in(&root).args(["mv", A, "sumo/moved"]).output().unwrap();
    let state: Value =
        serde_json::from_slice(&fs::read(root.join(".kb/state.json")).unwrap()).unwrap();
    assert_eq!(state["usage"]["sumo/moved"]["count"], 1);
    assert!(state["usage"].get(A).is_none());
    assert_eq!(state["vars"]["entries"]["sumo/moved"]["minutes"], "45");
}

#[test]
fn mv_refuses_what_it_cannot_do() {
    let (_d, root) = search_kb();
    let other = "cw/latency-prod";
    let out = kb_in(&root).args(["mv", A, other]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("already exists"),
        "{}",
        text(&out.stderr)
    );
    let out = kb_in(&root)
        .args(["mv", A, "CW/Latency-Prod"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "different case still clashes");
    assert!(root.join(format!("{A}.md")).is_file());
    let out = kb_in(&root).args(["mv", A, A]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let out = kb_in(&root)
        .args(["mv", "no/such", "x/y"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = kb_in(&root).args(["mv", A, "../escape"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("not a valid"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn mv_and_rm_use_git_for_tracked_files_and_fall_back_otherwise() {
    if !have_git() {
        return;
    }
    let (_d, root) = search_kb();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "--", "sumo", "cw", "link"]);
    git(
        &root,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    );
    let out = kb_in(&root)
        .args(["mv", A, "sumo/renamed"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!("renamed {A} -> sumo/renamed (git mv)\n")
    );
    let status = git(&root, &["status", "--porcelain"]);
    assert!(
        status.contains("R  sumo/5xx-by-authorizer.md -> sumo/renamed.md"),
        "{status}"
    );
    // an untracked file is renamed plainly
    let untracked = root.join("prompt/cache.md");
    assert!(untracked.is_file());
    let out = kb_in(&root)
        .args(["mv", "prompt/cache", "prompt/cache2"])
        .output()
        .unwrap();
    assert!(
        text(&out.stdout).ends_with("(rename)\n"),
        "{}",
        text(&out.stdout)
    );
    // rm: tracked uses git rm, untracked a plain delete
    let out = kb_in(&root)
        .args(["rm", "sumo/renamed", "--yes"])
        .output()
        .unwrap();
    assert_eq!(
        text(&out.stdout),
        "deleted sumo/renamed (git rm)\n",
        "{}",
        text(&out.stderr)
    );
    assert!(
        git(&root, &["status", "--porcelain"]).contains("D  sumo/renamed.md")
            || git(&root, &["status", "--porcelain"]).contains("D  sumo/5xx")
    );
    let out = kb_in(&root)
        .args(["rm", "prompt/cache2", "--yes"])
        .output()
        .unwrap();
    assert!(
        text(&out.stdout).ends_with("(delete)\n"),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn no_git_means_plain_operations() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["mv", A, "sumo/renamed"])
        .env("PATH", "/nonexistent-bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).ends_with("(rename)\n"));
}

#[test]
fn json_shapes() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["mv", A, "sumo/j", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        (
            doc["id"].as_str(),
            doc["from"].as_str(),
            doc["to"].as_str(),
            doc["method"].as_str()
        ),
        (Some("sumo/j"), Some(A), Some("sumo/j"), Some("rename"))
    );
    let out = kb_in(&root)
        .args(["rm", "sumo/j", "--yes", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        (doc["id"].as_str(), doc["method"].as_str()),
        (Some("sumo/j"), Some("delete"))
    );
}

#[test]
fn rm_forgets_usage_and_cleans_folders() {
    let (_d, root) = support::vars_kb();
    kb_in(&root)
        .args([
            "use",
            A,
            "--var",
            "env=prod",
            "--var",
            "minutes=45",
            "-y",
            "--method",
            "stdout",
        ])
        .output()
        .unwrap();
    let out = kb_in(&root).args(["rm", A, "-y"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(!root.join(format!("{A}.md")).exists());
    let state: Value =
        serde_json::from_slice(&fs::read(root.join(".kb/state.json")).unwrap()).unwrap();
    assert!(state["usage"].get(A).is_none());
    assert!(state["vars"]["entries"].get(A).is_none());
    assert!(!ids(&root).contains(&A.to_string()));
    let out = kb_in(&root).args(["rm", "no/such", "-y"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn rm_asks_on_the_terminal_and_only_y_deletes() {
    let (_t, home, root) = kb_home("search");
    let mut s = Session::spawn(&Spec::new(&["rm", A], &home, &root));
    s.wait_for(&format!("Delete {A}? (y/N)"));
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 130);
    assert!(root.join(format!("{A}.md")).is_file());

    let mut s = Session::spawn(&Spec::new(&["rm", A], &home, &root));
    s.wait_for("(y/N)");
    s.type_text("y");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(!root.join(format!("{A}.md")).exists());
}

#[test]
fn rm_without_a_terminal_needs_yes() {
    let (_d, root) = search_kb();
    let out = support::kb_without_terminal(&["--root", root.to_str().unwrap(), "rm", A], &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("kb rm needs --yes when there is no terminal"));
    assert!(root.join(format!("{A}.md")).is_file());
}
