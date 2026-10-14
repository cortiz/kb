//! Integration tests for `kb open`: opening links, Sumo Logic and CloudWatch deep links.

mod support;

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use support::{fake_clipboard, kb_in, path_with};

fn actions_kb() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    support::copy_dir(&support::fixtures().join("actions/entries"), &root);
    (dir, root)
}

fn fake_opener(base: &Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = base.join("fake-opener-bin");
    fs::create_dir_all(&dir).unwrap();
    let opened = base.join("opened.log");
    for name in &["open", "xdg-open"] {
        let script = dir.join(name);
        fs::write(
            &script,
            format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 0\n", opened.display()),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    (dir, opened)
}

#[test]
fn open_link_launches_browser_and_records_use() {
    let (d, root) = actions_kb();
    let (opener_dir, opened_log) = fake_opener(d.path());

    let out = kb_in(&root)
        .arg("open")
        .arg("link-auth0")
        .env("PATH", path_with(&opener_dir))
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let opened = fs::read_to_string(opened_log).expect("opened.log exists");
    assert!(
        opened.contains("https://manage.auth0.com/dashboard/us/example/scopes"),
        "opened content: {opened}"
    );

    // Verify use recorded in state.json
    let state_bytes = fs::read(root.join(".kb/state.json")).expect("state.json exists");
    let state: Value = serde_json::from_slice(&state_bytes).unwrap();
    assert!(state["usage"]["link-auth0"].is_object(), "state: {state}");
}

#[test]
fn open_sumo_with_configured_base_url_and_resolved_vars() {
    let (d, root) = actions_kb();
    let (opener_dir, opened_log) = fake_opener(d.path());

    let out = kb_in(&root)
        .arg("open")
        .arg("sumo-5xx")
        .args(["--var", "env=prod", "--var", "minutes=30"])
        .env("PATH", path_with(&opener_dir))
        .env("KB_SUMO_BASE_URL", "https://service.us2.sumologic.com")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let opened = fs::read_to_string(opened_log).expect("opened.log exists");
    assert!(
        opened.contains("https://service.us2.sumologic.com/ui/index.html#section/search/"),
        "opened content: {opened}"
    );
    assert!(
        opened.contains("_sourceCategory%3Dprod%2Flambda%2Fauthorizer"),
        "query encoded: {opened}"
    );
}

#[test]
fn open_sumo_missing_base_url_copies_query_and_explains() {
    let (d, root) = actions_kb();
    let (opener_dir, opened_log) = fake_opener(d.path());
    let (_clip_name, clip_dir, clip_file, clip_env) = fake_clipboard(d.path());

    let mut combined_paths = vec![clip_dir, opener_dir];
    combined_paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path_val = std::env::join_paths(combined_paths).unwrap();

    let mut cmd = kb_in(&root);
    cmd.arg("open")
        .arg("sumo-5xx")
        .args(["--var", "env=prod", "--var", "minutes=30"])
        .env("PATH", path_val)
        .env_remove("KB_SUMO_BASE_URL");
    for (k, v) in clip_env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("sumo.base_url"),
        "stderr should explain missing sumo.base_url: {stderr}"
    );

    // Browser was NOT opened
    assert!(!opened_log.exists() || fs::read_to_string(&opened_log).unwrap().is_empty());

    // Query WAS copied to clipboard
    let copied = fs::read_to_string(&clip_file).expect("query copied to clipboard");
    assert!(
        copied.contains("_sourceCategory=prod/lambda/authorizer"),
        "copied: {copied}"
    );
}

#[test]
fn open_cw_with_log_groups_and_resolved_range() {
    let (d, root) = actions_kb();
    let (opener_dir, opened_log) = fake_opener(d.path());

    let out = kb_in(&root)
        .arg("open")
        .arg("cw-latency")
        .args(["--var", "ms=250"])
        .env("PATH", path_with(&opener_dir))
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let opened = fs::read_to_string(opened_log).expect("opened.log exists");
    assert!(
        opened.contains("https://us-east-1.console.aws.amazon.com/cloudwatch/home?region=us-east-1#logsV2:logs-insights$3FqueryDetail$3D~("),
        "opened content: {opened}"
    );
    assert!(
        opened.contains("start~'-1800'"),
        "range -30m converted to -1800s: {opened}"
    );
    assert!(
        opened.contains("source~(~'/aws/lambda/prod-authorizer'~'/aws/lambda/shared-auth')"),
        "log groups present: {opened}"
    );
}

#[test]
fn open_ssh_fallback_prints_and_copies_url() {
    let (d, root) = actions_kb();
    let (opener_dir, opened_log) = fake_opener(d.path());
    let (clip_name, clip_dir, clip_file, _clip_env) = fake_clipboard(d.path());
    let clip_prog = clip_dir.join(clip_name);

    let mut combined_paths = vec![clip_dir, opener_dir];
    combined_paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path_val = std::env::join_paths(combined_paths).unwrap();

    let mut cmd = kb_in(&root);
    cmd.arg("open")
        .arg("link-auth0")
        .env("PATH", path_val)
        .env("SSH_CONNECTION", "1.2.3.4 22 5.6.7.8 22")
        .env("KB_CLIPBOARD_COMMAND", clip_prog.display().to_string())
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY");
    let out = cmd.output().unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("https://manage.auth0.com/dashboard/us/example/scopes"),
        "stdout should print url: {stdout}"
    );

    // Browser was NOT launched
    assert!(!opened_log.exists() || fs::read_to_string(&opened_log).unwrap().is_empty());

    // URL WAS copied to clipboard
    let copied = fs::read_to_string(&clip_file).expect("URL copied to clipboard");
    assert!(
        copied.contains("https://manage.auth0.com/dashboard/us/example/scopes"),
        "copied: {copied}"
    );
}

#[test]
fn open_unknown_entry_exits_1() {
    let (_d, root) = actions_kb();
    let out = kb_in(&root)
        .arg("open")
        .arg("nonexistent-entry")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no entry matches"), "stderr: {stderr}");
}
