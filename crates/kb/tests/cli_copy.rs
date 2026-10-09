//! `kb copy`: the clipboard from the shell, with fake clipboard programs so nothing real is touched.

mod support;

use std::fs;

use kb_core::entry_json::PrintMode;
use kb_core::search::Kb;
use serde_json::Value;
use support::{fake_clipboard, kb_in, kb_without_terminal, path_with, search_kb};

const ID: &str = "sumo/5xx-by-authorizer";

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

fn lines_label(n: usize) -> String {
    format!("{n} line{}", if n == 1 { "" } else { "s" })
}

/// `kb --root <root> copy <args>` with the fake clipboard program first on `PATH`.
fn copy_with_fake(
    base: &std::path::Path,
    root: &std::path::Path,
    args: &[&str],
) -> (std::process::Output, std::path::PathBuf, &'static str) {
    let (tool, dir, copied, env) = fake_clipboard(base);
    let mut cmd = kb_in(root);
    cmd.arg("copy").args(args).env("PATH", path_with(&dir));
    for (k, v) in env {
        cmd.env(k, v);
    }
    (cmd.output().unwrap(), copied, tool)
}

fn payload(root: &std::path::Path, id: &str) -> String {
    Kb::open(root)
        .unwrap()
        .pick(id, PrintMode::Payload)
        .unwrap()
        .trim_end_matches('\n')
        .to_string()
}

