//! `kb sync` multi-clone test verifying runbook rename consistency across clones.

mod support;

use std::fs;
use support::git_sync::{entry, git, two_clones};
use support::kb_in;

#[test]
fn runbook_rename_arrives_consistently_across_clones() {
    let (_d, _remote, a, b) = two_clones();

    // 1. In clone A: create a cmd entry and a runbook referencing it
    fs::create_dir_all(a.join("service")).unwrap();
    fs::create_dir_all(a.join("runbook")).unwrap();
    fs::write(
        a.join("service/deploy.md"),
        entry("Deploy service", "kubectl apply -f deploy.yaml"),
    )
    .unwrap();
    fs::write(
        a.join("runbook/rollout.md"),
        r#"---
kb_format: 1
type: runbook
title: Rollout service
steps:
  - id: step-deploy
    title: Deploy step
    entry: service/deploy
---
Rollout notes.
"#,
    )
    .unwrap();

    // Sync clone A to push to the bare remote
    kb_in(&a).arg("sync").assert().success();

    // Sync clone B to receive entry and runbook
    kb_in(&b).arg("sync").assert().success();

    // Verify clone B has both and passes lint
    assert!(b.join("service/deploy.md").is_file());
    assert!(b.join("runbook/rollout.md").is_file());
    kb_in(&b).arg("lint").assert().success();

    // 2. In clone A: rename the entry using `kb mv`
    let mv_out = kb_in(&a)
        .args(["mv", "service/deploy", "service/authorizer-deploy"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&mv_out.get_output().stdout);
    assert!(stdout.contains("updated references in 1 runbook: runbook/rollout"));

    // Sync clone A (pushes the single commit with rename + runbook reference update)
    kb_in(&a).arg("sync").assert().success();

    // 3. Sync clone B: receives the rename and updated runbook
    kb_in(&b).arg("sync").assert().success();

    // Verify clone B has updated references and lint passes cleanly
    assert!(b.join("service/authorizer-deploy.md").is_file());
    assert!(!b.join("service/deploy.md").exists());
    let b_rb_content = fs::read_to_string(b.join("runbook/rollout.md")).unwrap();
    assert!(b_rb_content.contains("entry: service/authorizer-deploy"));
    assert!(!b_rb_content.contains("entry: service/deploy\n"));

    // Verify kb lint in clone B passes with zero errors
    kb_in(&b).arg("lint").assert().success();

    // Both clones must end at the exact same commit HEAD
    assert_eq!(
        git(&a, &["rev-parse", "HEAD"]),
        git(&b, &["rev-parse", "HEAD"])
    );
}
