//! `kb show`, `kb edit` and the terminal-less behavior of plain `kb`.

mod support;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kb_core::entry_json::PrintMode;
use kb_core::search::Kb;
use serde_json::Value;
use support::{kb_in, kb_without_terminal, search_kb};

const ID: &str = "sumo/5xx-by-authorizer";

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

fn show(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    kb_in(root).arg("show").args(args).output().unwrap()
}

#[test]
fn raw_prints_exactly_the_payload_a_pick_prints() {
    let (_d, root) = search_kb();
    let kb = Kb::open(&root).unwrap();
    for (id, text) in [
        (ID, None),
        (
            "link/payments-runbook",
            Some("https://wiki.example.com/runbooks/payments"),
        ),
        ("code/request-handler", Some("src/handler.rs:10")),
    ] {
        let out = show(&root, &[id, "--raw"]);
        assert_eq!(out.status.code(), Some(0), "{id}: {}", stderr(&out));
        let want = text.map_or_else(|| kb.pick(id, PrintMode::Payload).unwrap(), str::to_string);
        assert_eq!(stdout(&out), want, "{id}");
    }
    let out = show(&root, &[ID, "--raw"]);
    assert!(!stdout(&out).ends_with("\n\n") && stdout(&out).contains("_sourceCategory"));
}

#[test]
fn the_default_is_the_preview_as_plain_text_when_piped() {
    let (_d, root) = search_kb();
    let out = show(&root, &[ID]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        !text.contains('\x1b'),
        "no color codes when piped: {text:?}"
    );
    assert!(text.ends_with('\n'));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0].trim(), "5xx by authorizer", "the title first");
    assert!(
        lines[1].starts_with("sumo") && lines[1].contains("#auth"),
        "the metadata line: {}",
        lines[1]
    );
    assert_eq!(
        lines[2].trim(),
        "sumo/5xx-by-authorizer.md",
        "the path in the KB"
    );
    assert!(text.contains("── payload"), "{text}");
    assert!(text.contains("_sourceCategory"), "{text}");
    let first_payload_line = lines
        .iter()
        .skip_while(|l| !l.contains("── payload"))
        .nth(1)
        .unwrap();
    assert!(
        first_payload_line.trim_start().starts_with("1 "),
        "line numbers: {first_payload_line}"
    );
    for l in text.lines() {
        assert_eq!(l, l.trim_end(), "no trailing spaces: {l:?}");
    }
}

