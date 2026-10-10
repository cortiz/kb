//! `kb add --editor`: a template in the editor, checked afterwards. Editors are scripts; the
//! answers to the prompt are typed into a pty.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use support::pty::{CTRL_C, ENTER, Session, Spec, kb_home};

const NOW: &str = "2026-10-09T12:00:00Z";
const GOOD: &str = "---\nkb_format: 1\ntype: sql\ntitle: Demo\ncreated: 2026-10-09T12:00:00Z\nupdated: 2026-10-09T12:00:00Z\n---\n\n```sql\nSELECT 1;\n```\n";

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// An editor that writes `content` into the file it is given.
fn writer(t: &tempfile::TempDir, name: &str, content: &str) -> PathBuf {
    let src = t.path().join(format!("{name}.content"));
    fs::write(&src, content).unwrap();
    script(t.path(), name, &format!("cat '{}' > \"$1\"", src.display()))
}

fn run(
    t: &tempfile::TempDir,
    editor: &Path,
    extra: &[&str],
) -> (Session, PathBuf, PathBuf, PathBuf) {
    let (_, home, root) = (0, t.path().join("home"), t.path().join("home/kb"));
    let _ = home;
    let out = t.path().join("out.txt");
    let seen = t.path().join("seen-path");
    let mut args = vec!["add", "--editor", "--empty"];
    args.extend_from_slice(extra);
    fs::create_dir_all(t.path().join("tmp")).unwrap();
    let mut spec = Spec::new(&args, &t.path().join("home"), &root)
        .env("TMPDIR", t.path().join("tmp").to_str().unwrap())
        .env("KB_EDITOR", editor.to_str().unwrap())
        .env("KB_TEST_NOW", NOW);
    spec.out = Some(out.clone());
    (Session::spawn(&spec), root, out, seen)
}

fn kb_dir() -> (tempfile::TempDir, PathBuf) {
    let (t, _home, root) = kb_home("search");
    (t, root)
}

/// Files kb left in this test's own temp directory.
fn temp_files(t: &tempfile::TempDir) -> Vec<String> {
    let Ok(dir) = fs::read_dir(t.path().join("tmp")) else {
        return Vec::new();
    };
    dir.flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("kb-") && n.ends_with(".md"))
        .collect()
}

#[test]
fn a_valid_result_is_created_and_the_id_printed() {
    let (t, _root) = kb_dir();
    let before = temp_files(&t);
    let editor = writer(&t, "ed", GOOD);
    let (mut s, root, out, _) = run(&t, &editor, &["--type", "sql", "--path", "sql/demo"]);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(fs::read_to_string(out).unwrap(), "sql/demo\n");
    assert_eq!(fs::read_to_string(root.join("sql/demo.md")).unwrap(), GOOD);
    assert_eq!(temp_files(&t), before);
}

#[test]
fn the_id_is_derived_from_the_type_and_title_of_the_result() {
    let (t, _root) = kb_dir();
    let editor = writer(&t, "ed", GOOD);
    let (mut s, root, out, _) = run(&t, &editor, &[]);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(fs::read_to_string(out).unwrap(), "sql/demo\n");
    assert!(root.join("sql/demo.md").is_file());
}

#[test]
fn an_unchanged_template_or_a_failing_editor_cancels() {
    let (t, _root) = kb_dir();
    let before = temp_files(&t);
    let (mut s, root, out, _) = run(
        &t,
        &script(t.path(), "noop", "exit 0"),
        &["--type", "sql", "-t", "X"],
    );
    assert_eq!(s.wait_exit(), 130, "{}", s.screen());
    assert_eq!(fs::read_to_string(&out).unwrap_or_default(), "");
    assert!(!root.join("sql/x.md").exists());
    let (t2, _r) = kb_dir();
    let failing = script(t2.path(), "fail", "echo junk >> \"$1\"; exit 3");
    let (mut s, root2, _, _) = run(&t2, &failing, &["--type", "sql", "-t", "X"]);
    assert_eq!(s.wait_exit(), 130, "{}", s.screen());
    assert!(!root2.join("sql/x.md").exists());
    assert_eq!(temp_files(&t), before);
}

