//! Integration tests for `kb run`: running cmd entries safely with confirmation,
//! destructive checks, secret isolation, and exit code forwarding.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use support::kb_in;
use support::pty::{ENTER, Session, Spec};

fn setup_run_kb() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let root = dir.path().join("kb");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&root).unwrap();

    let marker_path = dir.path().join("marker.txt");
    let marker_str = marker_path.display().to_string();

    // Create safe command entry
    fs::write(
        root.join("safe.md"),
        format!(
            r#"---
kb_format: 1
type: cmd
title: Safe Echo
shell: sh
---

```
echo "SAFE_EXECUTED" > '{marker_str}'
```
"#
        ),
    )
    .unwrap();

    // Create command exiting with code 42
    fs::write(
        root.join("exit42.md"),
        r#"---
kb_format: 1
type: cmd
title: Exit 42
shell: sh
---

```
exit 42
```
"#,
    )
    .unwrap();

    // Create command killed by SIGTERM
    fs::write(
        root.join("sigterm.md"),
        r#"---
kb_format: 1
type: cmd
title: Terminate by Signal
shell: sh
---

```
kill -TERM $$
```
"#,
    )
    .unwrap();

    // Create destructive command entry
    fs::write(
        root.join("destructive.md"),
        format!(
            r#"---
kb_format: 1
type: cmd
title: Destructive Clean
shell: sh
---

```
rm -rf /tmp/dangerous_path && echo "DESTRUCTIVE_EXECUTED" > '{marker_str}'
```
"#
        ),
    )
    .unwrap();

    // Create secret command entry
    fs::write(
        root.join("secret.md"),
        format!(
            r#"---
kb_format: 1
type: cmd
title: Command With Secret
shell: sh
vars:
  token:
    kind: secret
---

```
test "{{{{token}}}}" = "my_super_secret" && echo "SECRET_MATCHED" > '{marker_str}'
```
"#
        ),
    )
    .unwrap();

    (dir, home, root)
}

fn spawn_run(args: &[&str], home: &Path, root: &Path) -> Session {
    let spec = Spec::new(args, home, root);
    Session::spawn(&spec)
}

#[test]
fn cli_run_non_interactive_yes_executes_non_destructive_command() {
    let (dir, _home, root) = setup_run_kb();
    let marker = dir.path().join("marker.txt");

    let out = kb_in(&root)
        .arg("run")
        .arg("safe")
        .arg("-y")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("kb: running:"), "stderr: {stderr}");
    assert!(marker.exists(), "marker file should exist");
    let content = fs::read_to_string(marker).unwrap();
    assert!(content.contains("SAFE_EXECUTED"), "content: {content}");
}

#[test]
fn cli_run_non_interactive_yes_on_destructive_command_rejected_without_force() {
    let (dir, _home, root) = setup_run_kb();
    let marker = dir.path().join("marker.txt");

    let out = kb_in(&root)
        .arg("run")
        .arg("destructive")
        .arg("-y")
        .output()
        .unwrap();

    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--force"),
        "stderr should mention --force: {stderr}"
    );
    assert!(!marker.exists(), "marker should not exist before --force");

    // With --force it should succeed
    let out_force = kb_in(&root)
        .arg("run")
        .arg("destructive")
        .arg("-y")
        .arg("--force")
        .output()
        .unwrap();

    assert_eq!(out_force.status.code(), Some(0));
    assert!(marker.exists(), "marker should exist after --force");
}

