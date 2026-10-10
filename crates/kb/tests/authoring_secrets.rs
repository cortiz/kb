//! SC-004: whatever path an entry takes in (the form, `--yes`, the editor template, `kb edit`),
//! the fixture secrets never end up in a file under the KB or in `state.json`.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use support::pty::{ENTER, Session, Spec, kb_home};
use support::{fixtures, kb_in};

const NOW: &str = "2026-10-09T12:00:00Z";
/// Fixtures whose secret is in the body (the frontmatter one is covered by lint tests).
const LEAKY: [&str; 5] = [
    "aws-access-key",
    "aws-secret-key",
    "bearer-token",
    "jwt",
    "private-key",
];
const MARKERS: [&str; 5] = [
    "AKIAIOSFODNN7EXAMPLE",
    "wJalrXUtnFEMI",
    "eyJ",
    "BEGIN RSA PRIVATE KEY",
    "BEGIN PRIVATE KEY",
];

fn secrets_text(name: &str) -> String {
    fs::read_to_string(fixtures().join("secrets").join(format!("{name}.md"))).unwrap()
}

/// Every file under `root` that holds one of the markers.
fn leaks(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(text) = fs::read_to_string(&p) {
                // only what a path of this milestone wrote: the fixture KB itself is clean
                if p.to_string_lossy().contains("authoring-")
                    && MARKERS.iter().any(|m| text.contains(m))
                {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn fresh() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (t, home, root) = kb_home("search");
    (t, home, root)
}

#[test]
fn yes_refuses_every_fixture_secret_and_writes_nothing() {
    for name in LEAKY {
        let (_t, _h, root) = fresh();
        let before = fs::read_dir(&root).unwrap().count();
        let body = secrets_text(name);
        let out = kb_in(&root)
            .args(["add", "--yes", "-t", "authoring leak", "--type", "prompt"])
            .env("KB_TEST_NOW", NOW)
            .write_stdin(body)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!root.join("prompt/authoring-leak.md").exists(), "{name}");
        assert_eq!(fs::read_dir(&root).unwrap().count(), before);
        assert!(
            !fs::read_to_string(root.join(".kb/state.json"))
                .unwrap_or_default()
                .contains("AKIA")
        );
    }
}

#[test]
fn allow_secret_and_secret_variables_are_honored() {
    let (_t, _h, root) = fresh();
    let out = kb_in(&root)
        .args(["add", "--yes", "-t", "Allowed", "--type", "cmd"])
        .env("KB_TEST_NOW", NOW)
        .write_stdin("curl -H 'Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345' https://example.com # kb:allow-secret\n")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = kb_in(&root)
        .args(["add", "--yes", "-t", "Variable", "--type", "cmd"])
        .env("KB_TEST_NOW", NOW)
        .write_stdin("curl -H \"Authorization: Bearer {{token}}\" https://example.com\n")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let t = fs::read_to_string(root.join("cmd/variable.md")).unwrap();
    assert!(t.contains("token: { kind: secret }"), "{t}");
}

#[test]
fn the_editor_template_cannot_save_a_secret() {
    for name in ["aws-access-key", "jwt"] {
        let (t, home, root) = fresh();
        let content = t.path().join("content");
        let leaky = format!(
            "---\nkb_format: 1\ntype: prompt\ntitle: authoring leak\n---\n\n```\n{}\n```\n",
            secrets_text(name)
                .lines()
                .skip(6)
                .collect::<Vec<_>>()
                .join("\n")
        );
        fs::write(&content, leaky).unwrap();
        let editor = script(
            t.path(),
            "ed",
            &format!("cat '{}' > \"$1\"", content.display()),
        );
        let spec = Spec::new(
            &[
                "add",
                "--editor",
                "--empty",
                "--path",
                "prompt/authoring-leak",
            ],
            &home,
            &root,
        )
        .env("KB_EDITOR", editor.to_str().unwrap());
        let mut s = Session::spawn(&spec);
        s.wait_for("[e]dit again");
        assert!(!s.screen().contains("[s]ave anyway"), "{name}");
        s.type_text("s");
        s.send(ENTER);
        s.type_text("d");
        s.send(ENTER);
        assert_eq!(s.wait_exit(), 130);
        assert!(leaks(&root).is_empty() && !root.join("prompt/authoring-leak.md").exists());
    }
}

#[test]
fn kb_edit_restores_a_file_that_gained_a_secret() {
    let (t, home, root) = fresh();
    let before = fs::read_to_string(root.join("prompt/cache.md")).unwrap();
    let editor = script(t.path(), "ed", "echo 'x AKIAIOSFODNN7EXAMPLE' >> \"$1\"");
    let spec =
        Spec::new(&["edit", "prompt/cache"], &home, &root).env("EDITOR", editor.to_str().unwrap());
    let mut s = Session::spawn(&spec);
    s.wait_for("[e]dit again  [d]iscard");
    s.type_text("d");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 130);
    assert_eq!(
        fs::read_to_string(root.join("prompt/cache.md")).unwrap(),
        before
    );
}

#[test]
fn the_form_never_writes_a_fixture_secret() {
    let (t, home, root) = fresh();
    let content = t.path().join("clip");
    fs::write(
        &content,
        "aws configure set aws_access_key_id AKIAIOSFODNN7EXAMPLE\naws s3 ls\n",
    )
    .unwrap();
    let (tool, extra) = if cfg!(target_os = "macos") {
        ("pbpaste", vec![])
    } else {
        ("wl-paste", vec![("WAYLAND_DISPLAY", "wayland-test")])
    };
    let mut spec = Spec::new(&[], &home, &root)
        .env("KB_TEST_NOW", NOW)
        .with_fake_tool(tool, &format!("cat '{}'", content.display()));
    for (k, v) in extra {
        spec = spec.env(k, v);
    }
    let mut s = Session::spawn(&spec);
    s.wait_for("entries");
    s.send(b"\x0e");
    s.wait_for("New entry");
    s.wait_for("detected cmd");
    s.type_text("authoring leak");
    s.send(b"\x13");
    s.wait_for("KB060");
    s.send(b"\x13");
    s.wait_for("KB060");
    assert!(!root.join("cmd/authoring-leak.md").exists());
    s.send(b"\x03");
    assert!(leaks(&root).is_empty());
}
