//! PTY integration tests for TUI Action Menu modal (Alt-A / F2).

mod support;

use std::fs;
use std::path::PathBuf;
use support::pty::{CTRL_C, ESC, Session, Spec};

const ALT_A: &[u8] = b"\x1ba";
const F2: &[u8] = b"\x1bOQ";

fn setup_action_pty_env() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let kb_root = dir.path().join("kb");
    let home = dir.path().join("home");
    fs::create_dir_all(&kb_root).unwrap();
    fs::create_dir_all(home.join(".config/kb")).unwrap();

    support::copy_dir(&support::fixtures().join("actions/entries"), &kb_root);

    (dir, kb_root, home)
}

#[test]
fn tui_action_menu_alt_a_and_f2_and_esc() {
    let (_dir, kb_root, home) = setup_action_pty_env();

    let mut spec = Spec::new(&["5xx"], &home, &kb_root);
    spec.size = (100, 30);
    spec.allow_clipboard = true;

    let mut s = Session::spawn(&spec);
    s.wait_for("5xx by authorizer");

    // 1. Press Alt-A: opens Action Menu modal
    s.send(ALT_A);
    s.wait_for("Actions: 5xx by authorizer (sumo)");
    s.wait_for("[o] Open in Sumo");
    s.wait_for("[c] Copy Query");
    s.wait_for("Esc to close");

    // 2. Press Esc: closes Action Menu modal
    s.send(ESC);
    s.wait_for("5xx by authorizer");

    // 3. Press F2: also opens Action Menu modal
    s.send(F2);
    s.wait_for("Actions: 5xx by authorizer (sumo)");

    // 4. Press 'c' to copy: runs action and closes modal
    s.send(b"c");
    s.wait_for("5xx by authorizer");

    s.send(CTRL_C);
}

#[test]
fn tui_action_menu_on_code_opens_code_view() {
    let (_dir, kb_root, home) = setup_action_pty_env();

    let mut spec = Spec::new(&["AuthorizerHandler"], &home, &kb_root);
    spec.size = (100, 30);

    let mut s = Session::spawn(&spec);
    s.wait_for("AuthorizerHandler.handleRequest");

    // Press Alt-A on code entry: directly opens Code View (Screen 3) per US5 Scenario 3
    s.send(ALT_A);
    s.wait_for("AuthorizerHandler.handleRequest (code)");
    s.wait_for("Repo: auth0/auth0-java");

    // Esc returns to main list
    s.send(ESC);
    s.wait_for("AuthorizerHandler.handleRequest");

    s.send(CTRL_C);
}

fn native_clipboard() -> (&'static str, Vec<(&'static str, &'static str)>) {
    if cfg!(target_os = "macos") {
        ("pbcopy", vec![])
    } else {
        ("wl-copy", vec![("WAYLAND_DISPLAY", "wayland-test")])
    }
}

/// A KB with the sample notes and the entry one of them links to.
fn notes_pty_env() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (dir, kb_root, home) = setup_action_pty_env();
    support::add_notes(&kb_root);
    fs::create_dir_all(kb_root.join("sumo")).unwrap();
    fs::copy(
        support::fixtures().join("search/sumo/5xx-by-authorizer.md"),
        kb_root.join("sumo/5xx-by-authorizer.md"),
    )
    .unwrap();
    (dir, kb_root, home)
}

fn note_menu(
    dir: &tempfile::TempDir,
    home: &std::path::Path,
    root: &std::path::Path,
) -> (Session, PathBuf) {
    let copied = dir.path().join("copied");
    let (tool, env) = native_clipboard();
    let mut spec = Spec::new(&["role", "cache", "ttl"], home, root)
        .copy_mode("auto")
        .env("KB_TEST_COPIED", copied.to_str().unwrap())
        .with_fake_tool(tool, "cat > \"$KB_TEST_COPIED\"");
    for (k, v) in env {
        spec = spec.env(k, v);
    }
    spec.size = (120, 40);
    let mut s = Session::spawn(&spec);
    s.wait_for("Role cache TTL and Redis architecture");
    s.send(ALT_A);
    s.wait_for("[t] Copy Text");
    (s, copied)
}

#[test]
fn a_note_offers_markdown_text_and_edit() {
    let (dir, root, home) = notes_pty_env();
    let (mut s, _) = note_menu(&dir, &home, &root);
    s.wait_for("[c] Copy Markdown");
    s.wait_for("[t] Copy Text");
    s.wait_for("[e] Edit");
    s.send(ESC);
    s.send(CTRL_C);
}

#[test]
fn copy_markdown_copies_the_source_and_copy_text_copies_what_is_drawn() {
    let (dir, root, home) = notes_pty_env();
    let (mut s, copied) = note_menu(&dir, &home, &root);
    s.send(b"c");
    s.wait_until("the clipboard", |_| {
        copied.exists() && fs::metadata(&copied).map_or(0, |m| m.len()) > 0
    });
    let source = fs::read_to_string(&copied).unwrap();
    assert!(
        source.starts_with("# Role cache TTL and Redis architecture"),
        "{source}"
    );
    assert!(
        source.contains("[[sumo/5xx-by-authorizer]]") && source.contains("| Event |"),
        "{source}"
    );
    assert!(!source.contains("kb_format"), "no frontmatter");
    fs::remove_file(&copied).unwrap();
    s.wait_for("Copied");
    s.send(ALT_A);
    s.wait_for("[t] Copy Text");
    s.send(b"t");
    s.wait_until("the clipboard", |_| {
        copied.exists() && fs::metadata(&copied).map_or(0, |m| m.len()) > 0
    });
    let text = fs::read_to_string(&copied).unwrap();
    for want in [
        "[sumo] 5xx by authorizer",
        "┌─",
        "☐ Check the hit ratio",
        "▌ ⚠ Warning",
    ] {
        assert!(text.contains(want), "{want:?} missing:\n{text}");
    }
    assert!(
        !text.contains("[[") && !text.contains("**") && !text.contains("```"),
        "{text}"
    );
    assert!(!text.contains('\x1b'), "no styling in plain text");
    s.send(CTRL_C);
}

