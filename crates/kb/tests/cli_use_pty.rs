//! `kb use` prompts on the controlling terminal, under a pseudo-terminal.

mod support;

use std::fs;

use support::pty::{ENTER, Session, Spec, kb_home};

const CTRL_D: &[u8] = b"\x04";

fn spawn(
    args: &[&str],
    t: &tempfile::TempDir,
    home: &std::path::Path,
    root: &std::path::Path,
) -> (Session, std::path::PathBuf) {
    let out = t.path().join("stdout");
    let mut spec = Spec::new(args, home, root);
    spec.out = Some(out.clone());
    (Session::spawn(&spec), out)
}

#[test]
fn prompts_show_the_prefill_and_enter_accepts_it_even_with_stdout_redirected() {
    let (t, home, root) = kb_home("vars");
    let (mut s, out) = spawn(&["use", "cmd/repeat", "--print"], &t, &home, &root);
    s.wait_for("name (text) [web]: ");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(fs::read_to_string(out).unwrap(), "echo web && echo web\n");
}

#[test]
fn a_wrong_answer_is_explained_and_asked_again() {
    let (t, home, root) = kb_home("vars");
    let (mut s, out) = spawn(
        &["use", "sumo/5xx-by-authorizer", "--print"],
        &t,
        &home,
        &root,
    );
    s.wait_for("env (enum: prod|stage|dev): ");
    s.type_text("qa");
    s.send(ENTER);
    s.wait_for("env: expected one of prod, stage, dev");
    s.type_text("prod");
    s.send(ENTER);
    s.wait_for("minutes (number) [15]: ");
    s.type_text("abc");
    s.send(ENTER);
    s.wait_for("minutes: expected a number like 30, -5 or 0.25");
    s.type_text("30");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert_eq!(
        fs::read_to_string(out).unwrap(),
        "_sourceCategory=prod/api | where status >= 500 | timeslice 30m | count by authorizer\n"
    );
}

/// SC-005: what you type at the prompts and what you give with `--var` produce the same bytes.
#[test]
fn interactive_and_scripted_runs_produce_the_same_bytes() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        (
            "sumo/5xx-by-authorizer",
            &[("env", "stage"), ("minutes", "7")],
        ),
        ("cmd/git-checkout", &[("branch", "feature/x; rm -rf ~")]),
        ("cmd/grep-quoted", &[("pattern", "a \"b\" $HOME")]),
        ("link/search", &[("query", "a&b c")]),
        ("sql/escape-sql", &[("name", "O'Neil")]),
    ];
    for (id, answers) in cases {
        let (t, home, root) = kb_home("vars");
        let (mut s, typed_out) = spawn(&["use", id, "--print"], &t, &home, &root);
        for (name, value) in *answers {
            s.wait_for(&format!("{name} ("));
            s.type_text(value);
            s.send(ENTER);
        }
        assert_eq!(s.wait_exit(), 0, "{id}: {}", s.screen());
        let typed = fs::read(typed_out).unwrap();

        let (t2, home2, root2) = kb_home("vars");
        let mut args: Vec<String> =
            vec!["use".into(), (*id).into(), "--print".into(), "--yes".into()];
        for (name, value) in *answers {
            args.push("--var".into());
            args.push(format!("{name}={value}"));
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (mut s2, scripted_out) = spawn(&refs, &t2, &home2, &root2);
        assert_eq!(s2.wait_exit(), 0, "{id}");
        assert_eq!(typed, fs::read(scripted_out).unwrap(), "{id}");
    }
}

#[test]
fn a_typed_secret_is_masked_and_never_stored() {
    let (t, home, root) = kb_home("vars");
    let (mut s, out) = spawn(&["use", "cmd/curl-token", "--print"], &t, &home, &root);
    s.wait_for("token (secret): ");
    s.type_text("S3CR3T-MARKER");
    // nothing is echoed while typing
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(!s.screen().contains("S3CR3T"), "{}", s.screen());
    assert!(!s.raw_output().contains("S3CR3T"));
    s.send(ENTER);
    s.wait_for("path (text) [health]: ");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    assert!(!s.screen().contains("S3CR3T") && !s.raw_output().contains("S3CR3T"));
    let printed = fs::read_to_string(out).unwrap();
    assert_eq!(
        printed,
        "curl -H \"Authorization: Bearer S3CR3T-MARKER\" https://api.example.com/health\n"
    );
    let state = fs::read_to_string(root.join(".kb/state.json")).unwrap();
    assert!(!state.contains("S3CR3T"), "{state}");
    assert!(state.contains("health"));
}

#[test]
fn escape_at_a_secret_prompt_and_end_of_input_cancel_with_130() {
    let (t, home, root) = kb_home("vars");
    let (mut s, out) = spawn(&["use", "cmd/curl-token", "--print"], &t, &home, &root);
    s.wait_for("token (secret): ");
    s.send(b"\x1b");
    assert_eq!(s.wait_exit(), 130, "{}", s.screen());
    assert_eq!(fs::read_to_string(out).unwrap(), "");
    assert!(!root.join(".kb/state.json").exists());

    let (t, home, root) = kb_home("vars");
    let (mut s, _) = spawn(&["use", "cmd/repeat", "--print"], &t, &home, &root);
    s.wait_for("name (text) [web]: ");
    s.send(CTRL_D);
    assert_eq!(s.wait_exit(), 130, "{}", s.screen());
    assert!(s.screen().contains("cancelled"));
}

#[test]
fn close_matches_open_the_picker_and_say_that_var_is_ignored() {
    let (t, home, root) = kb_home("vars");
    let (mut s, _out) = spawn(
        &["use", "echo", "--var", "name=x", "--print"],
        &t,
        &home,
        &root,
    );
    s.wait_for("results");
    s.wait_for("Esc clear");
    // the UI owns the screen; the notice is printed before it opens and is gone from the screen,
    // but the picker is there with the words in the search line
    assert!(
        s.raw_output()
            .contains("--var and --yes are ignored when you choose in the UI")
    );
    assert!(s.screen().contains("echo"));
    s.send(b"\x03");
    assert_eq!(s.wait_exit(), 130);
}
