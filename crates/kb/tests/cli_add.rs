//! `kb add`: capture from stdin or the clipboard, and save with `--yes`. Nothing here touches the
//! real clipboard (fake read programs on `PATH`) or the real `~/kb` (temp roots).

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use support::{fake_paste, kb_in, kb_without_terminal, path_with};

const NOW: &str = "2026-10-09T12:00:00Z";
const SQL: &str = "select state, count(*)\nfrom pg_stat_activity\ngroup by state;";

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

fn new_root() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("kb");
    fs::create_dir_all(&root).unwrap();
    (d, root)
}

/// `kb add <args>` with `stdin` piped in and the clock fixed.
fn add(root: &Path, args: &[&str], stdin: &str) -> std::process::Output {
    kb_in(root)
        .arg("add")
        .args(args)
        .env("KB_TEST_NOW", NOW)
        .write_stdin(stdin)
        .output()
        .unwrap()
}

fn file(root: &Path, id: &str) -> String {
    fs::read_to_string(root.join(format!("{id}.md"))).unwrap()
}

fn md_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if !e.file_name().to_string_lossy().starts_with('.') {
                    stack.push(p);
                }
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn stdin_becomes_the_body_and_yes_prints_only_the_id() {
    let (_d, root) = new_root();
    let out = add(
        &root,
        &["-t", "Active connections", "--tag", "postgres", "--yes"],
        SQL,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "sql/active-connections\n");
    assert_eq!(
        file(&root, "sql/active-connections"),
        format!(
            "---\nkb_format: 1\ntype: sql\ntitle: Active connections\ntags: [postgres]\ncreated: {NOW}\nupdated: {NOW}\n---\n\n```sql\n{SQL}\n```\n"
        )
    );
}

#[test]
fn stdin_wins_over_the_clipboard_and_the_flags_force_a_source() {
    let (d, root) = new_root();
    let (_tool, dir, log, env) = fake_paste(d.path(), "kubectl get pods -n prod");
    let run = |args: &[&str], stdin: &str| {
        let mut cmd = kb_in(&root);
        cmd.arg("add")
            .args(args)
            .env("KB_TEST_NOW", NOW)
            .env("PATH", path_with(&dir));
        for (k, v) in &env {
            cmd.env(k, v);
        }
        cmd.write_stdin(stdin).output().unwrap()
    };
    let out = run(&["-t", "From stdin", "--yes"], SQL);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(file(&root, "sql/from-stdin").contains("pg_stat_activity"));
    assert!(!log.exists(), "the clipboard was not read");

    let out = run(&["-t", "From clipboard", "--clipboard", "--yes"], SQL);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let t = file(&root, "cmd/from-clipboard");
    assert!(
        t.contains("kubectl get pods -n prod") && !t.contains("pg_stat_activity"),
        "{t}"
    );
    assert!(log.exists());

    let out = run(&["-t", "Forced stdin", "--stdin", "--yes"], SQL);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(file(&root, "sql/forced-stdin").contains("pg_stat_activity"));

    let out = run(
        &["-t", "Nothing", "--empty", "--type", "prompt", "--yes"],
        SQL,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let t = file(&root, "prompt/nothing");
    assert!(t.contains("type: prompt") && !t.contains("pg_stat"), "{t}");
}

#[test]
fn two_source_flags_together_are_a_usage_error() {
    let (_d, root) = new_root();
    for pair in [
        ["--stdin", "--empty"],
        ["--clipboard", "--stdin"],
        ["--empty", "--clipboard"],
    ] {
        let out = add(&root, &[pair[0], pair[1], "-t", "x", "--yes"], SQL);
        assert_eq!(out.status.code(), Some(2), "{pair:?}");
    }
    assert!(md_files(&root).is_empty());
}

#[test]
fn yes_without_a_title_exits_1() {
    let (_d, root) = new_root();
    let out = add(&root, &["--yes"], "kubectl get pods -n prod");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    assert!(
        text(&out.stderr).contains("a title is required (use --title)"),
        "{}",
        text(&out.stderr)
    );
    assert!(md_files(&root).is_empty());
}

#[test]
fn yes_takes_a_title_from_a_leading_comment() {
    let (_d, root) = new_root();
    let out = add(&root, &["--yes"], &format!("-- Active connections\n{SQL}"));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "sql/active-connections\n");
}

#[test]
fn low_confidence_needs_a_type() {
    let (_d, root) = new_root();
    let prose = "please remember to ask about the numbers for last quarter";
    let out = add(&root, &["-t", "Remember", "--yes"], prose);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("could not tell the type (use --type)"),
        "{}",
        text(&out.stderr)
    );
    assert!(md_files(&root).is_empty());
    let out = add(
        &root,
        &["-t", "Remember", "--type", "prompt", "--yes"],
        prose,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "prompt/remember\n");
}

