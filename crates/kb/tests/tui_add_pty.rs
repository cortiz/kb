//! The add form (screen 4) under a pseudo-terminal, with fake clipboard programs and editors.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use support::pty::{CTRL_C, ENTER, ESC, Session, Spec, TAB, kb_home};

const CTRL_N: &[u8] = b"\x0e";
const CTRL_S: &[u8] = b"\x13";
const CTRL_E: &[u8] = b"\x05";
const NOW: &str = "2026-10-09T12:00:00Z";
const CW: &str = "fields @timestamp, @message\n| filter @message like /{{fn}}/\n| stats pct(@duration, 99) by bin({{minutes}}m)\n";

fn read_tool() -> (&'static str, Vec<(&'static str, &'static str)>) {
    if cfg!(target_os = "macos") {
        ("pbpaste", vec![])
    } else {
        ("wl-paste", vec![("WAYLAND_DISPLAY", "wayland-test")])
    }
}

fn spec(t: &tempfile::TempDir, home: &Path, root: &Path, args: &[&str], clipboard: &str) -> Spec {
    let content = t.path().join("clipboard.txt");
    fs::write(&content, clipboard).unwrap();
    let (tool, env) = read_tool();
    let mut spec = Spec::new(args, home, root)
        .env("KB_TEST_NOW", NOW)
        .env("KB_TUI_NO_REFRESH_TIMER", "1")
        .with_fake_tool(tool, &format!("cat '{}'", content.display()));
    for (k, v) in env {
        spec = spec.env(k, v);
    }
    spec
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn temp_files() -> Vec<String> {
    fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("kb-") && !n.contains("tmp"))
        .collect()
}

#[test]
fn ctrl_n_title_ctrl_s_saves_the_golden_file_in_two_key_presses() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&spec(&t, &home, &root, &[], CW));
    s.wait_for("entries");
    s.send(CTRL_N); // key press 1
    s.wait_for("detected cw");
    assert!(
        s.screen().contains("New entry · from clipboard"),
        "{}",
        s.screen()
    );
    s.type_text("Authorizer p99 latency"); // text, not counted
    s.wait_for("Authorizer p99 latency");
    s.send(CTRL_S); // key press 2
    s.wait_for("Saved cw/authorizer-p99-latency");
    let want = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/authoring/golden/03-cw.md"),
    )
    .unwrap();
    let got = fs::read_to_string(root.join("cw/authorizer-p99-latency.md")).unwrap();
    assert_eq!(got, want);
    s.send(CTRL_C);
}

#[test]
fn an_empty_clipboard_opens_the_form_on_the_type() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&spec(&t, &home, &root, &[], ""));
    s.wait_for("entries");
    s.send(CTRL_N);
    s.wait_for("New entry · empty");
    assert!(s.screen().contains("▸Type"), "{}", s.screen());
    s.send(CTRL_C);
}

#[test]
fn escape_asks_only_when_something_changed() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&spec(&t, &home, &root, &[], CW));
    s.wait_for("entries");
    s.send(CTRL_N);
    s.wait_for("detected cw");
    s.send(ESC); // nothing changed: closes at once
    s.wait_for("entries");
    s.send(CTRL_N);
    s.wait_for("detected cw");
    s.type_text("x");
    s.send(ESC);
    s.wait_for("Discard this entry? (y/N)");
    s.type_text("n");
    s.wait_for("New entry");
    s.send(ESC);
    s.wait_for("Discard this entry? (y/N)");
    s.type_text("y");
    s.wait_for("entries");
    assert!(!root.join("cw/x.md").exists());
    s.send(CTRL_C);
}

#[test]
fn a_missing_title_is_explained_and_nothing_is_written() {
    let (t, home, root) = kb_home("search");
    let before = fs::read_dir(root.join("cw")).unwrap().count();
    let mut s = Session::spawn(&spec(&t, &home, &root, &[], CW));
    s.wait_for("entries");
    s.send(CTRL_N);
    s.wait_for("detected cw");
    s.send(CTRL_S);
    s.wait_for("a title is required");
    assert_eq!(fs::read_dir(root.join("cw")).unwrap().count(), before);
    s.send(CTRL_C);
}

