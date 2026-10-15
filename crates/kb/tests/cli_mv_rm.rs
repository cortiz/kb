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

// ---- entry links ----

fn note_with(root: &Path, id: &str, body: &str) {
    let path = root.join(format!("{id}.md"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!("---\nkb_format: 1\ntype: note\ntitle: {id}\nupdated: 2026-01-01\n---\n\n{body}"),
    )
    .unwrap();
}

fn link_kb() -> (tempfile::TempDir, std::path::PathBuf) {
    let (d, root) = search_kb();
    note_with(
        &root,
        "note/auth-guide",
        "# Guide\n\nSee [[sumo/5xx-by-authorizer]] and `[[sumo/5xx-by-authorizer]]`.\n",
    );
    note_with(
        &root,
        "note/incident-42",
        "# Incident\n\nQuery: [[sumo/5xx-by-authorizer]]. Other: [[cw/authorizer-latency]].\n\n```\n[[sumo/5xx-by-authorizer]]\n```\n",
    );
    note_with(
        &root,
        "note/unrelated",
        "# Unrelated\n\nNothing to see: [[cw/authorizer-latency]].\n",
    );
    (d, root)
}

#[test]
fn mv_points_the_links_of_notes_at_the_new_id_and_touches_nothing_else() {
    let (_d, root) = link_kb();
    let out = kb_in(&root)
        .args(["mv", A, "sumo/5xx-by-gateway"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "renamed sumo/5xx-by-authorizer -> sumo/5xx-by-gateway (rename)\nupdated links in 2 notes: note/auth-guide, note/incident-42\n"
    );
    let guide = fs::read_to_string(root.join("note/auth-guide.md")).unwrap();
    assert_eq!(
        guide,
        "---\nkb_format: 1\ntype: note\ntitle: note/auth-guide\nupdated: 2026-01-01\n---\n\n# Guide\n\nSee [[sumo/5xx-by-gateway]] and `[[sumo/5xx-by-authorizer]]`.\n",
        "links in code stay, everything else is byte for byte, `updated` included"
    );
    let incident = fs::read_to_string(root.join("note/incident-42.md")).unwrap();
    assert!(
        incident.contains("Query: [[sumo/5xx-by-gateway]]. Other: [[cw/authorizer-latency]]."),
        "{incident}"
    );
    assert!(
        incident.contains("```\n[[sumo/5xx-by-authorizer]]\n```"),
        "{incident}"
    );
    assert!(
        fs::read_to_string(root.join("note/unrelated.md"))
            .unwrap()
            .contains("[[cw/authorizer-latency]]")
    );
    // lint now finds no broken links to the old id
    let lint = kb_in(&root).arg("lint").output().unwrap();
    assert!(
        !text(&lint.stdout).contains("KB074"),
        "{}",
        text(&lint.stdout)
    );
    // the index follows
    let json = kb_in(&root)
        .args(["mv", "cw/authorizer-latency", "cw/latency", "--json"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        v["links_updated"],
        serde_json::json!(["note/incident-42", "note/unrelated"])
    );
}

#[test]
fn a_note_that_links_to_itself_follows_its_own_rename() {
    let (_d, root) = search_kb();
    note_with(&root, "note/self", "# Self\n\nI am [[note/self]].\n");
    let out = kb_in(&root)
        .args(["mv", "note/self", "note/me"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        fs::read_to_string(root.join("note/me.md"))
            .unwrap()
            .contains("I am [[note/me]].")
    );
}

#[test]
fn mv_commits_the_rename_and_the_edited_notes_together() {
    if !have_git() {
        return;
    }
    let (_d, root) = link_kb();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
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
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    let out = kb_in(&root)
        .args(["mv", A, "sumo/5xx-by-gateway"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        git(&root, &["rev-list", "--count", "HEAD"]).trim(),
        "2",
        "one commit, not one per file"
    );
    let changed = git(&root, &["show", "--name-status", "--format=%s", "HEAD"]);
    for want in [
        "sumo/5xx-by-gateway.md",
        "note/auth-guide.md",
        "note/incident-42.md",
    ] {
        assert!(changed.contains(want), "{want} missing:\n{changed}");
    }
    assert!(!changed.contains("note/unrelated.md"), "{changed}");
    assert_eq!(
        git(&root, &["status", "--porcelain"]).trim(),
        "",
        "nothing is left over"
    );
}

#[test]
fn rm_warns_about_notes_that_link_to_the_entry() {
    let (_t, home, root) = kb_home("search");
    note_with(
        &root,
        "note/auth-guide",
        "# Guide\n\nSee [[sumo/5xx-by-authorizer]].\n",
    );
    note_with(
        &root,
        "note/incident-42",
        "# Incident\n\n[[sumo/5xx-by-authorizer]]\n",
    );
    let question = "Entry 'sumo/5xx-by-authorizer' is referenced by 2 notes (note/auth-guide, note/incident-42). Delete anyway? [y/N]";
    // no: nothing changes and the exit is clean
    let mut s = Session::spawn(&Spec::new(&["rm", A], &home, &root));
    s.wait_for(question);
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(root.join(format!("{A}.md")).is_file());
    // yes: it goes
    let mut s = Session::spawn(&Spec::new(&["rm", A], &home, &root));
    s.wait_for("Delete anyway? [y/N]");
    s.type_text("y");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(!root.join(format!("{A}.md")).exists());
}

#[test]
fn rm_with_yes_says_what_it_leaves_dangling_and_unlinked_entries_are_not_questioned() {
    let (_d, root) = link_kb();
    let out = kb_in(&root).args(["rm", A, "--yes"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("kb: Entry 'sumo/5xx-by-authorizer' is referenced by 2 notes (note/auth-guide, note/incident-42); deleting anyway"),
        "{}",
        text(&out.stderr)
    );
    let lint = kb_in(&root).arg("lint").output().unwrap();
    assert!(
        text(&lint.stdout).contains("KB074"),
        "the notes now have broken links"
    );
    let out = kb_in(&root)
        .args(["rm", "link/payments-runbook", "--yes"])
        .output()
        .unwrap();
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn mv_updates_referencing_runbooks_in_one_git_commit() {
    if !have_git() {
        return;
    }
    let (_d, root) = search_kb();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "Carlos Ortiz"]);
    git(&root, &["config", "user.email", "carlos@example.com"]);

    // Create a runbook that references sumo/5xx-by-authorizer
    let rb_path = root.join("runbook/deploy-service.md");
    fs::create_dir_all(rb_path.parent().unwrap()).unwrap();
    fs::write(
        &rb_path,
        r#"---
kb_format: 1
type: runbook
title: Deploy service
steps:
  - id: check
    title: Check error rate
    entry: sumo/5xx-by-authorizer
---
Notes.
"#,
    )
    .unwrap();

    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "initial commit with runbook"]);

    let out = kb_in(&root)
        .args(["mv", A, "sumo/authorizer/5xx"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("renamed sumo/5xx-by-authorizer -> sumo/authorizer/5xx"),
        "{stdout}"
    );
    assert!(
        stdout.contains("updated references in 1 runbook: runbook/deploy-service"),
        "{stdout}"
    );

    // Verify runbook content updated format-preserving
    let updated_rb = fs::read_to_string(&rb_path).unwrap();
    assert!(
        updated_rb.contains("entry: sumo/authorizer/5xx"),
        "runbook step should be updated: {updated_rb}"
    );
    assert!(
        !updated_rb.contains("entry: sumo/5xx-by-authorizer"),
        "old entry id should not remain: {updated_rb}"
    );

    // Verify exactly one new commit in git log
    let log_msg = git(&root, &["log", "-1", "--format=%B"]);
    assert!(
        log_msg.contains("mv sumo/5xx-by-authorizer -> sumo/authorizer/5xx"),
        "{log_msg}"
    );
    assert!(
        log_msg.contains("Updated references in:\n- runbook/deploy-service"),
        "{log_msg}"
    );

    // Verify git status is completely clean
    let status = git(&root, &["status", "--porcelain"]);
    assert_eq!(status.trim(), "", "working tree must be clean: {status}");
}

#[test]
fn rm_refuses_deletion_if_referenced_by_runbook() {
    let (_d, root) = search_kb();

    // Create a runbook that references cmd/deploy-service
    let rb_path = root.join("runbook/test-rb.md");
    fs::create_dir_all(rb_path.parent().unwrap()).unwrap();
    fs::write(
        &rb_path,
        r#"---
kb_format: 1
type: runbook
title: Test runbook
steps:
  - id: deploy
    title: Deploy
    entry: cmd/deploy-service
---
Notes.
"#,
    )
    .unwrap();

    // Try rm without --force: must refuse and exit 1
    let out = kb_in(&root)
        .args(["rm", "cmd/deploy-service", "--yes"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("cannot delete 'cmd/deploy-service': referenced by runbook(s):"),
        "{stderr}"
    );
    assert!(stderr.contains("- runbook/test-rb (step: deploy)"), "{stderr}");
    assert!(
        root.join("cmd/deploy-service.md").is_file(),
        "file must not be deleted"
    );

    // Now rm with --force: must succeed
    let out_force = kb_in(&root)
        .args(["rm", "cmd/deploy-service", "--force", "--yes"])
        .output()
        .unwrap();
    assert_eq!(out_force.status.code(), Some(0), "{}", text(&out_force.stderr));
    let stderr_force = text(&out_force.stderr);
    assert!(
        stderr_force.contains("warning: 'cmd/deploy-service' is referenced by runbook(s); deleting anyway"),
        "{stderr_force}"
    );
    assert!(
        !root.join("cmd/deploy-service.md").exists(),
        "file must be deleted with --force"
    );

    // Lint now reports KB082 (broken reference) on the runbook
    let lint = kb_in(&root).arg("lint").output().unwrap();
    assert_eq!(lint.status.code(), Some(1));
    let stdout_lint = text(&lint.stdout);
    assert!(
        stdout_lint.contains("KB082"),
        "expected broken reference diagnostic: {stdout_lint}"
    );
}

