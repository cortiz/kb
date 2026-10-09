//! `kb show`, `kb edit` and the terminal-less behavior of plain `kb`.

mod support;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kb_core::search::Kb;
use serde_json::Value;
use support::{kb_in, search_kb};

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

#[test]
fn show_prints_exactly_what_a_pick_prints() {
    let (_d, root) = search_kb();
    let kb = Kb::open(&root).unwrap();
    for (id, text) in [
        ("sumo/5xx-by-authorizer", None),
        (
            "link/payments-runbook",
            Some("https://wiki.example.com/runbooks/payments"),
        ),
        ("code/request-handler", Some("src/handler.rs:10")),
    ] {
        let out = kb_in(&root).args(["show", id]).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{id}: {}", stderr(&out));
        let want = text.map_or_else(
            || {
                kb.pick(id, kb_core::entry_json::PrintMode::Payload)
                    .unwrap()
            },
            str::to_string,
        );
        assert_eq!(stdout(&out), want, "{id}");
    }
    let out = kb_in(&root)
        .args(["show", "sumo/5xx-by-authorizer"])
        .output()
        .unwrap();
    assert!(!stdout(&out).ends_with("\n\n") && stdout(&out).contains("_sourceCategory"));
}

#[test]
fn show_modes() {
    let (_d, root) = search_kb();
    let id = "sumo/5xx-by-authorizer";
    let run = |args: &[&str]| {
        kb_in(&root)
            .arg("show")
            .arg(id)
            .args(args)
            .output()
            .unwrap()
    };
    assert_eq!(stdout(&run(&["--print", "id"])), id);
    let path = stdout(&run(&["--print", "path"]));
    assert!(
        std::path::Path::new(&path).is_absolute() && std::path::Path::new(&path).exists(),
        "{path}"
    );
    assert!(path.ends_with("sumo/5xx-by-authorizer.md"));
    let json = run(&["--print", "json"]);
    let doc: Value = serde_json::from_slice(&json.stdout).unwrap();
    for k in [
        "id", "path", "file", "type", "title", "tags", "vars", "payload", "notes", "fields",
    ] {
        assert!(doc.get(k).is_some(), "{k}");
    }
    assert_eq!(doc["payload"], stdout(&run(&[])));
    assert!(stdout(&json).ends_with("}\n"));
    assert_eq!(stdout(&run(&["--json"])), stdout(&json));
    assert_eq!(run(&["--print", "bogus"]).status.code(), Some(2));
}

#[test]
fn show_errors_are_exit_2() {
    let (_d, root) = search_kb();
    let out = kb_in(&root).args(["show", "nope/x"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("no entry with ID \"nope/x\""),
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), "");
    let out = kb_in(&root)
        .args(["show", "nope/x", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["error"]["kind"], "io");
    assert_eq!(
        kb_in(&root)
            .args(["show", "../x"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn show_records_no_use() {
    let (_d, root) = search_kb();
    kb_in(&root)
        .args(["show", "sumo/5xx-by-authorizer"])
        .output()
        .unwrap();
    assert!(!root.join(".kb/state.json").exists());
}

/// Runs `kb` with no controlling terminal and no stdin, in a new session.
fn without_a_terminal(args: &[&str]) -> (Option<i32>, String, Duration) {
    let started = Instant::now();
    let mut child = Command::new("perl")
        .args([
            "-MPOSIX",
            "-e",
            "POSIX::setsid(); exec @ARGV",
            "--",
            support::pty::KB,
        ])
        .args(args)
        .env_remove("KB_ROOT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("perl is available");
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut err).unwrap();
            return (status.code(), err, started.elapsed());
        }
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("kb waited for a terminal that does not exist");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn plain_kb_without_a_terminal_prints_help_and_exits_2() {
    let (_d, root) = search_kb();
    for args in [
        vec!["--root", root.to_str().unwrap()],
        vec!["--root", root.to_str().unwrap(), "serach", "x"],
    ] {
        let (code, err, took) = without_a_terminal(&args);
        assert_eq!(code, Some(2), "{err}");
        assert!(
            err.contains("Usage:") && err.contains("needs a terminal"),
            "{err}"
        );
        assert!(took < Duration::from_secs(5), "{took:?}");
    }
}

#[test]
fn help_mentions_the_ui_options() {
    let out = kb_in(std::path::Path::new("/x"))
        .args(["tui", "--help"])
        .output()
        .unwrap();
    assert!(stdout(&out).contains("--print"), "{}", stdout(&out));
    let out = kb_in(std::path::Path::new("/x"))
        .arg("--help")
        .output()
        .unwrap();
    let text = stdout(&out);
    for cmd in ["search", "show", "edit", "tui", "reindex"] {
        assert!(text.contains(cmd), "{cmd}");
    }
}

// ---- kb edit ----

fn script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// Runs `kb edit` under a pseudo-terminal (the editor needs one) and returns its exit code.
fn edit_in_a_pty(
    home: &std::path::Path,
    root: &std::path::Path,
    editor: &std::path::Path,
    id: &str,
) -> (u32, String) {
    use support::pty::{Session, Spec};
    let mut spec = Spec::new(&["edit", id], home, root);
    spec = spec.env("EDITOR", &editor.display().to_string());
    let mut s = Session::spawn(&spec);
    let code = s.wait_exit();
    (code, s.screen())
}

#[test]
fn edit_runs_the_editor_on_the_file() {
    let (t, home, root) = support::pty::kb_home("search");
    let marker = t.path().join("marker");
    let ed = script(
        t.path(),
        "ed",
        &format!("echo \"$1\" > {}", marker.display()),
    );
    let (code, screen) = edit_in_a_pty(&home, &root, &ed, "prompt/cache");
    assert_eq!(code, 0, "{screen}");
    let got = std::fs::read_to_string(&marker).unwrap();
    assert!(got.trim().ends_with("prompt/cache.md"), "{got}");
}

#[test]
fn edit_reports_the_first_error_the_editor_left_and_still_exits_0() {
    let (t, home, root) = support::pty::kb_home("search");
    let ed = script(
        t.path(),
        "breaker",
        "sed -i.bak 's/^type: prompt$/type: nope/' \"$1\"\nrm -f \"$1.bak\"",
    );
    let (code, screen) = edit_in_a_pty(&home, &root, &ed, "prompt/cache");
    assert_eq!(code, 0, "{screen}");
    assert!(
        screen.contains("prompt/cache.md:") && screen.contains("error[KB001"),
        "{screen}"
    );
}

#[test]
fn edit_with_an_unknown_id_or_failing_editor() {
    let (_d, root) = search_kb();
    let out = kb_in(&root).args(["edit", "nope/x"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("no entry with ID"),
        "{}",
        stderr(&out)
    );
    let out = kb_in(&root)
        .args(["edit", "nope/x", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["error"]["kind"], "io");
    let (t, home, root) = support::pty::kb_home("search");
    let ed = script(t.path(), "fail", "exit 3");
    let (code, _) = edit_in_a_pty(&home, &root, &ed, "prompt/cache");
    assert_eq!(code, 1);
}
