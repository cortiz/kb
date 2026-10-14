//! Integration tests for standalone `kb pick` CLI execution.

mod support;

use std::fs;
use support::pty::*;

#[test]
fn test_pick_standalone_resolved_mode() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("pick_out.txt");

    let mut spec = Spec::new(&["pick", "restart-worker"], &home, &root);
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);
    s.wait_for("Search");
    s.send(ENTER);

    assert_eq!(s.wait_exit(), 0);
    let output = fs::read_to_string(&out).expect("failed to read output");
    assert_eq!(output.trim(), "systemctl restart worker");
}

#[test]
fn test_pick_standalone_id_mode() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("pick_out.txt");

    let mut spec = Spec::new(&["pick", "restart-worker", "--print", "id"], &home, &root);
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);
    s.wait_for("Search");
    s.send(ENTER);

    assert_eq!(s.wait_exit(), 0);
    let output = fs::read_to_string(&out).expect("failed to read output");
    assert_eq!(output.trim(), "cmd/restart-worker");
}

#[test]
fn test_pick_standalone_raw_mode() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("pick_out.txt");

    let mut spec = Spec::new(&["pick", "deploy-service", "--print", "raw"], &home, &root);
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);
    s.wait_for("Search");
    s.send(ENTER);
    s.wait_for("Variables");
    s.send(ENTER);

    assert_eq!(s.wait_exit(), 0);
    let output = fs::read_to_string(&out).expect("failed to read output");
    assert_eq!(
        output.trim(),
        "./deploy.sh --env {{env}} --region {{region}}"
    );
}

#[test]
fn test_pick_standalone_payload_mode() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("pick_out.txt");

    let mut spec = Spec::new(
        &["pick", "restart-worker", "--print", "payload"],
        &home,
        &root,
    );
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);
    s.wait_for("Search");
    s.send(ENTER);

    assert_eq!(s.wait_exit(), 0);
    let output = fs::read_to_string(&out).expect("failed to read output");
    assert_eq!(output.trim(), "systemctl restart worker");
}

#[test]
fn test_pick_standalone_cancelled() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("pick_out.txt");

    let mut spec = Spec::new(&["pick", "restart-worker"], &home, &root);
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);
    s.wait_for("Search");
    s.send(ESC);

    assert_eq!(s.wait_exit(), 130);
    let output = fs::read_to_string(&out).unwrap_or_default();
    assert_eq!(output.trim(), "");
}

#[test]
fn test_pick_type_and_tag_filtering() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("pick_out.txt");

    let mut spec = Spec::new(
        &["pick", "--type", "cmd", "--tag", "ops", "--print", "id"],
        &home,
        &root,
    );
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);
    s.wait_for("Search");
    s.send(ENTER);

    assert_eq!(s.wait_exit(), 0);
    let output = fs::read_to_string(&out).expect("failed to read output");
    assert_eq!(output.trim(), "cmd/restart-worker");
}

#[test]
fn test_pick_tmux_outside_falls_back_gracefully() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("pick_out.txt");

    let mut spec = Spec::new(
        &["pick", "restart-worker", "--tmux", "--print", "id"],
        &home,
        &root,
    );
    spec.out = Some(out.clone());

    let mut s = Session::spawn(&spec);
    s.wait_for("Search");
    s.send(ENTER);

    assert_eq!(s.wait_exit(), 0);
    let output = fs::read_to_string(&out).expect("failed to read output");
    assert_eq!(output.trim(), "cmd/restart-worker");
}
