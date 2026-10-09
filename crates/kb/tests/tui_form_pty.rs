//! The variable form (screen 2) under a pseudo-terminal, with a fake clipboard program.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use support::pty::{CTRL_C, CTRL_U, ENTER, ESC, Session, Spec, TAB, kb_home};

const CTRL_Z: &[u8] = b"\x1a";
const ALT_ENTER: &[u8] = b"\x1b\r";
const ALT_DOWN: &[u8] = b"\x1b[1;3B";
const ALT_C: &[u8] = b"\x1bc";
const RIGHT: &[u8] = b"\x1b[C";
const CTRL_Y: &[u8] = b"\x19";

const QUERY: &str =
    "_sourceCategory=prod/api | where status >= 500 | timeslice 30m | count by authorizer";

fn native() -> (&'static str, Vec<(&'static str, &'static str)>) {
    if cfg!(target_os = "macos") {
        ("pbcopy", vec![])
    } else {
        ("wl-copy", vec![("WAYLAND_DISPLAY", "wayland-test")])
    }
}

/// A session on the `vars` fixture that copies through a fake clipboard program.
fn session(
    t: &tempfile::TempDir,
    home: &Path,
    root: &Path,
    size: (u16, u16),
) -> (Session, PathBuf) {
    let copied = t.path().join("copied");
    let (tool, env) = native();
    let mut spec = Spec::new(&[], home, root)
        .copy_mode("auto")
        .env("KB_TEST_COPIED", copied.to_str().unwrap())
        .with_fake_tool(tool, "cat > \"$KB_TEST_COPIED\"");
    for (k, v) in env {
        spec = spec.env(k, v);
    }
    spec.size = size;
    (Session::spawn(&spec), copied)
}

/// The form is drawn completely once its key bar is there (it is the last row painted).
fn form_is_up(s: &Session) {
    s.wait_for("Esc back");
}

/// Searches for `query` and waits until the entry whose file is `file` is the result.
fn find(s: &mut Session, query: &str, file: &str) {
    s.wait_for("17 entries");
    s.type_text(query);
    s.wait_for(file);
}

fn open_the_form(s: &mut Session) {
    find(s, "5xx", "sumo/5xx-by-authorizer.md");
    s.send(ENTER);
    form_is_up(s);
}

/// Picks `prod`, then makes minutes 30.
fn fill(s: &mut Session) {
    s.send(RIGHT); // env: nothing chosen yet, so the first option
    s.wait_for("[prod]");
    s.send(TAB);
    s.send(CTRL_U);
    s.type_text("30");
}

#[test]
fn enter_opens_the_form_and_enter_again_copies_the_resolved_text() {
    let (t, home, root) = kb_home("vars");
    let (mut s, copied) = session(&t, &home, &root, (120, 40));
    open_the_form(&mut s);
    let screen = s.screen();
    assert!(
        screen.contains("variables ·") && screen.contains("field 1/2"),
        "{screen}"
    );
    assert!(
        screen.contains("env") && screen.contains("enum") && screen.contains("minutes"),
        "{screen}"
    );
    assert!(
        screen.contains("{{env}}"),
        "the preview shows the placeholder until it has a value:\n{screen}"
    );
    fill(&mut s);
    s.wait_for("timeslice 30m");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(fs::read_to_string(copied).unwrap(), QUERY);
    let state = fs::read_to_string(root.join(".kb/state.json")).unwrap();
    assert!(
        state.contains("\"minutes\": \"30\"") && state.contains("\"env\": \"prod\""),
        "{state}"
    );
}

#[test]
fn a_wrong_value_blocks_the_copy_and_says_what_is_expected() {
    let (t, home, root) = kb_home("vars");
    let (mut s, copied) = session(&t, &home, &root, (120, 40));
    open_the_form(&mut s);
    s.send(RIGHT);
    s.send(TAB);
    s.send(CTRL_U);
    s.type_text("abc");
    s.send(ENTER);
    s.wait_for("expected a number like 30, -5 or 0.25");
    assert!(s.is_running());
    assert!(!copied.exists());
    // fix it and go on
    s.send(CTRL_U);
    s.type_text("5");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);
    assert!(fs::read_to_string(copied).unwrap().contains("timeslice 5m"));
}