#[test]
fn the_template_is_private_and_outside_the_kb() {
    let (t, _root) = kb_dir();
    let log = t.path().join("where");
    let editor = script(
        t.path(),
        "spy",
        &format!(
            "echo \"$1\" > '{0}'; ls -l \"$1\" >> '{0}'; head -c 300 \"$1\" >> '{0}'",
            log.display()
        ),
    );
    let (mut s, root, _, _) = run(&t, &editor, &["--type", "cw", "-t", "Spy", "--tag", "x"]);
    assert_eq!(s.wait_exit(), 130);
    let seen = fs::read_to_string(&log).unwrap();
    let path = seen.lines().next().unwrap();
    assert!(!Path::new(path).starts_with(&root), "{path}");
    assert!(path.contains("/tmp/kb-"), "{path}");
    assert!(seen.contains("-rw-------"), "{seen}");
    assert!(
        seen.contains("type: cw")
            && seen.contains("title: Spy")
            && seen.contains("tags: [x]")
            && seen.contains("# log_groups:"),
        "{seen}"
    );
    assert!(!Path::new(path).exists());
}

const BAD: &str = "---\nkb_format: 1\ntype: sql\ntitle: Demo\n---\n\n```sql\nSELECT 1;\n```\n";

#[test]
fn errors_are_listed_and_edit_again_keeps_the_content() {
    let (t, _root) = kb_dir();
    let counter = t.path().join("n");
    let fixed = t.path().join("fixed.content");
    fs::write(&fixed, GOOD).unwrap();
    fs::write(
        t.path().join("bad.content"),
        BAD.replace("title: Demo\n", ""),
    )
    .unwrap();
    // first run writes the bad file, second appends nothing but fixes the title in place
    let editor = script(
        t.path(),
        "twice",
        &format!(
            "if [ ! -e '{c}' ]; then touch '{c}'; cat '{b}' > \"$1\"; else sed 's/^type: sql/type: sql\\ntitle: Demo/' \"$1\" > \"$1.new\" && mv \"$1.new\" \"$1\"; fi",
            c = counter.display(),
            b = t.path().join("bad.content").display()
        ),
    );
    let (mut s, root, out, _) = run(&t, &editor, &["--path", "sql/again"]);
    s.wait_for("[e]dit again  [s]ave anyway  [d]iscard");
    assert!(s.screen().contains("KB014"), "{}", s.screen());
    s.type_text("e");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(fs::read_to_string(out).unwrap(), "sql/again\n");
    assert!(
        fs::read_to_string(root.join("sql/again.md"))
            .unwrap()
            .contains("title: Demo")
    );
}

#[test]
fn save_anyway_writes_what_is_there_and_discard_does_not() {
    let (t, _root) = kb_dir();
    let bad = BAD.replace("title: Demo\n", "");
    let editor = writer(&t, "bad", &bad);
    let (mut s, root, out, _) = run(&t, &editor, &["--path", "sql/rough"]);
    s.wait_for("[e]dit again  [s]ave anyway  [d]iscard");
    s.type_text("s");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(fs::read_to_string(&out).unwrap(), "sql/rough\n");
    assert_eq!(fs::read_to_string(root.join("sql/rough.md")).unwrap(), bad);

    let (t2, _r) = kb_dir();
    let editor = writer(&t2, "bad", &bad);
    let (mut s, root2, _, _) = run(&t2, &editor, &["--path", "sql/rough"]);
    s.wait_for("[d]iscard");
    s.type_text("d");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 130);
    assert!(!root2.join("sql/rough.md").exists());
}

#[test]
fn a_secret_cannot_be_saved_anyway_and_ctrl_c_discards() {
    let (t, _root) = kb_dir();
    let before = temp_files(&t);
    let leaky = GOOD.replace("SELECT 1;", "SELECT 'AKIAIOSFODNN7EXAMPLE';");
    let editor = writer(&t, "leaky", &leaky);
    let (mut s, root, _, _) = run(&t, &editor, &["--path", "sql/leaky"]);
    s.wait_for("[e]dit again");
    let screen = s.screen();
    assert!(
        screen.contains("KB060") && !screen.contains("[s]ave anyway"),
        "{screen}"
    );
    s.type_text("s");
    s.send(ENTER);
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130, "{}", s.screen());
    assert!(!root.join("sql/leaky.md").exists());
    assert_eq!(temp_files(&t), before);
}

#[test]
fn without_a_terminal_the_editor_cannot_run() {
    let (t, root) = kb_dir();
    let out = support::kb_without_terminal(
        &[
            "--root",
            root.to_str().unwrap(),
            "add",
            "--editor",
            "--type",
            "sql",
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no terminal to run the editor on"));
    let _ = t;
}