#[test]
fn edit_opens_the_note_in_the_editor() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, root, home) = notes_pty_env();
    let log = dir.path().join("edited");
    let editor = dir.path().join("ed");
    fs::write(
        &editor,
        format!("#!/bin/sh\necho \"$1\" > '{}'\n", log.display()),
    )
    .unwrap();
    fs::set_permissions(&editor, fs::Permissions::from_mode(0o755)).unwrap();
    let mut spec = Spec::new(&["role", "cache", "ttl"], &home, &root)
        .env("KB_EDITOR", editor.to_str().unwrap());
    spec.size = (120, 40);
    let mut s = Session::spawn(&spec);
    s.wait_for("Role cache TTL and Redis architecture");
    s.send(ALT_A);
    s.wait_for("[e] Edit");
    s.send(b"e");
    s.wait_until("the editor to run", |_| {
        log.exists() && fs::metadata(&log).map_or(0, |m| m.len()) > 0
    });
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .trim()
            .ends_with("note/role-cache-ttl.md")
    );
    s.send(CTRL_C);
}

fn linking_note(root: &std::path::Path, name: &str, body: &str) {
    fs::write(
        root.join(format!("note/{name}.md")),
        format!(
            "---\nkb_format: 1\ntype: note\ntitle: {name} note\n---\n\n# {name} note\n\n{body}\n"
        ),
    )
    .unwrap();
}

fn open_menu_for(query: &[&str], home: &std::path::Path, root: &std::path::Path) -> Session {
    let mut spec = Spec::new(query, home, root);
    spec.size = (120, 40);
    let mut s = Session::spawn(&spec);
    s.wait_until("one result", |screen| screen.contains("1 / 1")); // selected and previewed
    s.wait_for("━━━"); // the note's preview is drawn
    s.send(ALT_A);
    s.wait_for("[t] Copy Text");
    s
}

#[test]
fn follow_entry_link_with_one_link_goes_straight_to_the_entry() {
    let (_dir, root, home) = notes_pty_env();
    linking_note(
        &root,
        "single",
        "See [[sumo/5xx-by-authorizer]] for the query.",
    );
    let mut s = open_menu_for(&["single", "note"], &home, &root);
    s.wait_for("[f] Follow entry link");
    s.send(b"f");
    // the search became the target's title and the target is selected and previewed
    s.wait_until("the linked entry", |screen| {
        screen.contains("› 5xx by authorizer") && screen.contains("_sourceCategory")
    });
    assert!(!s.screen().contains("Follow entry link"), "{}", s.screen());
    s.send(CTRL_C);
}

#[test]
fn follow_entry_link_with_several_links_opens_a_picker() {
    let (_dir, root, home) = notes_pty_env();
    linking_note(
        &root,
        "several",
        "Query [[sumo/5xx-by-authorizer]], latency [[cw-latency]], and a stale one [[gone/entry]].",
    );
    let mut s = open_menu_for(&["several", "note"], &home, &root);
    s.send(b"f");
    s.wait_for("Follow entry link ");
    s.wait_for("[sumo] 5xx by authorizer");
    s.wait_for("[cw]");
    s.wait_for("[?] gone/entry");
    s.wait_for("↑↓ choose");
    // Esc closes it without moving
    s.send(ESC);
    s.wait_until("the picker to close", |screen| {
        !screen.contains("↑↓ choose")
    });
    assert!(s.screen().contains("› several note"), "{}", s.screen());
    // choose the second one
    s.send(ALT_A);
    s.wait_for("[f] Follow entry link");
    s.send(b"f");
    s.wait_for("↑↓ choose");
    s.send(b"\x1b[B"); // Down
    s.send(b"\r");
    s.wait_until("the cw entry", |screen| {
        screen.contains("› ") && !screen.contains("↑↓ choose") && screen.contains("cw")
    });
    let rows = s.rows();
    assert!(
        rows[1].contains('›') && !rows[1].contains("several"),
        "{}",
        rows[1]
    );
    s.send(CTRL_C);
}

#[test]
fn a_broken_link_says_so_and_a_note_without_links_has_no_such_action() {
    let (_dir, root, home) = notes_pty_env();
    linking_note(&root, "broken", "Only [[gone/entry]] here.");
    let mut s = open_menu_for(&["broken", "note"], &home, &root);
    s.send(b"f");
    s.wait_for("No entry named gone/entry");
    s.send(CTRL_C);
    linking_note(&root, "plain", "No links at all.");
    let mut s = open_menu_for(&["plain", "note"], &home, &root);
    s.wait_for("[e] Edit");
    assert!(!s.screen().contains("Follow entry link"), "{}", s.screen());
    s.send(CTRL_C);
}
