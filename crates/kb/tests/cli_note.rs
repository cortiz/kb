//! `kb note` and `kb add --type note`: authoring Markdown notes.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use support::kb_in;
use support::pty::{Session, Spec, kb_home};

const NOW: &str = "2026-10-09T12:00:00Z";
const BODY: &str = "# Role cache TTL\n\nThe authorizer caches roles for 300 seconds.\n\n- revoke: DEL\n- grant: SETEX\n";

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

fn new_root() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("kb");
    fs::create_dir_all(&root).unwrap();
    (d, root)
}

fn run(root: &Path, args: &[&str], stdin: &str) -> std::process::Output {
    kb_in(root)
        .args(args)
        .env("KB_TEST_NOW", NOW)
        .write_stdin(stdin)
        .output()
        .unwrap()
}

fn file(root: &Path, id: &str) -> String {
    fs::read_to_string(root.join(format!("{id}.md"))).unwrap()
}

#[test]
fn a_note_from_stdin_is_saved_under_note_with_the_title_given() {
    let (_d, root) = new_root();
    let out = run(
        &root,
        &[
            "note",
            "Role cache TTL",
            "--tag",
            "redis",
            "--tags",
            "caching",
            "--yes",
        ],
        BODY,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "note/role-cache-ttl\n");
    assert_eq!(
        file(&root, "note/role-cache-ttl"),
        format!(
            "---\nkb_format: 1\ntype: note\ntitle: Role cache TTL\ntags: [redis, caching]\ncreated: {NOW}\nupdated: {NOW}\n---\n\n{BODY}"
        )
    );
}

#[test]
fn piped_input_needs_no_yes_and_the_title_may_come_from_the_heading() {
    let (_d, root) = new_root();
    let out = run(
        &root,
        &["note"],
        "# Rotate the `signing` **key**\n\nSteps.\n",
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "note/rotate-the-signing-key\n");
    let written = file(&root, "note/rotate-the-signing-key");
    assert!(
        written.contains("title: Rotate the signing key\n"),
        "{written}"
    );
    // and it lints clean
    let lint = kb_in(&root).arg("lint").arg("--strict").output().unwrap();
    assert_eq!(lint.status.code(), Some(0), "{}", text(&lint.stdout));
}

#[test]
fn add_type_note_is_the_same_thing() {
    let (_d, root) = new_root();
    for (args, id) in [
        (
            vec!["add", "--type", "note", "--title", "One", "--yes"],
            "note/one",
        ),
        (vec!["add", "--type", "note", "Two", "--yes"], "note/two"),
    ] {
        let out = run(&root, &args, BODY);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        assert_eq!(text(&out.stdout), format!("{id}\n"));
        assert!(file(&root, id).contains("type: note\n"));
    }
    // detection alone finds a note in structured Markdown
    let out = run(&root, &["add", "--yes"], BODY);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "note/role-cache-ttl\n");
}

#[test]
fn private_adds_the_tag() {
    let (_d, root) = new_root();
    let out = run(
        &root,
        &["note", "Plans", "--private", "--tag", "hr", "--yes"],
        "# Plans\n\nText.\n",
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(file(&root, "note/plans").contains("tags: [hr, private]\n"));
}

#[test]
fn a_note_without_any_title_is_refused() {
    let (_d, root) = new_root();
    let out = run(&root, &["note", "--yes"], "just text, no heading\n");
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("title"), "{}", text(&out.stderr));
    assert!(!root.join("note").exists());
}

#[test]
fn a_secret_in_the_body_blocks_the_save() {
    let (_d, root) = new_root();
    let out = run(
        &root,
        &["note", "Keys", "--yes"],
        "# Keys\n\naws_access_key_id = AKIAIOSFODNN7EXAMPLE\n",
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("KB060"), "{}", text(&out.stderr));
    assert!(!root.join("note/keys.md").exists());
}

#[test]
fn the_slug_is_made_from_the_title_and_clashes_get_a_number() {
    let (_d, root) = new_root();
    for expected in ["note/same-title", "note/same-title-2"] {
        let out = run(
            &root,
            &["note", "Same title", "--yes"],
            "# Same title\n\nx\n",
        );
        assert_eq!(
            text(&out.stdout),
            format!("{expected}\n"),
            "{}",
            text(&out.stderr)
        );
    }
}

fn editor(dir: &Path, content: &str) -> PathBuf {
    let src = dir.join("content");
    fs::write(&src, content).unwrap();
    let p = dir.join("ed");
    fs::write(&p, format!("#!/bin/sh\ncat '{}' > \"$1\"\n", src.display())).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn on_a_terminal_the_editor_opens_on_a_template_and_the_result_is_saved() {
    let (t, home, root) = kb_home("search");
    let ed = editor(
        t.path(),
        "---\nkb_format: 1\ntype: note\n---\n\n# Written in the editor\n\nBody.\n",
    );
    let out = t.path().join("out.txt");
    fs::create_dir_all(t.path().join("tmp")).unwrap();
    let mut spec = Spec::new(&["note", "--tag", "ops"], &home, &root)
        .env("TMPDIR", t.path().join("tmp").to_str().unwrap())
        .env("KB_EDITOR", ed.to_str().unwrap())
        .env("KB_TEST_NOW", NOW);
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(
        fs::read_to_string(out).unwrap(),
        "note/written-in-the-editor\n"
    );
    let written = fs::read_to_string(root.join("note/written-in-the-editor.md")).unwrap();
    assert!(written.contains("# Written in the editor"), "{written}");
}

#[test]
fn the_template_has_the_heading_the_hints_and_the_given_title() {
    let (t, home, root) = kb_home("search");
    let log = t.path().join("seen");
    let spy = t.path().join("spy");
    fs::write(&spy, format!("#!/bin/sh\ncp \"$1\" '{}'\n", log.display())).unwrap();
    fs::set_permissions(&spy, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(t.path().join("tmp")).unwrap();
    let spec = Spec::new(&["note", "Role cache TTL", "--private"], &home, &root)
        .env("TMPDIR", t.path().join("tmp").to_str().unwrap())
        .env("KB_EDITOR", spy.to_str().unwrap())
        .env("KB_TEST_NOW", NOW);
    let mut s = Session::spawn(&spec);
    assert_eq!(s.wait_exit(), 130, "{}", s.screen());
    let seen = fs::read_to_string(log).unwrap();
    assert!(seen.contains("type: note\n"), "{seen}");
    assert!(seen.contains("title: Role cache TTL\n"), "{seen}");
    assert!(seen.contains("tags: [private]\n"), "{seen}");
    assert!(seen.contains("# Add \"private\" to tags"), "{seen}");
    assert!(seen.contains("\n# Role cache TTL\n"), "{seen}");
}
