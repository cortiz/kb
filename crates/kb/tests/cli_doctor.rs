//! `kb doctor`: the environment checklist, its exit codes and the paste test.

mod support;

use std::fs;

use serde_json::Value;
use support::pty::{ENTER, Session, Spec, kb_home};
use support::{
    fake_clipboard, fake_paste, fixtures, kb, kb_without_terminal, path_with, search_kb,
};

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

#[test]
fn a_missing_root_fails_the_doctor() {
    let d = tempfile::tempdir().unwrap();
    let missing = d.path().join("nope");
    let out = kb()
        .arg("--root")
        .arg(&missing)
        .arg("doctor")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let report = text(&out.stdout);
    assert!(report.contains("FAIL root"), "{report}");
    assert!(
        report.contains("fix:") && report.contains("kb init"),
        "{report}"
    );
}

#[test]
fn a_healthy_kb_passes_and_warnings_alone_exit_0() {
    let (d, root) = search_kb();
    let (tool, dir, _copied, env) = fake_clipboard(d.path());
    let mut cmd = kb();
    cmd.arg("--root")
        .arg(&root)
        .arg("doctor")
        .env("PATH", path_with(&dir));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let report = text(&out.stdout);
    assert!(
        report.starts_with(&format!("kb {} on ", env!("CARGO_PKG_VERSION"))),
        "{report}"
    );
    assert!(
        report.lines().any(|l| l.contains(" ok ")
            && l.contains("root")
            && l.contains(root.to_str().unwrap())),
        "{report}"
    );
    assert!(
        report
            .lines()
            .any(|l| l.contains("clipboard") && l.contains(tool)),
        "{report}"
    );
    // piped, so plain marks (ok / warn / FAIL), never the unicode ones
    assert!(!report.contains('✓') && !report.contains('⚠'), "{report}");
    // the index was missing: a warning, not a failure
    assert!(
        report
            .lines()
            .any(|l| l.contains("warn") && l.contains("index")),
        "{report}"
    );
}

#[test]
fn json_is_the_same_checks() {
    let (_d, root) = search_kb();
    let out = kb()
        .arg("--root")
        .arg(&root)
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));
    let ids: Vec<&str> = doc["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    for want in [
        "kb",
        "root",
        "index",
        "config",
        "display",
        "clipboard",
        "editor",
        "opener",
    ] {
        assert!(ids.contains(&want), "{want} in {ids:?}");
    }
    // a failure shows in the json and the exit code
    let d = tempfile::tempdir().unwrap();
    let out = kb()
        .arg("--root")
        .arg(d.path().join("nope"))
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["ok"], false);
}

#[test]
fn doctor_changes_nothing_in_the_kb() {
    let (_d, root) = search_kb();
    let listing = |r: &std::path::Path| {
        let mut names: Vec<String> = fs::read_dir(r)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != ".kb")
            .collect();
        names.sort();
        names
    };
    let before = listing(&root);
    kb().arg("--root")
        .arg(&root)
        .arg("doctor")
        .output()
        .unwrap();
    assert_eq!(listing(&root), before, "nothing was added to the KB root");
}

#[test]
fn config_problems_show_up_as_a_warning_with_the_lines() {
    let (d, root) = search_kb();
    let home = d.path().join("home");
    fs::create_dir_all(home.join(".config/kb")).unwrap();
    fs::copy(
        fixtures().join("config/bad.toml"),
        home.join(".config/kb/config.toml"),
    )
    .unwrap();
    let out = kb()
        .arg("--root")
        .arg(&root)
        .arg("doctor")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let report = text(&out.stdout);
    assert!(
        report
            .lines()
            .any(|l| l.contains("warn") && l.contains("config")),
        "{report}"
    );
    assert!(
        report.contains("config.toml:3:") && report.contains("config.toml:6:"),
        "{report}"
    );
}

// ---- clipboard-read ----