#[test]
fn an_exact_id_is_copied_and_confirmed_on_stderr() {
    let (d, root) = search_kb();
    let (out, copied, tool) = copy_with_fake(d.path(), &root, &[ID]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(fs::read_to_string(copied).unwrap(), payload(&root, ID));
    assert_eq!(text(&out.stdout), "");
    let lines = payload(&root, ID).lines().count();
    assert!(
        text(&out.stderr).contains(&format!("Copied {ID} ({}) via {tool}", lines_label(lines))),
        "{}",
        text(&out.stderr)
    );
    let state: Value =
        serde_json::from_slice(&fs::read(root.join(".kb/state.json")).unwrap()).unwrap();
    assert_eq!(state["usage"][ID]["count"], 1);
}

#[test]
fn a_query_with_a_clear_winner_is_copied_and_the_id_is_shown() {
    let (d, root) = search_kb();
    let (out, copied, _) = copy_with_fake(d.path(), &root, &["5xx", "auth"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(fs::read_to_string(copied).unwrap(), payload(&root, ID));
    assert!(
        text(&out.stderr).contains(&format!("Copied {ID}")),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn links_and_code_copy_the_url_and_path_line() {
    let (d, root) = search_kb();
    let (out, copied, _) = copy_with_fake(d.path(), &root, &["link/payments-runbook"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        fs::read_to_string(&copied).unwrap(),
        "https://wiki.example.com/runbooks/payments"
    );
    let (out, copied, _) = copy_with_fake(d.path(), &root, &["code/request-handler"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(fs::read_to_string(copied).unwrap(), "src/handler.rs:10");
}

#[test]
fn close_matches_without_a_terminal_are_listed_and_nothing_is_copied() {
    let (d, root) = search_kb();
    let (_tool, dir, copied, env) = fake_clipboard(d.path());
    let path = path_with(&dir);
    let mut vars: Vec<(&str, &str)> = env;
    let path = path.to_str().unwrap().to_string();
    vars.push(("PATH", &path));
    let out = kb_without_terminal(
        &["--root", root.to_str().unwrap(), "copy", "latency"],
        &vars,
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let err = text(&out.stderr);
    let candidates: Vec<&str> = err.lines().filter(|l| l.contains('\t')).collect();
    assert!(
        (2..=5).contains(&candidates.len()),
        "up to five `id<TAB>title` lines: {err}"
    );
    assert!(
        candidates.iter().all(|l| l.split('\t').count() == 2),
        "{err}"
    );
    assert!(!copied.exists(), "nothing was copied");
    assert!(!root.join(".kb/state.json").exists());
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn nothing_found_is_exit_1() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["copy", "zzzznotaword"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("no entry matches \"zzzznotaword\""),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn the_method_flag_overrides_the_config_for_one_run() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["copy", ID, "--method", "stdout"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        payload(&root, ID),
        "printed without a newline added"
    );
    assert!(
        text(&out.stderr).contains(&format!("Printed {ID}"))
            && text(&out.stderr).contains("to stdout"),
        "{}",
        text(&out.stderr)
    );
    let bad = kb_in(&root)
        .args(["copy", ID, "--method", "bogus"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
}

#[test]
fn print_is_a_usage_error_on_copy() {
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["copy", ID, "--print", "id"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn json_describes_what_happened() {
    let (d, root) = search_kb();
    let (out, _copied, tool) = copy_with_fake(d.path(), &root, &[ID, "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["id"], ID);
    assert_eq!(doc["method"], tool);
    assert_eq!(doc["confirmed"], true);
    assert_eq!(doc["sent"], true);
    assert_eq!(doc["bytes"], payload(&root, ID).len());
    assert_eq!(doc["attempts"], serde_json::json!([]));
    // with the stdout method the text is inside the document, not printed raw
    let out = kb_in(&root)
        .args(["copy", ID, "--json", "--method", "stdout"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["method"], "stdout");
    assert_eq!(doc["text"], payload(&root, ID));
    assert_eq!(doc["confirmed"], false);
}

#[test]
fn a_payload_over_the_limit_is_refused_with_exit_2_and_no_use() {
    let (_d, root) = search_kb();
    // over ssh the only methods are the terminal and stdout; the size check happens before any
    // write to the terminal, so nothing escapes to the developer's terminal
    let out = kb_in(&root)
        .args(["copy", ID])
        .env("SSH_CONNECTION", "1.2.3.4 22 5.6.7.8 22")
        .env("KB_CLIPBOARD_OSC52_MAX_BYTES", "10")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(text(&out.stdout), "", "the payload is not printed");
    let err = text(&out.stderr);
    assert!(err.contains("over clipboard.osc52_max_bytes (10)"), "{err}");
    assert!(err.contains(&format!("kb show --raw {ID}")), "{err}");
    assert!(!root.join(".kb/state.json").exists(), "no use recorded");
}

#[test]
fn a_bad_setting_warns_once_and_copy_still_works() {
    let (d, root) = search_kb();
    let home = d.path().join("home");
    let cfg = home.join(".config/kb");
    fs::create_dir_all(&cfg).unwrap();
    fs::write(
        cfg.join("config.toml"),
        "bogus = 1\n[clipboard]\nmethod = \"xclipp\"\n",
    )
    .unwrap();
    let (tool, dir, copied, env) = fake_clipboard(d.path());
    let mut cmd = kb_in(&root);
    cmd.args(["copy", ID])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("PATH", path_with(&dir));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(fs::read_to_string(copied).unwrap(), payload(&root, ID));
    let err = text(&out.stderr);
    assert_eq!(err.matches("unknown key \"bogus\"").count(), 1, "{err}");
    assert_eq!(err.matches("is not valid").count(), 1, "{err}");
    assert!(err.contains(&format!("via {tool}")), "{err}");
}

#[test]
#[ignore = "uses the real clipboard; run by hand on a Mac: cargo test -p kb --test cli_copy -- --ignored"]
fn real_pbcopy_round_trip() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    if !cfg!(target_os = "macos") {
        return;
    }
    // leave the clipboard as it was
    let saved = Command::new("pbpaste").output().unwrap().stdout;
    let (_d, root) = search_kb();
    let out = kb_in(&root)
        .args(["copy", ID])
        // the real PATH: the harness would put its stand-ins first
        .env("PATH", std::env::var_os("PATH").unwrap())
        .output()
        .unwrap();
    let pasted = Command::new("pbpaste").output().unwrap().stdout;
    let mut restore = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    restore.stdin.take().unwrap().write_all(&saved).unwrap();
    restore.wait().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("via pbcopy"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(String::from_utf8(pasted).unwrap(), payload(&root, ID));
}