#[test]
fn type_overrides_detection() {
    let (_d, root) = new_root();
    let out = add(
        &root,
        &["-t", "As a command", "--type", "cmd", "--yes"],
        SQL,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(file(&root, "cmd/as-a-command").contains("type: cmd"));
    let out = add(&root, &["-t", "Bad", "--type", "nope", "--yes"], SQL);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn tags_can_repeat_and_be_comma_separated() {
    let (_d, root) = new_root();
    let out = add(
        &root,
        &[
            "-t", "Tagged", "--tag", "a", "--tag", "b, #C", "--tag", "a", "--yes",
        ],
        SQL,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(file(&root, "sql/tagged").contains("tags: [a, b, c]\n"));
}

#[test]
fn path_creates_folders_and_a_clash_gets_a_number() {
    let (_d, root) = new_root();
    let out = add(
        &root,
        &["-t", "Placed", "--path", "team/db/active", "--yes"],
        SQL,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "team/db/active\n");
    assert!(root.join("team/db/active.md").is_file());
    // a derived path that clashes gets -2, then -3
    for expected in ["sql/same", "sql/same-2", "sql/same-3"] {
        let out = add(&root, &["-t", "Same", "--yes"], SQL);
        assert_eq!(
            text(&out.stdout),
            format!("{expected}\n"),
            "{}",
            text(&out.stderr)
        );
    }
    // an explicit path that exists is refused
    let out = add(
        &root,
        &["-t", "Placed", "--path", "team/db/active", "--yes"],
        SQL,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("team/db/active already exists"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn the_path_template_setting_shapes_the_id() {
    let (_d, root) = new_root();
    let out = kb_in(&root)
        .args(["add", "-t", "P99 latency", "--tag", "ECS", "--yes"])
        .env("KB_PATH_TEMPLATE", "{tag}/{slug}")
        .env("KB_TEST_NOW", NOW)
        .write_stdin(SQL)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "ecs/p99-latency\n");
}

#[test]
fn a_title_with_no_usable_characters_needs_a_path() {
    let (_d, root) = new_root();
    let out = add(&root, &["-t", "¿¿??", "--yes"], SQL);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("a path is required (use --path)"),
        "{}",
        text(&out.stderr)
    );
    let out = add(&root, &["-t", "¿¿??", "--path", "sql/odd", "--yes"], SQL);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn secrets_are_refused_and_the_way_out_is_named() {
    let (_d, root) = new_root();
    let out = add(
        &root,
        &["-t", "Leaky", "--yes"],
        "select * from t where k = 'AKIAIOSFODNN7EXAMPLE';",
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    let err = text(&out.stderr);
    assert!(err.contains("KB060"), "{err}");
    assert!(
        err.contains("{{variable}} of kind secret") && err.contains("kb:allow-secret"),
        "{err}"
    );
    assert!(md_files(&root).is_empty(), "nothing was written");
    // a secret variable and an allow comment are the ways out
    let out = add(
        &root,
        &["-t", "Fine", "--yes"],
        "curl -H \"Authorization: {{token}}\" https://example.com",
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
}

#[test]
fn an_entry_over_the_size_limit_is_refused_with_the_limit() {
    let (_d, root) = new_root();
    let big = "select 1 from t;\n".repeat(70_000);
    let out = add(&root, &["-t", "Huge", "--yes"], &big);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains("entries are limited to 1048576"), "{err}");
    assert!(md_files(&root).is_empty());
}

#[test]
fn json_prints_the_show_document() {
    let (_d, root) = new_root();
    let out = add(
        &root,
        &["-t", "Json please", "--tag", "x", "--json", "--yes"],
        "kubectl get pods -n {{name}}",
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["id"], "cmd/json-please");
    assert_eq!(doc["type"], "cmd");
    assert_eq!(doc["title"], "Json please");
    assert_eq!(doc["tags"][0], "x");
    assert_eq!(doc["vars"][0]["name"], "name");
    assert_eq!(doc["created"], NOW);
    let shown = kb_in(&root)
        .args(["show", "--json", "cmd/json-please"])
        .output()
        .unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&shown.stdout).unwrap(), doc);
}

#[test]
fn yes_and_editor_conflict_and_a_missing_terminal_needs_yes() {
    let (_d, root) = new_root();
    let out = add(&root, &["--yes", "--editor", "-t", "x"], SQL);
    assert_eq!(out.status.code(), Some(2));
    let out = kb_without_terminal(
        &["--root", root.to_str().unwrap(), "add", "-t", "x"],
        &[("KB_TEST_NOW", NOW)],
    );
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("kb add needs --yes or --editor when there is no terminal"),
        "{}",
        text(&out.stderr)
    );
    assert!(md_files(&root).is_empty());
}

#[test]
fn over_ssh_the_clipboard_is_not_read_and_the_draft_is_empty() {
    let (d, root) = new_root();
    let (_tool, dir, log, env) = fake_paste(d.path(), "select 1 from t;");
    let mut cmd = kb_in(&root);
    cmd.args([
        "add",
        "--clipboard",
        "-t",
        "Over ssh",
        "--type",
        "prompt",
        "--yes",
    ])
    .env("PATH", path_with(&dir))
    .env("SSH_TTY", "/dev/pts/1")
    .env("KB_TEST_NOW", NOW);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.write_stdin("").output().unwrap();
    assert!(!log.exists(), "the clipboard was not read over SSH");
    let err = text(&out.stderr);
    assert!(err.contains("clipboard not readable over SSH"), "{err}");
    assert_eq!(
        err.matches("clipboard not readable").count(),
        1,
        "one line only: {err}"
    );
    // an empty body is only a warning for lint, so the entry is saved
    assert_eq!(out.status.code(), Some(0), "{err}");
    assert!(file(&root, "prompt/over-ssh").contains("type: prompt"));
}

#[test]
fn the_new_entry_is_searchable_at_once() {
    let (_d, root) = new_root();
    let out = add(&root, &["-t", "Zebra stripes", "--yes"], SQL);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let found = kb_in(&root)
        .args(["search", "zebra", "--ids"])
        .output()
        .unwrap();
    assert_eq!(text(&found.stdout), "sql/zebra-stripes\n");
}
