//! `kb runbook runs`, `kb runbook run` (argument handling) and `kb runbook abandon`.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use kb_core::runbook::run::{self, StepStatus, fs as runfs};
use serde_json::Value;
use support::kb_in;

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("kb");
    fs::create_dir_all(root.join("runbook")).unwrap();
    fs::create_dir_all(root.join("cmd")).unwrap();
    fs::write(
        root.join("cmd/hello.md"),
        "---\nkb_format: 1\ntype: cmd\ntitle: Hello\n---\n\necho hello\n",
    )
    .unwrap();
    fs::write(
        root.join("runbook/deploy.md"),
        "---\nkb_format: 1\ntype: runbook\ntitle: Deploy it\nsteps:\n  - id: a\n    title: One\n    entry: cmd/hello\n  - id: b\n    title: Two\n    do: Think\n---\n",
    )
    .unwrap();
    (d, root)
}

/// A stored run with step `a` done.
fn make_run(root: &Path, now: i64) -> run::Run {
    let mut r = run::new_run(
        "runbook/deploy",
        "deploy",
        "Deploy it (stage)",
        "h",
        now,
        &["a", "b"],
    );
    r.vars.insert("env".into(), "stage".into());
    run::mark_step(&mut r, "a", StepStatus::Done, now + 1);
    runfs::save(root, &r).unwrap();
    r
}

#[test]
fn runs_lists_active_runs_as_a_table_and_as_json() {
    let (_d, root) = fixture();
    let r = make_run(&root, run::now_secs() - 120);
    let out = kb_in(&root).args(["runbook", "runs"]).output().unwrap();
    assert!(out.status.success());
    let t = text(&out.stdout);
    assert!(t.contains("ACTIVE RUNS (1)"), "{t}");
    assert!(t.contains(&r.id) && t.contains("1/2 done"), "{t}");

    let out = kb_in(&root)
        .args(["runbook", "runs", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc[0]["id"], r.id.as_str());
    assert_eq!(doc[0]["state"], "active");
    assert_eq!(doc[0]["steps_total"], 2);
    assert_eq!(doc[0]["steps_done"], 1);
    assert_eq!(doc[0]["is_locked"], false);
}

#[test]
fn abandoned_runs_are_hidden_until_all_and_can_be_pruned() {
    let (_d, root) = fixture();
    let r = make_run(&root, 1_000);
    let out = kb_in(&root)
        .args(["runbook", "abandon", &r.id, "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        runfs::load(&root, &r.id).unwrap().state,
        run::RunState::Abandoned
    );

    let active = kb_in(&root)
        .args(["runbook", "runs", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&active.stdout)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let all = kb_in(&root)
        .args(["runbook", "runs", "--all", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&all.stdout).unwrap();
    assert_eq!(doc[0]["state"], "abandoned");

    // an active run is never pruned
    let live = make_run(&root, run::now_secs());
    let out = kb_in(&root)
        .args(["runbook", "runs", "--prune", "--older-than", "1"])
        .output()
        .unwrap();
    assert!(
        text(&out.stdout).contains("removed 0"),
        "closed just now: {}",
        text(&out.stdout)
    );
    let out = kb_in(&root)
        .args(["runbook", "runs", "--prune"])
        .output()
        .unwrap();
    assert!(
        text(&out.stdout).contains("removed 1"),
        "{}",
        text(&out.stdout)
    );
    assert!(runfs::load(&root, &live.id).is_ok());
    assert!(matches!(
        runfs::load(&root, &r.id),
        Err(runfs::LoadError::NotFound)
    ));
}

#[test]
fn abandon_refuses_a_run_held_by_a_living_process_but_reclaims_a_stale_lock() {
    let (_d, root) = fixture();
    let r = make_run(&root, 5_000);
    let lock = root.join(".kb/runs").join(format!("{}.lock", r.id));

    // pid 1 is alive (and not ours)
    fs::write(&lock, r#"{"pid":1,"started_at":1,"hostname":"h"}"#).unwrap();
    let out = kb_in(&root)
        .args(["runbook", "abandon", &r.id, "--yes"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("PID 1"));
    let listed = kb_in(&root)
        .args(["runbook", "runs", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&listed.stdout).unwrap()[0]["is_locked"],
        true
    );

    // a dead pid is a stale lock
    fs::write(&lock, r#"{"pid":999999,"started_at":1,"hostname":"h"}"#).unwrap();
    let out = kb_in(&root)
        .args(["runbook", "abandon", &r.id, "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
}

#[test]
fn abandon_and_run_report_what_is_wrong() {
    let (_d, root) = fixture();
    let out = kb_in(&root)
        .args(["runbook", "abandon", "nope", "--yes"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = kb_in(&root)
        .args(["runbook", "run", "cmd/hello"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("not a runbook"));
    let out = kb_in(&root)
        .args(["runbook", "run", "zzzz-no-such"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn a_corrupt_run_file_is_kept_as_bak_and_not_listed() {
    let (_d, root) = fixture();
    make_run(&root, 100);
    fs::write(root.join(".kb/runs/broken-1.json"), "{not json").unwrap();
    let out = kb_in(&root)
        .args(["runbook", "runs", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let out = kb_in(&root)
        .args(["runbook", "abandon", "broken-1", "--yes"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(root.join(".kb/runs/broken-1.json.bak").exists());
}
