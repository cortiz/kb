//! Constitution VI for run mode: no secret value ever reaches `.kb/runs/` (spec 015 SC-003).

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use support::pty::{CTRL_C, ENTER, Session, Spec};

const CTRL_O: &[u8] = b"\x0f";
const FROM_ENV: &str = "env-secret-value-Zk3q9";
const TYPED: &str = "typed-secret-value-Qw7m2";

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn secrets_from_the_environment_and_from_typing_are_never_written_to_run_files() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let root = dir.path().join("kb");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(root.join("cmd")).unwrap();
    fs::create_dir_all(root.join("runbook")).unwrap();
    let marker = dir.path().join("marker.txt");
    fs::write(
        root.join("cmd/auth.md"),
        format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Auth check\nshell: sh\nvars:\n  token: {{ kind: secret, env: RUN_TEST_TOKEN }}\n  other: {{ kind: secret }}\n---\n\n```\necho {{{{token}}}} {{{{other}}}} > '{}'\n```\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::write(
        root.join("runbook/auth.md"),
        "---\nkb_format: 1\ntype: runbook\ntitle: Auth Runbook\nvars:\n  token: { kind: secret, env: RUN_TEST_TOKEN }\nsteps:\n  - id: check\n    title: Check auth\n    entry: cmd/auth\n---\n",
    )
    .unwrap();

    let spec = Spec::new(&["Auth Runbook"], &home, &root)
        .env("RUN_TEST_TOKEN", FROM_ENV)
        .env("KB_RUN_PAUSE_AFTER", "true");
    let mut s = Session::spawn(&spec);
    s.wait_for("runbook/auth.md");
    s.send(CTRL_O);
    // the runbook's own secret comes from the environment: its form (if shown) is locked
    s.wait_for("kb · variables");
    s.send(ENTER);
    s.wait_for("· run");
    s.send(ENTER);
    // `other` is only the step's: asked on first use, typed here
    s.wait_for("kb · variables");
    s.type_text(TYPED);
    s.send(ENTER);
    s.wait_for("Run Command");
    s.send(ENTER);
    s.wait_for("exit code 0");
    s.send(ENTER);
    s.wait_for("1/1 done");
    s.send(CTRL_C);
    s.wait_exit();

    let written = fs::read_to_string(&marker).unwrap();
    assert!(
        written.contains(FROM_ENV) && written.contains(TYPED),
        "injected into the command: {written}"
    );

    let mut files = Vec::new();
    files_under(&root.join(".kb"), &mut files);
    files_under(&home, &mut files);
    assert!(files.iter().any(
        |p| p.extension().is_some_and(|e| e == "json") && p.starts_with(root.join(".kb/runs"))
    ));
    for f in files {
        let text = fs::read_to_string(&f).unwrap_or_default();
        for secret in [FROM_ENV, TYPED] {
            assert!(!text.contains(secret), "{} holds a secret", f.display());
        }
    }
}
