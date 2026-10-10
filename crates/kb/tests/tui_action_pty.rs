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