#[test]
fn a_secret_is_refused_in_the_form() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&spec(
        &t,
        &home,
        &root,
        &[],
        "aws s3 ls --profile x # AKIAIOSFODNN7EXAMPLE\nkubectl get pods -n prod\n",
    ));
    s.wait_for("entries");
    s.send(CTRL_N);
    s.wait_for("New entry");
    s.type_text("Leaky");
    s.send(CTRL_S);
    s.wait_for("KB060");
    assert!(!root.join("cmd/leaky.md").exists());
    s.send(CTRL_C);
}

#[test]
fn ctrl_e_replaces_the_body_and_reruns_detection() {
    let (t, home, root) = kb_home("search");
    let editor = script(
        t.path(),
        "editor.sh",
        "printf 'select state, count(*)\\nfrom pg_stat_activity\\ngroup by state;\\n' > \"$1\"",
    );
    let before = temp_files();
    let spec = spec(&t, &home, &root, &[], CW).env("KB_EDITOR", editor.to_str().unwrap());
    let mut s = Session::spawn(&spec);
    s.wait_for("entries");
    s.send(CTRL_N);
    s.wait_for("detected cw");
    s.send(CTRL_E);
    s.wait_for("pg_stat_activity");
    assert!(
        s.screen().contains("type changed from cw to sql"),
        "{}",
        s.screen()
    );
    s.type_text("Sessions");
    s.send(CTRL_S);
    s.wait_for("Saved sql/sessions");
    assert!(
        fs::read_to_string(root.join("sql/sessions.md"))
            .unwrap()
            .contains("group by state;")
    );
    assert_eq!(temp_files(), before, "the temp file is gone");
    s.send(CTRL_C);
}

#[test]
fn a_failing_or_unchanged_editor_leaves_the_body_alone() {
    let (t, home, root) = kb_home("search");
    let bad = script(t.path(), "bad.sh", "printf 'junk' > \"$1\"; exit 3");
    let spec = spec(&t, &home, &root, &[], CW).env("KB_EDITOR", bad.to_str().unwrap());
    let mut s = Session::spawn(&spec);
    s.wait_for("entries");
    s.send(CTRL_N);
    s.wait_for("detected cw");
    s.send(CTRL_E);
    s.wait_for("the editor exited with an error");
    assert!(s.screen().contains("pct(@duration, 99)"), "{}", s.screen());
    assert!(!s.screen().contains("junk"));
    s.send(CTRL_C);
}

#[test]
fn kb_add_opens_the_form_and_prints_the_id() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("out.txt");
    let mut sp = spec(&t, &home, &root, &["add"], CW);
    sp.out = Some(out.clone());
    let mut s = Session::spawn(&sp);
    s.wait_for("detected cw");
    s.type_text("Authorizer p99 latency");
    s.send(CTRL_S);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        "cw/authorizer-p99-latency\n"
    );
    assert!(root.join("cw/authorizer-p99-latency.md").is_file());
}

#[test]
fn escape_in_kb_add_exits_130_and_prints_nothing() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("out.txt");
    let mut sp = spec(&t, &home, &root, &["add"], CW);
    sp.out = Some(out.clone());
    let mut s = Session::spawn(&sp);
    s.wait_for("detected cw");
    s.send(ESC);
    assert_eq!(s.wait_exit(), 130);
    assert_eq!(fs::read_to_string(&out).unwrap_or_default(), "");
}

#[test]
fn tab_walks_the_fields_and_tag_completion_uses_the_kb() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&spec(&t, &home, &root, &[], CW));
    s.wait_for("entries");
    s.send(CTRL_N);
    s.wait_for("detected cw");
    s.type_text("Tagged");
    s.send(TAB);
    s.wait_for("▸Tags");
    s.type_text("lam");
    s.wait_for("‹lambda›");
    s.send(TAB); // accepts the completion
    s.wait_for("lambda, ");
    s.send(ENTER);
    s.send(CTRL_C);
}
