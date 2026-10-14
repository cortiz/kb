//! PTY integration test for shell integration and inline picker.

mod support;

use std::fs;
use support::pty::*;

#[test]
fn test_widget_normal_screen_rendering_and_clean_exit() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("widget_out.json");

    let mut spec = Spec::new(
        &[
            "pick", "--widget", "--shell", "zsh", "--buffer", "tail", "--cursor", "4",
        ],
        &home,
        &root,
    );
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);

    // Should NOT enter alternate screen (normal buffer rendering)
    s.wait_for("Search");
    assert!(
        !s.alternate_screen(),
        "inline picker must not use alternate screen"
    );

    // Press enter on the first entry (has variables, opens form)
    s.send(ENTER);
    s.wait_for("Variables");

    // Press enter on form to pick
    s.send(ENTER);

    assert_eq!(s.wait_exit(), 0);

    // Verify stdout received JSON IPC payload
    let output = fs::read_to_string(&out).expect("failed to read widget output");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("output must be valid JSON");
    assert_eq!(parsed["status"], "picked");
    assert!(parsed["command"].is_string());
    assert!(parsed["cursor"].is_number());
}

#[test]
fn test_widget_cancellation_exits_cleanly() {
    let (_t, home, root) = kb_home("search");

    let spec = Spec::new(&["pick", "--widget", "--shell", "zsh"], &home, &root);
    let mut s = Session::spawn(&spec);

    s.wait_for("Search");
    assert!(!s.alternate_screen());

    s.send(ESC);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn test_widget_esc_in_form_returns_to_search() {
    let (_t, home, root) = kb_home("search");

    let spec = Spec::new(
        &["pick", "--widget", "--shell", "zsh", "--buffer", "tail"],
        &home,
        &root,
    );
    let mut s = Session::spawn(&spec);

    s.wait_for("Search");
    // Enter into variable form (kubectl-pods has variables)
    s.send(ENTER);
    s.wait_for("Variables");

    // ESC in form returns to search
    s.send(ESC);
    s.wait_for("Search");

    // ESC again cancels picker
    s.send(ESC);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn test_widget_pick_entry_without_variables_direct() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("widget_out.json");

    let mut spec = Spec::new(
        &["pick", "--widget", "--shell", "zsh", "--buffer", "redis"],
        &home,
        &root,
    );
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);

    s.wait_for("Search");
    // Single ENTER picks entry without variables directly
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);

    let output = fs::read_to_string(&out).expect("failed to read widget output");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("output must be valid JSON");
    assert_eq!(parsed["status"], "picked");
    assert!(parsed["command"].as_str().unwrap().contains("redis-cli"));
}

#[test]
fn test_widget_secret_with_env_resolves_to_reference() {
    let (t, home, root) = kb_home("vars");
    let out = t.path().join("widget_out.json");

    let mut spec = Spec::new(
        &[
            "pick",
            "--widget",
            "--shell",
            "zsh",
            "--buffer",
            "curl-token",
        ],
        &home,
        &root,
    );
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);

    s.wait_for("Search");
    s.send(ENTER);
    s.wait_for("Variables");
    s.wait_for("from $KB_TEST_TOKEN");

    // Submit form
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);

    let output = fs::read_to_string(&out).expect("failed to read widget output");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("output must be valid JSON");
    assert_eq!(parsed["status"], "picked");
    let cmd = parsed["command"].as_str().unwrap();
    assert!(
        cmd.contains("$KB_TEST_TOKEN"),
        "command should reference env var: {cmd}"
    );
}

#[test]
fn test_widget_secret_without_env_refuses_insertion() {
    let (t, home, root) = kb_home("vars");
    let out = t.path().join("widget_out.json");

    // Create entry with secret lacking env source
    let entry_path = root.join("cmd/secret-no-env.md");
    let content = r#"---
kb_format: 1
type: cmd
title: Secret without env
shell: bash
vars:
  pass: { kind: secret }
---

```
mycli --password {{pass}}
```
"#;
    fs::write(entry_path, content).unwrap();

    let mut spec = Spec::new(
        &[
            "pick",
            "--widget",
            "--shell",
            "zsh",
            "--buffer",
            "secret-no-env",
        ],
        &home,
        &root,
    );
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);

    s.wait_for("Search");
    s.send(ENTER);
    s.wait_for("Variables");
    s.wait_for("no env source");

    // Submit form -> must refuse
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 1);

    let output = fs::read_to_string(&out).expect("failed to read widget output");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("output must be valid JSON");
    assert_eq!(parsed["status"], "refused");
    assert!(parsed["message"].as_str().unwrap().contains("use kb run"));
}

#[test]
fn test_widget_small_terminal_falls_back_to_alternate_screen() {
    let (_t, home, root) = kb_home("search");

    let mut spec = Spec::new(&["pick", "--widget", "--shell", "zsh"], &home, &root);
    spec.size = (80, 6); // very small terminal: < 8 lines
    let mut s = Session::spawn(&spec);

    s.wait_for("Search");
    assert!(
        s.alternate_screen(),
        "small terminal must use alternate screen"
    );

    s.send(ESC);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn test_widget_multiline_buffer_preservation() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("widget_out.json");

    let multiline_buffer = "echo hello\necho world";
    let mut spec = Spec::new(
        &[
            "pick",
            "--widget",
            "--shell",
            "zsh",
            "--buffer",
            multiline_buffer,
            "--cursor",
            "10",
        ],
        &home,
        &root,
    );
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);
    s.wait_for("Search");
    s.send(ESC);
    assert_eq!(s.wait_exit(), 130);

    let output = fs::read_to_string(&out).expect("failed to read widget output");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("output must be valid JSON");
    assert_eq!(parsed["status"], "cancelled");
}

#[test]
fn test_shell_init_includes_vi_keymaps() {
    let script_zsh = kb_core::shell::templates::generate_shell_init(
        kb_core::shell::ShellDialect::Zsh,
        r"\ek",
        r"\eK",
    );
    assert!(script_zsh.contains("-M vicmd"));
    assert!(script_zsh.contains("-M viins"));

    let script_fish = kb_core::shell::templates::generate_shell_init(
        kb_core::shell::ShellDialect::Fish,
        r"\ek",
        r"\eK",
    );
    assert!(script_fish.contains("-M insert"));
}

#[test]
fn test_widget_unreviewed_cmd_requires_y_confirmation() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("widget_out.json");

    let entry_path = root.join("cmd/agent-unrev.md");
    let content = r#"---
kb_format: 1
type: cmd
title: Agent Unreviewed Cmd
shell: sh
---

```
echo 'agent-danger'
```
"#;
    fs::write(&entry_path, content).unwrap();
    kb_core::mcp::review::ReviewStateTracker::mark_unreviewed(&root, "cmd/agent-unrev").unwrap();

    let mut spec = Spec::new(
        &[
            "pick",
            "--widget",
            "--shell",
            "zsh",
            "--buffer",
            "agent-unrev",
        ],
        &home,
        &root,
    );
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);
    s.wait_for("Search");
    s.send(ENTER);
    s.wait_for("Unreviewed Agent Change");
    s.wait_for("Insert this command? [y/N]");

    // Send y to confirm
    s.send(b"y");
    assert_eq!(s.wait_exit(), 0);

    let output = fs::read_to_string(&out).expect("failed to read widget output");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("output must be valid JSON");
    assert_eq!(parsed["status"], "picked");
    assert!(parsed["command"].as_str().unwrap().contains("agent-danger"));
}
