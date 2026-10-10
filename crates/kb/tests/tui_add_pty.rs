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
    s.wait_until("the prompt to go away", |screen| {
        !screen.contains("Discard this entry")
    });
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

const CTRL_R: &[u8] = b"\x12";
const CTRL_X: &[u8] = b"\x18";
const BACKSPACE: &[u8] = b"\x7f";
const A: &str = "sumo/5xx-by-authorizer";

fn select_sumo(s: &mut Session) {
    s.wait_for("entries");
    s.type_text("5xx auth");
    s.wait_for("5xx by authorizer");
}

#[test]
fn ctrl_r_renames_with_a_prefilled_prompt() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&spec(&t, &home, &root, &[], ""));
    select_sumo(&mut s);
    s.send(CTRL_R);
    s.wait_for(&format!("Rename to: {A}▌"));
    // an existing target keeps the prompt open and says why
    for _ in 0..A.len() {
        s.send(BACKSPACE);
    }
    s.type_text("cw/latency-prod");
    s.send(ENTER);
    s.wait_for("already exists");
    assert!(
        s.screen().contains("Rename to: cw/latency-prod"),
        "{}",
        s.screen()
    );
    s.send(ESC);
    s.wait_for("entries");
    assert!(root.join(format!("{A}.md")).is_file());
    // a free name is applied and selected
    select_after_clear(&mut s);
    s.send(CTRL_R);
    s.wait_for("Rename to:");
    s.type_text("-v2");
    s.send(ENTER);
    s.wait_for("Renamed to sumo/5xx-by-authorizer-v2");
    assert!(root.join("sumo/5xx-by-authorizer-v2.md").is_file());
    assert!(!root.join(format!("{A}.md")).exists());
    s.send(CTRL_C);
}

fn select_after_clear(s: &mut Session) {
    s.send(ESC); // clears the query
    s.wait_for("entries");
    s.type_text("5xx auth");
    s.wait_for("5xx by authorizer");
}

#[test]
fn ctrl_x_deletes_only_on_y() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&spec(&t, &home, &root, &[], ""));
    select_sumo(&mut s);
    s.send(CTRL_X);
    s.wait_for(&format!("Delete {A}? (y/N)"));
    s.type_text("n");
    assert!(root.join(format!("{A}.md")).is_file());
    s.send(CTRL_X);
    s.wait_for("(y/N)");
    s.type_text("y");
    s.wait_for(&format!("Deleted {A}"));
    assert!(!root.join(format!("{A}.md")).exists());
    s.send(CTRL_C);
}

#[test]
fn rename_and_delete_do_nothing_on_an_empty_list() {
    let (t, home, root) = kb_home("search");
    let mut s = Session::spawn(&spec(&t, &home, &root, &[], ""));
    s.wait_for("entries");
    s.type_text("zzzzqqqq-nothing");
    s.wait_for("No matches");
    s.send(CTRL_R);
    s.send(CTRL_X);
    assert!(
        !s.screen().contains("Rename to") && !s.screen().contains("Delete "),
        "{}",
        s.screen()
    );
    s.send(CTRL_C);
}

#[test]
fn list_ctrl_e_is_unchanged_it_does_not_bump_updated() {
    let (t, home, root) = kb_home("search");
    let before = fs::read_to_string(root.join(format!("{A}.md"))).unwrap();
    let editor = script(t.path(), "ed.sh", "echo 'note' >> \"$1\"");
    let spec = spec(&t, &home, &root, &[], "").env("KB_EDITOR", editor.to_str().unwrap());
    let mut s = Session::spawn(&spec);
    select_sumo(&mut s);
    s.send(CTRL_E);
    std::thread::sleep(std::time::Duration::from_millis(600));
    s.wait_for("entries");
    let after = fs::read_to_string(root.join(format!("{A}.md"))).unwrap();
    assert_eq!(after, format!("{before}note\n"));
    s.send(CTRL_C);
}

#[test]
fn space_accepts_a_suggestion_and_saving_declares_it() {
    let (t, home, root) = kb_home("search");
    let query = "_sourceCategory=prod/lambda/authorizer | timeslice 1m | count by x\n";
    let mut s = Session::spawn(&spec(&t, &home, &root, &[], query));
    s.wait_for("entries");
    s.send(CTRL_N);
    s.wait_for("detected sumo");
    s.type_text("By env");
    s.send(TAB); // tags
    s.send(TAB); // variables
    s.wait_for("suggest  prod → {{env}}");
    s.type_text(" ");
    s.wait_for("_sourceCategory={{env}}/lambda/authorizer");
    s.send(CTRL_S);
    s.wait_for("Saved sumo/by-env");
    let saved = fs::read_to_string(root.join("sumo/by-env.md")).unwrap();
    assert!(
        saved.contains("env: { kind: enum, options: [dev, qa, stage, prod], default: prod }"),
        "{saved}"
    );
    assert!(
        saved.contains("timeslice 1m"),
        "the other suggestion was not applied:\n{saved}"
    );
    s.send(CTRL_C);
}