fn read_check(out: &std::process::Output) -> Value {
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    doc["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "clipboard-read")
        .unwrap_or_else(|| panic!("no clipboard-read check in {doc}"))
        .clone()
}

#[test]
fn the_read_check_passes_when_the_read_tool_is_installed_and_never_reads() {
    let (d, root) = search_kb();
    let (tool, dir, log, env) = fake_paste(d.path(), "secret clipboard text");
    let mut cmd = kb();
    cmd.arg("--root")
        .arg(&root)
        .args(["doctor", "--json"])
        .env("PATH", path_with(&dir));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let check = read_check(&out);
    assert_eq!(check["status"], "pass", "{check}");
    assert_eq!(check["summary"], format!("reads with {tool}"));
    assert!(!log.exists(), "doctor must not read the clipboard");
    assert!(!text(&out.stdout).contains("secret clipboard text"));
}

#[test]
fn the_read_check_warns_with_a_hint_when_nothing_is_installed() {
    let (d, root) = search_kb();
    let empty = d.path().join("empty-bin");
    fs::create_dir_all(&empty).unwrap();
    let out = kb()
        .arg("--root")
        .arg(&root)
        .args(["doctor", "--json"])
        .env("PATH", &empty)
        .env("WAYLAND_DISPLAY", "wayland-test")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "warnings alone exit 0");
    let check = read_check(&out);
    assert_eq!(check["status"], "warn", "{check}");
    assert!(
        check["summary"]
            .as_str()
            .unwrap()
            .starts_with("cannot read the clipboard")
    );
    assert!(!check["hint"].as_str().unwrap().is_empty());
}

#[test]
fn the_read_check_says_ssh_cannot_read_the_clipboard() {
    let (d, root) = search_kb();
    let (_tool, dir, _log, env) = fake_paste(d.path(), "x");
    let mut cmd = kb();
    cmd.arg("--root")
        .arg(&root)
        .args(["doctor", "--json"])
        .env("PATH", path_with(&dir))
        .env("SSH_TTY", "/dev/pts/9");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let check = read_check(&cmd.output().unwrap());
    assert_eq!(check["status"], "warn");
    assert!(
        check["hint"]
            .as_str()
            .unwrap()
            .contains("stdin or an empty form")
    );
}

// ---- --copy-test ----

fn copy_test_spec(t: &tempfile::TempDir, home: &std::path::Path, root: &std::path::Path) -> Spec {
    let copied = t.path().join("copied");
    let tool = if cfg!(target_os = "macos") {
        "pbcopy"
    } else {
        "wl-copy"
    };
    let mut spec = Spec::new(&["doctor", "--copy-test"], home, root)
        .env("KB_CLIPBOARD_METHOD", "native")
        .env("KB_TEST_COPIED", copied.to_str().unwrap())
        .with_fake_tool(tool, "cat > \"$KB_TEST_COPIED\"");
    if !cfg!(target_os = "macos") {
        spec = spec.env("WAYLAND_DISPLAY", "wayland-test");
    }
    spec
}

#[test]
fn pasting_the_test_text_back_passes() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&copy_test_spec(&t, &home, &root));
    s.wait_for("Paste it here and press Enter");
    let token = fs::read_to_string(t.path().join("copied")).unwrap();
    assert!(token.starts_with("kb doctor test "), "{token:?}");
    s.type_text(&token);
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);
    let screen = s.screen();
    assert!(
        screen
            .lines()
            .any(|l| l.contains('✓') && l.contains("copy-test") && l.contains("arrived")),
        "{screen}"
    );
}

#[test]
fn pasting_something_else_fails_the_doctor() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&copy_test_spec(&t, &home, &root));
    s.wait_for("Paste it here and press Enter");
    s.type_text("something else");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 1);
    let screen = s.screen();
    assert!(
        screen.contains("✗ copy-test"),
        "a terminal gets the unicode marks: {screen}"
    );
    assert!(screen.contains("did not match"), "{screen}");
}

#[test]
fn the_copy_test_needs_a_terminal() {
    let (_d, root) = search_kb();
    let out = kb_without_terminal(
        &["--root", root.to_str().unwrap(), "doctor", "--copy-test"],
        &[],
    );
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("needs a terminal"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn the_copy_test_records_no_use() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&copy_test_spec(&t, &home, &root));
    s.wait_for("Paste it here and press Enter");
    let token = fs::read_to_string(t.path().join("copied")).unwrap();
    s.type_text(&token);
    s.send(ENTER);
    s.wait_exit();
    assert!(!root.join(".kb/state.json").exists());
}