#[test]
fn esc_returns_to_the_list_and_what_was_typed_is_kept() {
    let (t, home, root) = kb_home("vars");
    let (mut s, _copied) = session(&t, &home, &root, (120, 40));
    open_the_form(&mut s);
    fill(&mut s);
    s.send(ESC);
    s.wait_for("Esc clear");
    assert!(s.screen().contains("5xx by authorizer") && !s.screen().contains("variables ·"));
    s.send(ENTER);
    form_is_up(&s);
    s.wait_until("the typed value", |screen| {
        screen.contains("timeslice 30m") && screen.contains("[prod]")
    });
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn alt_enter_prints_the_resolved_text_to_stdout() {
    let (t, home, root) = kb_home("vars");
    let out = t.path().join("stdout");
    let (tool, env) = native();
    let mut spec = Spec::new(&[], &home, &root)
        .copy_mode("auto")
        .with_fake_tool(tool, "cat >/dev/null");
    for (k, v) in env {
        spec = spec.env(k, v);
    }
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    open_the_form(&mut s);
    fill(&mut s);
    s.send(ALT_ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(fs::read_to_string(out).unwrap(), format!("{QUERY}\n"));
}

#[test]
fn ctrl_y_opens_the_form_copies_and_stays_open() {
    let (t, home, root) = kb_home("vars");
    let (mut s, copied) = session(&t, &home, &root, (120, 40));
    find(&mut s, "5xx", "sumo/5xx-by-authorizer.md");
    s.send(CTRL_Y);
    form_is_up(&s);
    fill(&mut s);
    s.send(ENTER);
    s.wait_until("the copy to be reported", |screen| {
        screen.contains("Copied") && screen.contains("Esc quit")
            || screen.contains("Copied") && !screen.contains("variables ·")
    });
    assert_eq!(fs::read_to_string(copied).unwrap(), QUERY);
    assert!(s.is_running(), "the UI stays open");
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn an_entry_without_variables_copies_on_the_first_enter() {
    let (t, home, root) = kb_home("vars");
    let (mut s, copied) = session(&t, &home, &root, (120, 40));
    find(&mut s, "No variables", "cmd/no-vars.md");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);
    assert_eq!(fs::read_to_string(copied).unwrap(), "echo plain");
}

#[test]
fn alt_c_copies_the_template_without_opening_the_form() {
    let (t, home, root) = kb_home("vars");
    let (mut s, copied) = session(&t, &home, &root, (120, 40));
    find(&mut s, "5xx", "sumo/5xx-by-authorizer.md");
    s.send(ALT_C);
    s.wait_for("template");
    assert!(!s.screen().contains("variables ·"));
    assert!(fs::read_to_string(copied).unwrap().contains("{{env}}"));
    assert!(s.is_running());
}

#[test]
fn a_narrow_terminal_puts_the_preview_under_the_fields() {
    let (t, home, root) = kb_home("vars");
    let (mut s, _copied) = session(&t, &home, &root, (80, 30));
    s.wait_for("17 entries");
    s.type_text("5xx");
    s.wait_for("5xx by authorizer");
    s.wait_for("Esc clear");
    s.send(ENTER);
    form_is_up(&s);
    let rows = s.rows();
    let preview = rows
        .iter()
        .position(|r| r.contains("Preview"))
        .expect("a preview");
    let minutes = rows
        .iter()
        .position(|r| r.contains("minutes"))
        .expect("the minutes field");
    assert!(minutes < preview, "{}", s.screen());
}

#[test]
fn an_entry_edited_while_the_form_is_open_reopens_it_with_a_notice() {
    let (t, home, root) = kb_home("vars");
    let (mut s, copied) = session(&t, &home, &root, (120, 40));
    open_the_form(&mut s);
    fill(&mut s);
    // add a variable to the entry behind the UI's back
    let path = root.join("sumo/5xx-by-authorizer.md");
    let text = fs::read_to_string(&path).unwrap();
    let edited = text
        .replace(
            "  minutes:",
            "  region: { kind: text, default: eu }\n  minutes:",
        )
        .replace(
            "count by authorizer",
            "count by authorizer | where r = \"{{region}}\"",
        );
    fs::write(&path, edited).unwrap();
    s.send(ENTER);
    s.wait_for("The entry changed; review the fields");
    assert!(s.is_running());
    assert!(!copied.exists());
    s.wait_until("the new field", |screen| {
        screen.contains("region") && screen.contains("field 1/3")
    });
    // what was typed is still there
    assert!(s.screen().contains("timeslice 30m"));
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(fs::read_to_string(copied).unwrap().contains("r = \"eu\""));
}

#[test]
fn a_secret_from_the_environment_is_masked_and_a_typed_one_is_never_drawn() {
    let (t, home, root) = kb_home("vars");
    let copied = t.path().join("copied");
    let (tool, env) = native();
    let mut spec = Spec::new(&[], &home, &root)
        .copy_mode("auto")
        .env("KB_TEST_COPIED", copied.to_str().unwrap())
        .env("KB_TEST_TOKEN", "S3CR3T-MARKER")
        .with_fake_tool(tool, "cat > \"$KB_TEST_COPIED\"");
    for (k, v) in env {
        spec = spec.env(k, v);
    }
    let mut s = Session::spawn(&spec);
    find(&mut s, "Call the API", "cmd/curl-token.md");
    s.send(ENTER);
    form_is_up(&s);
    s.wait_for("(from $KB_TEST_TOKEN)");
    s.wait_for("Bearer ••••");
    assert!(!s.screen().contains("S3CR3T") && !s.raw_output().contains("S3CR3T"));
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(
        fs::read_to_string(&copied)
            .unwrap()
            .contains("Bearer S3CR3T-MARKER")
    );
    assert!(
        !fs::read_to_string(root.join(".kb/state.json"))
            .unwrap()
            .contains("S3CR3T")
    );
}

#[test]
fn ctrl_z_resets_the_focused_field() {
    let (t, home, root) = kb_home("vars");
    let (mut s, _copied) = session(&t, &home, &root, (120, 40));
    open_the_form(&mut s);
    s.send(TAB);
    s.type_text("9");
    let input = |s: &str| s.lines().any(|l| l.trim_start().starts_with("159"));
    s.wait_until("the edited value", input);
    s.send(CTRL_Z);
    s.wait_until("the prefilled value", |screen| {
        !input(screen)
            && screen
                .lines()
                .any(|l| l.trim_start().starts_with("15 ") || l.trim() == "15")
    });
}

#[test]
fn alt_down_offers_recent_values_for_a_text_field() {
    let (t, home, root) = kb_home("vars");
    // an earlier use of another entry left `name=api` behind
    fs::create_dir_all(root.join(".kb")).unwrap();
    fs::write(
        root.join(".kb/state.json"),
        r#"{"state_version":1,"usage":{},"vars":{"recent":{"name":["api","db"]}}}"#,
    )
    .unwrap();
    let (mut s, copied) = session(&t, &home, &root, (120, 40));
    find(&mut s, "Repeat a name", "cmd/repeat.md");
    s.send(ENTER);
    form_is_up(&s);
    s.wait_for("↳ api");
    s.send(ALT_DOWN);
    s.send(b"\x1b[B");
    s.send(ENTER);
    s.wait_for("echo db && echo db");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(fs::read_to_string(copied).unwrap(), "echo db && echo db");
}

#[test]
fn alt_enter_after_ctrl_y_prints_and_exits() {
    let (t, home, root) = kb_home("vars");
    let out = t.path().join("stdout");
    let (tool, env) = native();
    let mut spec = Spec::new(&[], &home, &root)
        .copy_mode("auto")
        .with_fake_tool(tool, "cat >/dev/null");
    for (k, v) in env {
        spec = spec.env(k, v);
    }
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    find(&mut s, "5xx", "sumo/5xx-by-authorizer.md");
    s.send(CTRL_Y);
    form_is_up(&s);
    fill(&mut s);
    s.send(ALT_ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(fs::read_to_string(out).unwrap(), format!("{QUERY}\n"));
}

#[test]
fn raw_copy_from_the_picker_copies_the_template_without_a_form() {
    let (t, home, root) = kb_home("vars");
    let copied = t.path().join("copied");
    let (tool, env) = native();
    let mut spec = Spec::new(&["copy", "--raw", "cmd"], &home, &root)
        .copy_mode("auto")
        .env("KB_TEST_COPIED", copied.to_str().unwrap())
        .with_fake_tool(tool, "cat > \"$KB_TEST_COPIED\"");
    for (k, v) in env {
        spec = spec.env(k, v);
    }
    let mut s = Session::spawn(&spec);
    s.wait_for("results");
    s.type_text(" Repeat a name");
    s.wait_for("cmd/repeat.md");
    s.wait_for("Esc clear");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(
        fs::read_to_string(copied).unwrap(),
        "echo {{name}} && echo {{name}}"
    );
}

/// SC-001: with the right values already filled in, an entry with variables takes one more key
/// press than one without.
#[test]
fn an_entry_with_good_prefills_takes_one_more_enter() {
    let (t, home, root) = kb_home("vars");
    let (mut s, copied) = session(&t, &home, &root, (120, 40));
    find(&mut s, "Repeat a name", "cmd/repeat.md");
    s.wait_for("Esc clear");
    s.send(ENTER);
    form_is_up(&s);
    assert!(s.is_running(), "the first Enter opens the form");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(fs::read_to_string(copied).unwrap(), "echo web && echo web");
}
