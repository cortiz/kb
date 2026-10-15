//! Integration tests for `kb show --checklist` and `--resolve`.

mod support;

use std::fs;
use std::path::PathBuf;
use support::kb_without_terminal;

const NOW: &str = "2026-10-15T12:00:00Z";

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

fn fixture_kb() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("kb");
    fs::create_dir_all(root.join("sumo")).unwrap();
    fs::create_dir_all(root.join("cmd")).unwrap();
    fs::create_dir_all(root.join("runbook")).unwrap();

    fs::write(
        root.join("sumo/5xx.md"),
        r#"---
kb_format: 1
type: sumo
title: 5xx errors by service
vars:
  env: { kind: enum, options: [dev, stage, prod], default: stage }
  minutes: { kind: number, default: 30 }
---

```sumo
_sourceCategory=api/{{env}} status >= 500 | timeslice {{minutes}}m
```
"#,
    )
    .unwrap();

    fs::write(
        root.join("cmd/deploy.md"),
        r#"---
kb_format: 1
type: cmd
title: Deploy service
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  version: { kind: text }
  token: { kind: secret, env: DEPLOY_TOKEN }
  secret_no_env: { kind: secret }
---

```sh
deploy.sh --env {{env}} --version {{version}} --token {{token}} --key {{secret_no_env}}
```
"#,
    )
    .unwrap();

    fs::write(
        root.join("runbook/deploy.md"),
        r#"---
kb_format: 1
type: runbook
title: Deploy authorizer to prod
tags: [deploy, auth]
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  version: { kind: text, default: v1.0.0 }
steps:
  - id: baseline
    title: Check the current error rate
    entry: sumo/5xx
    vars: { minutes: 15 }
  - id: deploy
    title: Deploy {{version}} to {{env}}
    entry: cmd/deploy
    confirm: true
  - id: rollback
    title: Roll back if p99 > 800 ms
    do: |
      Only if p99 stayed above 800 ms for 5 minutes.
      Post in #auth-oncall before rolling back.
    optional: true
---

Owner: platform security.
"#,
    )
    .unwrap();

    (d, root)
}

#[test]
fn test_show_checklist_unresolved() {
    let (_d, root) = fixture_kb();
    let out = kb_without_terminal(
        &[
            "--root",
            root.to_str().unwrap(),
            "show",
            "runbook/deploy",
            "--checklist",
        ],
        &[("KB_TEST_NOW", NOW)],
    );

    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);

    assert!(stdout.contains("# Deploy authorizer to prod"));
    assert!(stdout.contains("- [ ] 1. Check the current error rate (`sumo/5xx`)"));
    assert!(stdout.contains("- [ ] 2. Deploy {{version}} to {{env}} (`cmd/deploy`)"));
    assert!(stdout.contains("- [ ] 3. Roll back if p99 > 800 ms (optional)"));
    assert!(stdout.contains("  Only if p99 stayed above 800 ms for 5 minutes."));
    assert!(stdout.contains("  Post in #auth-oncall before rolling back."));
    // Unresolved checklist must not have code blocks
    assert!(!stdout.contains("```sumo"));
}

#[test]
fn test_show_checklist_resolved_with_secrets_masked() {
    let (_d, root) = fixture_kb();
    let out = kb_without_terminal(
        &[
            "--root",
            root.to_str().unwrap(),
            "show",
            "runbook/deploy",
            "--checklist",
            "--resolve",
            "--var",
            "env=prod",
            "--var",
            "version=v2.14.0",
        ],
        &[("KB_TEST_NOW", NOW), ("DEPLOY_TOKEN", "supersecret123")],
    );

    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);

    // Title resolved
    assert!(stdout.contains("- [ ] 2. Deploy v2.14.0 to prod (`cmd/deploy`)"));

    // Code blocks inlined
    assert!(stdout.contains("```sumo"));
    assert!(stdout.contains("_sourceCategory=api/prod status >= 500 | timeslice 15m"));

    assert!(stdout.contains("```sh"));
    assert!(stdout.contains("deploy.sh --env prod --version v2.14.0"));

    // Secrets masked: token with env source becomes $DEPLOY_TOKEN
    assert!(stdout.contains("$DEPLOY_TOKEN"));
    // Secret without env retains placeholder {{secret_no_env}}
    assert!(stdout.contains("{{secret_no_env}}"));

    // Raw secret values MUST NOT appear anywhere
    assert!(!stdout.contains("supersecret123"));
}
