//! PTY integration tests for running commands safely from the TUI (Ctrl-G).

mod support;

use std::fs;
use std::path::PathBuf;
use support::pty::{CTRL_C, ENTER, Session, Spec};

const CTRL_G: &[u8] = b"\x07";

fn setup_tui_run_kb() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let root = dir.path().join("kb");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&root).unwrap();

    let marker_path = dir.path().join("marker.txt");
    let marker_str = marker_path.display().to_string();

    fs::write(
        root.join("cmd-safe.md"),
        format!(
            r#"---
kb_format: 1
type: cmd
title: Safe Echo Command
shell: sh
---

```
echo "SAFE_EXECUTED" > '{marker_str}'
```
"#
        ),
    )
    .unwrap();

    fs::write(
        root.join("cmd-dest.md"),
        format!(
            r#"---
kb_format: 1
type: cmd
title: Destructive Clean Command
shell: sh
---

```
rm -rf /tmp/test_dangerous && echo "DESTRUCTIVE_EXECUTED" > '{marker_str}'
```
"#
        ),
    )
    .unwrap();

    (dir, home, root)
}

#[test]
fn tui_run_safe_command_confirmation_pause_and_restore() {
    let (dir, home, root) = setup_tui_run_kb();
    let marker = dir.path().join("marker.txt");

    let spec = Spec::new(&[], &home, &root);
    let mut s = Session::spawn(&spec);

    s.wait_for("Safe Echo Command");
    // Filter to select safe command
    s.type_text("safe");
    s.wait_for("cmd-safe.md");

    // Press Ctrl-G to run
    s.send(CTRL_G);

    // Modal appears with confirmation details
    s.wait_for("Run Command");
    s.wait_for("Command:");
    s.wait_for("Shell:");
    s.wait_for("Press Enter or 'y' to run");

    // Confirm execution
    s.send(ENTER);

    // Post-run pause screen shows exit code
    s.wait_for("exit code 0");
    s.wait_for("Press any key to continue");

    assert!(marker.exists(), "marker should exist after execution");

    // Press Enter to return to TUI list
    s.send(ENTER);

    // Verify returned to main TUI list cleanly
    s.wait_for("Safe Echo Command");
    assert!(
        s.alternate_screen(),
        "terminal should remain in alternate screen"
    );

    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn tui_run_destructive_command_requires_typing_run() {
    let (dir, home, root) = setup_tui_run_kb();
    let marker = dir.path().join("marker.txt");

    let spec = Spec::new(&[], &home, &root);
    let mut s = Session::spawn(&spec);

    s.wait_for("cmd-dest");
    // Filter to select cmd-dest
    s.type_text("dest");
    s.wait_for("cmd-dest");

    s.send(CTRL_G);

    // Destructive modal requires typing 'run'
    s.wait_for("WARNING");
    s.wait_for("Type 'run' to execute:");

    // Typing wrong text and enter does not execute
    s.type_text("no");
    s.send(ENTER);
    assert!(!marker.exists(), "marker should not exist");

    // Backspace twice and type 'run'
    s.send(b"\x7f\x7f");
    s.type_text("run");
    s.send(ENTER);

    s.wait_for("exit code 0");
    assert!(marker.exists(), "marker should exist after typing run");

    s.send(ENTER);
    s.wait_for("cmd-dest");

    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}