#[test]
fn the_preview_has_variables_fields_and_notes_when_the_entry_has_them() {
    let (_d, root) = support::search_kb();
    // an entry with variables
    let out = show(&root, &["sumo/login-failures"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("── variables") || text.contains("── notes"),
        "{text}"
    );
    let out = show(&root, &["link/payments-runbook"]);
    let text = stdout(&out);
    assert!(
        text.contains("https://wiki.example.com/runbooks/payments"),
        "{text}"
    );
    let out = show(&root, &["code/request-handler"]);
    let text = stdout(&out);
    assert!(
        text.contains("repo") && text.contains("src/handler.rs"),
        "{text}"
    );
}

#[test]
fn colors_only_on_a_terminal_and_never_with_no_color() {
    use support::pty::{Session, Spec};
    let (_t, home, root) = support::pty::kb_home("search");
    let run = |extra: &[&str], no_color: bool| {
        let mut args = vec!["show", ID];
        args.extend_from_slice(extra);
        let mut spec = Spec::new(&args, &home, &root);
        if no_color {
            spec = spec.env("NO_COLOR", "1");
        }
        let mut s = Session::spawn(&spec);
        assert_eq!(s.wait_exit(), 0);
        s.raw_output()
    };
    assert!(run(&[], false).contains("\x1b["), "a terminal gets colors");
    assert!(!run(&["--no-color"], false).contains("\x1b["), "--no-color");
    assert!(!run(&[], true).contains("\x1b["), "NO_COLOR");
}

#[test]
fn json_is_the_entry_document() {
    let (_d, root) = search_kb();
    let json = show(&root, &[ID, "--json"]);
    assert_eq!(json.status.code(), Some(0), "{}", stderr(&json));
    let doc: Value = serde_json::from_slice(&json.stdout).unwrap();
    for k in [
        "id", "path", "file", "type", "title", "tags", "vars", "payload", "notes", "fields",
    ] {
        assert!(doc.get(k).is_some(), "{k}");
    }
    assert_eq!(doc["payload"], stdout(&show(&root, &[ID, "--raw"])));
    assert!(stdout(&json).ends_with("}\n"));
    let kb = Kb::open(&root).unwrap();
    assert_eq!(stdout(&json), kb.pick(ID, PrintMode::Json).unwrap());
}

#[test]
fn file_prints_the_absolute_path() {
    let (_d, root) = search_kb();
    let out = show(&root, &[ID, "--file"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let path = stdout(&out);
    assert!(
        std::path::Path::new(&path).is_absolute() && std::path::Path::new(&path).exists(),
        "{path}"
    );
    assert!(path.ends_with("sumo/5xx-by-authorizer.md"));
}

#[test]
fn the_modes_exclude_each_other_and_print_is_gone() {
    let (_d, root) = search_kb();
    for args in [
        vec![ID, "--raw", "--json"],
        vec![ID, "--file", "--raw"],
        vec![ID, "--json", "--file"],
    ] {
        assert_eq!(show(&root, &args).status.code(), Some(2), "{args:?}");
    }
    // clarification 3: --print is a usage error on show (and the old modes live on as flags)
    for args in [vec![ID, "--print", "id"], vec![ID, "--print", "payload"]] {
        assert_eq!(show(&root, &args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn a_query_resolves_like_copy() {
    let (_d, root) = search_kb();
    let out = show(&root, &["5xx", "auth", "--raw"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        Kb::open(&root)
            .unwrap()
            .pick(ID, PrintMode::Payload)
            .unwrap()
    );
    // an ID with .md works too
    assert_eq!(
        show(&root, &["sumo/5xx-by-authorizer.md", "--raw"])
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn close_matches_without_a_terminal_are_listed_with_exit_1() {
    let (_d, root) = search_kb();
    let out = kb_without_terminal(&["--root", root.to_str().unwrap(), "show", "latency"], &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let err = stderr(&out);
    let candidates = err.lines().filter(|l| l.contains('\t')).count();
    assert!((2..=5).contains(&candidates), "{err}");
    assert_eq!(stdout(&out), "");
    // and as JSON
    let out = kb_without_terminal(
        &[
            "--root",
            root.to_str().unwrap(),
            "show",
            "latency",
            "--json",
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["error"]["kind"], "ambiguous");
    assert!(doc["error"]["candidates"].as_array().unwrap().len() >= 2);
}

#[test]
fn nothing_found_is_exit_1_not_2() {
    let (_d, root) = search_kb();
    let out = show(&root, &["nope/x"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("no entry matches \"nope/x\""),
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), "");
    let out = show(&root, &["nope/x", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["error"]["kind"], "not_found");
    // text that is not an ID falls through to a search, so it can never read outside the root
    let outside = show(&root, &["../../etc/passwd", "--file"]);
    assert!(!stdout(&outside).contains("passwd"), "{}", stdout(&outside));
}

#[test]
fn show_records_no_use() {
    let (_d, root) = search_kb();
    show(&root, &[ID]);
    show(&root, &[ID, "--raw"]);
    assert!(!root.join(".kb/state.json").exists());
}

/// Runs `kb` with no controlling terminal and no stdin, in a new session.
fn without_a_terminal(args: &[&str]) -> (Option<i32>, String, Duration) {
    let started = Instant::now();
    let mut child = Command::new("perl")
        .args([
            "-MPOSIX",
            "-e",
            "POSIX::setsid(); exec @ARGV",
            "--",
            support::pty::KB,
        ])
        .args(args)
        .env_remove("KB_ROOT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("perl is available");
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut err).unwrap();
            return (status.code(), err, started.elapsed());
        }
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("kb waited for a terminal that does not exist");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn plain_kb_without_a_terminal_prints_help_and_exits_2() {
    let (_d, root) = search_kb();
    for args in [
        vec!["--root", root.to_str().unwrap()],
        vec!["--root", root.to_str().unwrap(), "serach", "x"],
    ] {
        let (code, err, took) = without_a_terminal(&args);
        assert_eq!(code, Some(2), "{err}");
        assert!(
            err.contains("Usage:") && err.contains("needs a terminal"),
            "{err}"
        );
        assert!(took < Duration::from_secs(5), "{took:?}");
    }
}

#[test]
fn help_mentions_the_ui_options() {
    let out = kb_in(std::path::Path::new("/x"))
        .args(["tui", "--help"])
        .output()
        .unwrap();
    assert!(stdout(&out).contains("--print"), "{}", stdout(&out));
    let out = kb_in(std::path::Path::new("/x"))
        .arg("--help")
        .output()
        .unwrap();
    let text = stdout(&out);
    for cmd in ["search", "show", "edit", "tui", "reindex"] {
        assert!(text.contains(cmd), "{cmd}");
    }
}

// ---- kb edit ----

fn script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// Runs `kb edit` under a pseudo-terminal (the editor needs one) and returns its exit code.
fn edit_in_a_pty(
    home: &std::path::Path,
    root: &std::path::Path,
    editor: &std::path::Path,
    id: &str,
) -> (u32, String) {
    use support::pty::{Session, Spec};
    let mut spec = Spec::new(&["edit", id], home, root);
    spec = spec.env("EDITOR", &editor.display().to_string());
    let mut s = Session::spawn(&spec);
    let code = s.wait_exit();
    (code, s.screen())
}

#[test]
fn edit_runs_the_editor_on_the_file() {
    let (t, home, root) = support::pty::kb_home("search");
    let marker = t.path().join("marker");
    let ed = script(
        t.path(),
        "ed",
        &format!("echo \"$1\" > {}", marker.display()),
    );
    let (code, screen) = edit_in_a_pty(&home, &root, &ed, "prompt/cache");
    assert_eq!(code, 0, "{screen}");
    let got = std::fs::read_to_string(&marker).unwrap();
    assert!(got.trim().ends_with("prompt/cache.md"), "{got}");
}

#[test]
fn test_show_and_search_json_mentions_unreviewed() {
    let (_d, root) = search_kb();
    let id = "sumo/5xx-by-authorizer";
    kb_core::mcp::review::ReviewStateTracker::mark_unreviewed(&root, id).unwrap();

    // 1. kb show mentions agent (unreviewed)
    let out = show(&root, &[id]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(text.contains("agent (unreviewed)"), "preview text: {text}");

    // 2. kb search --json includes unreviewed: true
    let search_out = kb_in(&root)
        .args(["search", "5xx", "--json"])
        .output()
        .unwrap();
    assert_eq!(search_out.status.code(), Some(0));
    let doc: Value = serde_json::from_slice(&search_out.stdout).unwrap();
    let hit = doc["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(hit["unreviewed"], true);

    // 3. kb search unreviewed:true filter works
    let filter_out = kb_in(&root)
        .args(["search", "unreviewed:true"])
        .output()
        .unwrap();
    assert_eq!(filter_out.status.code(), Some(0));
    let stdout_filter = String::from_utf8_lossy(&filter_out.stdout);
    assert!(
        stdout_filter.contains(id),
        "unreviewed:true should match {id}"
    );

    // 4. kb search unreviewed:false excludes it
    let filter_false_out = kb_in(&root)
        .args(["search", "5xx", "unreviewed:false"])
        .output()
        .unwrap();
    let stdout_false = String::from_utf8_lossy(&filter_false_out.stdout);
    assert!(
        !stdout_false.contains(id),
        "unreviewed:false should not match {id}"
    );
}

// ---- notes ----

const NOTE: &str = "note/role-cache-ttl";

#[test]
fn a_note_is_shown_as_formatted_markdown_when_piped() {
    let (_d, root) = support::notes_kb();
    let out = show(&root, &[NOTE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        !text.contains('\x1b'),
        "no escape codes when piped: {text:?}"
    );
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0].trim(), "Role cache TTL and Redis architecture");
    assert!(
        lines[1].starts_with("note") && lines[1].contains("#redis"),
        "{}",
        lines[1]
    );
    assert_eq!(lines[2].trim(), "note/role-cache-ttl.md");
    for want in [
        "━━━━",  // the level-1 underline
        "┌────", // the table, with its box borders
        "│ Role Revoke",
        "├────",
        "└────",
        "▌ ⚠ Warning", // the callout
        "☐ Check the hit ratio",
        "☑ Read the runbook",
        "┌─ python ──", // the framed code block
        "│ import redis",
        "[sumo] 5xx by authorizer", // an entry link, resolved
        "footnote[1]",
        "[1] Footnote text.",
    ] {
        assert!(text.contains(want), "{want:?} missing from:\n{text}");
    }
    assert!(
        !text.contains("── notes"),
        "a note has no 'notes' label:\n{text}"
    );
    assert!(
        !text.contains("[[") && !text.contains("**") && !text.contains("```"),
        "{text}"
    );
    for l in text.lines() {
        assert!(unicode_width::UnicodeWidthStr::width(l) <= 100, "{l:?}");
    }
}

#[test]
fn a_broken_entry_link_is_marked_and_an_escaped_one_is_literal() {
    let (_d, root) = support::notes_kb();
    let text = stdout(&show(&root, &["note/links"]));
    assert!(text.contains("[sumo] 5xx by authorizer"), "{text}");
    assert!(text.contains("[?] no/such-entry"), "{text}");
    assert!(text.contains("[[not-a-link]]"), "{text}");
}

#[test]
fn raw_prints_the_markdown_without_frontmatter() {
    let (_d, root) = support::notes_kb();
    let out = show(&root, &[NOTE, "--raw"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let file = std::fs::read_to_string(root.join("note/role-cache-ttl.md")).unwrap();
    let body = file
        .split_once("\n---\n")
        .unwrap()
        .1
        .trim_start_matches('\n');
    assert_eq!(stdout(&out), body);
    assert!(!stdout(&out).contains("kb_format"));
    assert!(
        stdout(&out).contains("[[sumo/5xx-by-authorizer]]"),
        "links stay as written"
    );
    // and it is what a pick prints
    assert_eq!(
        stdout(&out),
        Kb::open(&root)
            .unwrap()
            .pick(NOTE, PrintMode::Payload)
            .unwrap()
    );
}

#[test]
fn hostile_notes_are_inert_in_the_preview_and_raw() {
    let (_d, root) = support::notes_kb();
    let shown = stdout(&show(&root, &["note/malicious"]));
    assert!(
        shown.contains('␛'),
        "sequences leave a visible mark: {shown}"
    );
    assert!(
        shown.contains("<script>alert(1)</script>"),
        "HTML is shown as text: {shown}"
    );
    assert!(
        shown.chars().all(|c| c == '\n' || !c.is_control()),
        "no control characters: {shown:?}"
    );
    let raw = stdout(&show(&root, &["note/malicious", "--raw"]));
    assert!(!raw.contains('␛'), "raw leaves no marker");
    assert!(
        raw.chars()
            .all(|c| c == '\n' || c == '\t' || !c.is_control()),
        "no control characters in raw: {raw:?}"
    );
    assert!(raw.contains("Clear screen:  and move."), "{raw}");
    assert!(
        raw.contains("[click me](javascript:alert(1))"),
        "text itself is kept: {raw}"
    );
    let json = stdout(&show(&root, &["note/malicious", "--json"]));
    assert!(!json.contains('\x1b'), "JSON escapes control characters");
}

#[test]
fn an_oversized_note_still_shows_and_warns_in_lint() {
    let (_d, root) = support::notes_kb();
    support::add_oversized_note(&root);
    let out = show(&root, &["note/big"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("A line of prose that repeats"));
    let lint = kb_in(&root).args(["lint", "note/big.md"]).output().unwrap();
    assert!(
        stdout(&lint).contains("KB072-note-oversized"),
        "{}",
        stdout(&lint)
    );
    assert_eq!(lint.status.code(), Some(0), "a warning, not an error");
}
