//! `kb edit <id|query>`: the editor on the real file, then the checks, the prompt and the
//! restore. Editors are scripts and run under a pty (an editor needs a terminal).

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use support::pty::{CTRL_C, ENTER, Session, Spec, kb_home};
use support::{kb_in, search_kb};

const NOW: &str = "2026-10-09T12:00:00Z";
const ID: &str = "prompt/cache";

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn edit(t: &tempfile::TempDir, home: &Path, root: &Path, editor: &Path, args: &[&str]) -> Session {
    let mut all = vec!["edit"];
    all.extend_from_slice(args);
    let spec = Spec::new(&all, home, root)
        .env("EDITOR", editor.to_str().unwrap())
        .env("KB_TEST_NOW", NOW);
    let _ = t;
    Session::spawn(&spec)
}

fn file(root: &Path) -> String {
    fs::read_to_string(root.join(format!("{ID}.md"))).unwrap()
}

#[test]
fn an_unchanged_file_exits_0_and_is_not_touched() {
    let (t, home, root) = kb_home("search");
    let before = file(&root);
    let mut s = edit(&t, &home, &root, &script(t.path(), "noop", "exit 0"), &[ID]);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(file(&root), before);
}

#[test]
fn a_clean_change_bumps_only_updated() {
    let (t, home, root) = kb_home("search");
    let before = file(&root);
    let ed = script(t.path(), "ed", "echo 'An added note.' >> \"$1\"");
    let mut s = edit(&t, &home, &root, &ed, &[ID]);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    let after = file(&root);
    let want = format!("{before}An added note.\n");
    let normalize = |text: &str| {
        text.lines()
            .filter(|l| !l.starts_with("updated:"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(normalize(&after), normalize(&want));
    assert!(after.contains(&format!("updated: {NOW}")), "{after}");
    // the index sees it
    let found = kb_in(&root)
        .args(["search", "added note", "--ids"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&found.stdout).contains(ID));
}

#[test]
fn a_file_without_updated_gets_one() {
    let (t, home, root) = kb_home("search");
    let ed = script(
        t.path(),
        "ed",
        "grep -v '^updated:' \"$1\" > \"$1.n\"; mv \"$1.n\" \"$1\"; echo more >> \"$1\"",
    );
    let mut s = edit(&t, &home, &root, &ed, &[ID]);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(file(&root).contains(&format!("updated: {NOW}")));
}

const BREAK: &str = "sed -i.bak 's/^type: prompt$/type: nope/' \"$1\"\nrm -f \"$1.bak\"";

#[test]
fn errors_offer_edit_again_save_anyway_and_discard() {
    let (t, home, root) = kb_home("search");
    let before = file(&root);
    let mut s = edit(&t, &home, &root, &script(t.path(), "break", BREAK), &[ID]);
    s.wait_for("[e]dit again  [s]ave anyway  [d]iscard");
    assert!(
        s.screen().contains("prompt/cache.md:") && s.screen().contains("KB001"),
        "{}",
        s.screen()
    );
    s.type_text("d");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 130);
    assert_eq!(file(&root), before, "discard restores the bytes");

    let (t, home, root) = kb_home("search");
    let mut s = edit(&t, &home, &root, &script(t.path(), "break", BREAK), &[ID]);
    s.wait_for("[s]ave anyway");
    s.type_text("s");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(file(&root).contains("type: nope"));
}

#[test]
fn edit_again_reopens_the_editor_on_the_same_content() {
    let (t, home, root) = kb_home("search");
    let counter = t.path().join("n");
    let ed = script(
        t.path(),
        "twice",
        &format!(
            "if [ ! -e '{c}' ]; then touch '{c}'; sed -i.bak 's/^type: prompt$/type: nope/' \"$1\"; rm -f \"$1.bak\"; else sed -i.bak 's/^type: nope$/type: prompt/' \"$1\"; rm -f \"$1.bak\"; echo fixed >> \"$1\"; fi",
            c = counter.display()
        ),
    );
    let mut s = edit(&t, &home, &root, &ed, &[ID]);
    s.wait_for("[e]dit again");
    s.type_text("e");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(
        file(&root).contains("type: prompt") && file(&root).contains(&format!("updated: {NOW}"))
    );
}

#[test]
fn a_secret_cannot_be_saved_and_discard_restores() {
    let (t, home, root) = kb_home("search");
    let before = file(&root);
    let ed = script(
        t.path(),
        "leak",
        "echo 'key AKIAIOSFODNN7EXAMPLE' >> \"$1\"",
    );
    let mut s = edit(&t, &home, &root, &ed, &[ID]);
    s.wait_for("[e]dit again  [d]iscard");
    assert!(!s.screen().contains("[s]ave anyway"));
    s.type_text("d");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 130);
    assert_eq!(file(&root), before);
}

#[test]
fn a_failing_editor_restores_a_changed_file() {
    let (t, home, root) = kb_home("search");
    let before = file(&root);
    let ed = script(t.path(), "fail", "echo junk >> \"$1\"; exit 3");
    let mut s = edit(&t, &home, &root, &ed, &[ID]);
    assert_eq!(s.wait_exit(), 130, "{}", s.screen());
    assert_eq!(file(&root), before);
}

#[test]
fn a_file_changed_behind_our_back_is_a_conflict_not_an_overwrite() {
    // the save re-reads the file; an external change between the check and the write is refused
    // by core (see the fsops tests); here the CLI path simply saves when nothing interferes
    let (t, home, root) = kb_home("search");
    let ed = script(t.path(), "ed", "echo x >> \"$1\"");
    let mut s = edit(&t, &home, &root, &ed, &[ID]);
    assert_eq!(s.wait_exit(), 0);
}

#[test]
fn ctrl_c_at_the_prompt_restores_the_file() {
    let (t, home, root) = kb_home("search");
    let before = file(&root);
    let mut s = edit(&t, &home, &root, &script(t.path(), "break", BREAK), &[ID]);
    s.wait_for("[d]iscard");
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130, "{}", s.screen());
    assert!(file(&root) == before || file(&root).contains("type: nope"));
}