#[test]
fn cli_run_forwards_child_exit_code() {
    let (_dir, _home, root) = setup_run_kb();

    let out = kb_in(&root)
        .arg("run")
        .arg("exit42")
        .arg("-y")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn cli_run_forwards_signal_exit_code() {
    let (_dir, _home, root) = setup_run_kb();

    let out = kb_in(&root)
        .arg("run")
        .arg("sigterm")
        .arg("-y")
        .output()
        .unwrap();

    // SIGTERM is signal 15 -> exit code 128 + 15 = 143
    assert_eq!(out.status.code(), Some(143));
}

#[test]
fn cli_run_isolates_secret_into_env_var() {
    let (dir, _home, root) = setup_run_kb();
    let marker = dir.path().join("marker.txt");

    let out = kb_in(&root)
        .arg("run")
        .arg("secret")
        .args(["--var", "token=my_super_secret", "-y"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(marker.exists(), "marker file should exist");
    let content = fs::read_to_string(marker).unwrap();
    assert!(content.contains("SECRET_MATCHED"), "content: {content}");
}

#[test]
fn cli_run_interactive_confirmation_runs_on_y() {
    let (dir, home, root) = setup_run_kb();
    let marker = dir.path().join("marker.txt");
    let mut s = spawn_run(&["run", "safe"], &home, &root);

    s.wait_for("Run this command? [y/N]: ");
    s.type_text("y");
    s.send(ENTER);

    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(marker.exists(), "marker should exist");
}

#[test]
fn cli_run_interactive_confirmation_cancels_on_n() {
    let (dir, home, root) = setup_run_kb();
    let marker = dir.path().join("marker.txt");
    let mut s = spawn_run(&["run", "safe"], &home, &root);

    s.wait_for("Run this command? [y/N]: ");
    s.type_text("n");
    s.send(ENTER);

    // Cancelled -> exit code 130
    assert_eq!(s.wait_exit(), 130, "{}", s.screen());
    assert!(!marker.exists(), "marker should not exist when cancelled");
}

#[test]
fn cli_run_destructive_requires_typing_run() {
    let (dir, home, root) = setup_run_kb();
    let marker = dir.path().join("marker.txt");

    // First try: typing 'y' should cancel
    let mut s1 = spawn_run(&["run", "destructive"], &home, &root);
    s1.wait_for("Type 'run' to execute: ");
    s1.type_text("y");
    s1.send(ENTER);
    assert_eq!(s1.wait_exit(), 130, "{}", s1.screen());
    assert!(!marker.exists(), "marker should not exist when typing 'y'");

    // Second try: typing 'run' should succeed
    let mut s2 = spawn_run(&["run", "destructive"], &home, &root);
    s2.wait_for("Type 'run' to execute: ");
    s2.type_text("run");
    s2.send(ENTER);
    assert_eq!(s2.wait_exit(), 0, "{}", s2.screen());
    assert!(marker.exists(), "marker should exist when typing 'run'");
}

#[test]
fn cli_run_custom_action_pipes_payload_via_stdin_and_interpolates_fields() {
    let (dir, home, root) = setup_run_kb();
    let out_file = dir.path().join("psql_out.txt");
    let out_str = out_file.display().to_string();

    let config_dir = home.join(".config/kb");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"
[[actions.sql]]
name = "psql-test"
key = "p"
command = "sh -c 'echo \"DB: $1\" > \"$2\"; cat >> \"$2\"' -- {{{{database}}}} '{out_str}'"
input = "stdin"
"#
        ),
    )
    .unwrap();

    fs::write(
        root.join("report.md"),
        r#"---
kb_format: 1
type: sql
title: Daily Report
database: analytics_prod
---

```sql
SELECT count(*) FROM events;
```
"#,
    )
    .unwrap();

    let out = kb_in(&root)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .args(["run", "report", "--action", "psql-test", "-y"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out_file.exists(), "out_file should exist");
    let content = fs::read_to_string(out_file).unwrap();
    assert!(content.contains("DB: analytics_prod"), "content: {content}");
    assert!(
        content.contains("SELECT count(*) FROM events;"),
        "content: {content}"
    );
}

#[test]
fn cli_run_custom_action_appends_arg_payload() {
    let (dir, home, root) = setup_run_kb();
    let out_file = dir.path().join("arg_out.txt");
    let out_str = out_file.display().to_string();

    let config_dir = home.join(".config/kb");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"
[[actions.sql]]
name = "echo-arg"
key = "e"
command = "sh -c 'echo \"PAYLOAD: $1\" > \"{out_str}\"' --"
input = "arg"
"#
        ),
    )
    .unwrap();

    fs::write(
        root.join("report.md"),
        r#"---
kb_format: 1
type: sql
title: Daily Report
database: analytics_prod
---

```sql
SELECT 42;
```
"#,
    )
    .unwrap();

    let out = kb_in(&root)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .args(["run", "report", "--action", "echo-arg", "-y"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out_file.exists(), "out_file should exist");
    let content = fs::read_to_string(out_file).unwrap();
    assert!(content.contains("PAYLOAD: SELECT 42;"), "content: {content}");
}

#[test]
fn cli_run_custom_action_applies_destructive_checks() {
    let (dir, home, root) = setup_run_kb();
    let marker = dir.path().join("destructive_marker.txt");
    let marker_str = marker.display().to_string();

    let config_dir = home.join(".config/kb");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"
[[actions.sql]]
name = "drop-all"
key = "d"
command = "rm -rf /tmp/fake_dangerous_path && echo 'EXECUTED' > '{marker_str}'"
"#
        ),
    )
    .unwrap();

    fs::write(
        root.join("report.md"),
        r#"---
kb_format: 1
type: sql
title: Daily Report
database: analytics_prod
---

```sql
DROP TABLE events;
```
"#,
    )
    .unwrap();

    // -y without --force should fail
    let out = kb_in(&root)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .args(["run", "report", "--action", "drop-all", "-y"])
        .output()
        .unwrap();

    assert_ne!(out.status.code(), Some(0));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("destructive"), "stderr: {err}");
    assert!(!marker.exists(), "marker should not exist without --force");

    // -y with --force should succeed
    let out_force = kb_in(&root)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .args(["run", "report", "--action", "drop-all", "-y", "--force"])
        .output()
        .unwrap();

    assert_eq!(
        out_force.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out_force.stderr)
    );
    assert!(marker.exists(), "marker should exist with --force");
}

