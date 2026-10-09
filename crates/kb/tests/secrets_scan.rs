//! SC-004: a secret used through every path leaves no trace outside the clipboard (and `--print`,
//! whose whole point is to print it).

mod support;

use std::fs;
use std::path::Path;

use support::{fake_clipboard, kb_in, path_with, vars_kb};

const MARKER: &str = "S3CR3T-MARKER";

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

fn files_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                files_under(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

fn kb_with_fake(base: &Path, root: &Path, args: &[&str], token_env: bool) -> std::process::Output {
    let (_tool, dir, _copied, env) = fake_clipboard(base);
    let mut cmd = kb_in(root);
    cmd.args(args).env("PATH", path_with(&dir));
    for (k, v) in env {
        cmd.env(k, v);
    }
    if token_env {
        cmd.env("KB_TEST_TOKEN", MARKER);
    }
    cmd.output().unwrap()
}

#[test]
fn a_secret_leaves_no_trace_but_the_clipboard() {
    let (d, root) = vars_kb();
    let mut streams = String::new();
    let mut run = |args: &[&str], token_env: bool| {
        let out = kb_with_fake(d.path(), &root, args, token_env);
        streams.push_str(&text(&out.stdout));
        streams.push_str(&text(&out.stderr));
        out
    };
    // from the environment, copied
    let out = run(&["use", "cmd/curl-token", "-y"], true);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("the clipboard now holds a secret"),
        "{}",
        text(&out.stderr)
    );
    // on the command line, with the history warning
    let out = run(
        &["use", "cmd/curl-token", "--var", &format!("token={MARKER}")],
        false,
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stderr).contains("may be saved in your shell history"));
    assert_eq!(text(&out.stderr).matches("shell history").count(), 1);
    // JSON, variables listing, and runs that fail
    run(&["use", "cmd/curl-token", "-y", "--json"], true);
    run(&["vars", "cmd/curl-token"], true);
    run(&["vars", "cmd/curl-token", "--json"], true);
    let out = run(
        &[
            "use",
            "cmd/curl-token",
            "--var",
            &format!("token={MARKER}"),
            "--var",
            "nope=1",
        ],
        false,
    );
    assert_eq!(out.status.code(), Some(2));
    let out = run(&["use", "cmd/curl-token", "-y"], false);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("token  secret"),
        "{}",
        text(&out.stderr)
    );

    assert!(
        !streams.contains(MARKER),
        "a secret reached an output stream:\n{streams}"
    );

    // nothing on disk but the fake clipboard's own file
    let mut files = Vec::new();
    files_under(d.path(), &mut files);
    for f in files {
        if f.file_name().is_some_and(|n| n == "copied") {
            continue; // the clipboard: the one place the secret is meant to be
        }
        if let Ok(bytes) = fs::read(&f) {
            assert!(
                !String::from_utf8_lossy(&bytes).contains(MARKER),
                "{} holds the secret",
                f.display()
            );
        }
    }
    // the memory has the other variable but not the secret
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".kb/state.json")).unwrap()).unwrap();
    assert!(
        state["vars"]["entries"]["cmd/curl-token"]
            .get("token")
            .is_none()
    );
    assert!(state["vars"]["recent"].get("token").is_none());
}

#[test]
fn print_is_the_one_stream_that_carries_it() {
    let (d, root) = vars_kb();
    let out = kb_with_fake(
        d.path(),
        &root,
        &["use", "cmd/curl-token", "-y", "--print"],
        true,
    );
    assert!(text(&out.stdout).contains(MARKER));
    assert!(!text(&out.stderr).contains(MARKER));
}

#[test]
fn a_secret_without_a_source_is_missing_when_nobody_can_be_asked() {
    let (d, root) = vars_kb();
    let out = kb_with_fake(d.path(), &root, &["use", "cmd/curl-token", "-y"], false);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stdout).is_empty());
}

/// The `stdout` clipboard method is the last resort when nothing else can take the text; it prints
/// the text, as `--print` does, so a secret substituted into it is on stdout (and only there).
/// This is documented in the README and `contracts/cli.md`.
#[test]
fn the_stdout_fallback_prints_the_secret_like_print_does_and_nowhere_else() {
    let (d, root) = vars_kb();
    for args in [
        vec!["use", "cmd/curl-token", "-y"],
        vec!["use", "cmd/curl-token", "-y", "--json"],
    ] {
        let out = kb_in(&root)
            .args(&args)
            .env("KB_CLIPBOARD_METHOD", "stdout")
            .env("KB_TEST_TOKEN", MARKER)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        assert!(text(&out.stdout).contains(MARKER), "{args:?}");
        assert!(!text(&out.stderr).contains(MARKER), "{args:?}");
    }
    let mut files = Vec::new();
    files_under(d.path(), &mut files);
    files_under(&root, &mut files);
    for f in files {
        if let Ok(bytes) = fs::read(&f) {
            assert!(
                !String::from_utf8_lossy(&bytes).contains(MARKER),
                "{}",
                f.display()
            );
        }
    }
}