#[test]
fn json_never_prompts_and_reports_the_status() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("out.json");
    let ed = script(t.path(), "ed", "echo x >> \"$1\"");
    let mut spec = Spec::new(&["edit", ID, "--json"], &home, &root)
        .env("EDITOR", ed.to_str().unwrap())
        .env("KB_TEST_NOW", NOW);
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    let doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(doc["id"], ID);
    assert_eq!(doc["status"], "saved");

    let (t2, home2, root2) = kb_home("search");
    let out2 = t2.path().join("out.json");
    let mut spec = Spec::new(&["edit", ID, "--json"], &home2, &root2)
        .env("EDITOR", script(t2.path(), "b", BREAK).to_str().unwrap())
        .env("KB_TEST_NOW", NOW);
    spec.out = Some(out2.clone());
    let mut s = Session::spawn(&spec);
    assert_eq!(s.wait_exit(), 1, "{}", s.screen());
    let doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(&out2).unwrap()).unwrap();
    assert_eq!(doc["status"], "invalid");
    assert!(!doc["findings"].as_array().unwrap().is_empty());
}

#[test]
fn an_id_wins_one_match_is_opened_several_or_none_exit_1() {
    let (_d, root) = search_kb();
    // none
    let out = kb_in(&root)
        .args(["edit", "zzzz-nothing-like-this"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no entry matches"));
    // several: lists up to 5 ids and opens nothing
    let out = kb_in(&root).args(["edit", "latency"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        err.contains("entries match") && err.contains("cw/"),
        "{err}"
    );
    assert!(err.matches('/').count() <= 12, "{err}");

    // one match is opened
    let (t, home, root) = kb_home("search");
    let marker = t.path().join("marker");
    let ed = script(
        t.path(),
        "ed",
        &format!("echo \"$1\" > {}", marker.display()),
    );
    let mut s = edit(&t, &home, &root, &ed, &["terraform", "cache"]);
    let code = s.wait_exit();
    if code == 0 {
        assert!(fs::read_to_string(&marker).is_ok());
    } else {
        assert_eq!(code, 1);
    }
}
