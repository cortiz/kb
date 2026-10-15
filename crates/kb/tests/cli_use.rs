//! `kb use` and `kb copy` with variables, from the shell. A fake clipboard program stands in for
//! the real one. Tests never let kb prompt on the developer's terminal: they give every value with
//! `--var`, pass `--yes`, or run without a controlling terminal.

mod support;

use std::fs;
use std::path::Path;

use serde_json::Value;
use support::{fake_clipboard, kb_in, kb_without_terminal, path_with, vars_kb};

const FIVE_XX: &str = "sumo/5xx-by-authorizer";
const FIVE_XX_QUERY: &str =
    "_sourceCategory=prod/api | where status >= 500 | timeslice 30m | count by authorizer\n";

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

/// `kb --root <root> <args>` with the fake clipboard first on `PATH`; returns what was copied.
fn run(base: &Path, root: &Path, args: &[&str]) -> (std::process::Output, Option<String>) {
    let (_tool, dir, copied, env) = fake_clipboard(base);
    let mut cmd = kb_in(root);
    cmd.args(args).env("PATH", path_with(&dir));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    (out, fs::read_to_string(copied).ok())
}

fn state(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join(".kb/state.json")).unwrap()).unwrap()
}

#[test]
fn every_variable_given_copies_without_asking() {
    let (d, root) = vars_kb();
    let (out, copied) = run(
        d.path(),
        &root,
        &["use", FIVE_XX, "--var", "env=prod", "--var", "minutes=30"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        copied.as_deref(),
        Some(FIVE_XX_QUERY.trim_end_matches('\n'))
    );
    assert!(
        text(&out.stderr).contains(&format!("Copied {FIVE_XX}")),
        "{}",
        text(&out.stderr)
    );
    let s = state(&root);
    assert_eq!(s["usage"][FIVE_XX]["count"], 1);
    assert_eq!(s["vars"]["entries"][FIVE_XX]["env"], "prod");
    assert_eq!(s["vars"]["entries"][FIVE_XX]["minutes"], "30");
    assert_eq!(s["vars"]["recent"]["env"][0], "prod");
}

#[test]
fn print_writes_the_resolved_text_to_stdout_and_copies_nothing() {
    let (d, root) = vars_kb();
    let (out, copied) = run(
        d.path(),
        &root,
        &[
            "use",
            FIVE_XX,
            "--var",
            "env=stage",
            "--var",
            "minutes=5",
            "--print",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "_sourceCategory=stage/api | where status >= 500 | timeslice 5m | count by authorizer\n"
    );
    assert_eq!(copied, None);
    assert_eq!(state(&root)["usage"][FIVE_XX]["count"], 1);
}

#[test]
fn yes_takes_the_prefilled_values_and_remembered_ones_come_next_time() {
    let (d, root) = vars_kb();
    let (out, _) = run(d.path(), &root, &["use", "cmd/repeat", "-y", "--print"]);
    assert_eq!(text(&out.stdout), "echo web && echo web\n");
    // a value used once pre-fills the next run
    let (out, _) = run(
        d.path(),
        &root,
        &["use", "cmd/repeat", "--var", "name=api", "--print"],
    );
    assert_eq!(text(&out.stdout), "echo api && echo api\n");
    let (out, _) = run(d.path(), &root, &["use", "cmd/repeat", "-y", "--print"]);
    assert_eq!(text(&out.stdout), "echo api && echo api\n");
    // 5xx: the enum has no default, so it is missing until remembered
    let (out, _) = run(
        d.path(),
        &root,
        &["use", FIVE_XX, "--var", "env=dev", "-y", "--print"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let (out, _) = run(d.path(), &root, &["use", FIVE_XX, "-y", "--print"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("dev/api") && text(&out.stdout).contains("timeslice 15m"));
}

#[test]
fn without_a_terminal_missing_values_are_listed_and_nothing_is_copied() {
    let (d, root) = vars_kb();
    let out = kb_without_terminal(
        &[
            "--root",
            root.to_str().unwrap(),
            "use",
            FIVE_XX,
            "--var",
            "minutes=5",
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let err = text(&out.stderr);
    assert!(
        err.contains("missing values:") && err.contains("env  enum: prod|stage|dev"),
        "{err}"
    );
    assert_eq!(text(&out.stdout), "");
    assert!(!root.join(".kb/state.json").exists());
    drop(d);
}

#[test]
fn yes_also_reports_what_has_nothing_to_start_from() {
    let (d, root) = vars_kb();
    let (out, copied) = run(d.path(), &root, &["use", FIVE_XX, "-y"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("missing values:"));
    assert_eq!(copied, None);
}

#[test]
fn usage_errors_are_exit_2() {
    let (d, root) = vars_kb();
    let (out, _) = run(d.path(), &root, &["use", FIVE_XX, "--var", "nope=1"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("it uses: env, minutes"),
        "{}",
        text(&out.stderr)
    );
    let (out, _) = run(d.path(), &root, &["use", FIVE_XX, "--var", "novalue"]);
    assert_eq!(out.status.code(), Some(2));
    let (out, _) = run(
        d.path(),
        &root,
        &["use", FIVE_XX, "--var", "env=prod", "--var", "minutes=abc"],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("minutes: expected a number like 30, -5 or 0.25"),
        "{}",
        text(&out.stderr)
    );
    let (out, _) = run(
        d.path(),
        &root,
        &["use", FIVE_XX, "--var", "env=qa", "--var", "minutes=1"],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("expected one of prod, stage, dev"));
    let (out, copied) = run(d.path(), &root, &["use", "cmd/no-vars", "--var", "x=1"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("it uses: none"));
    assert_eq!(copied, None);
}

#[test]
fn nothing_found_is_exit_1() {
    let (d, root) = vars_kb();
    let (out, _) = run(d.path(), &root, &["use", "zzzz-no-such-thing"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn json_describes_the_variables_without_secrets() {
    let (d, root) = vars_kb();
    let (out, _) = run(
        d.path(),
        &root,
        &[
            "use",
            FIVE_XX,
            "--var",
            "env=prod",
            "--var",
            "minutes=30",
            "--json",
        ],
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["id"], FIVE_XX);
    assert_eq!(doc["sent"], true);
    assert_eq!(doc["secret"], false);
    assert_eq!(doc["variables"][0]["name"], "env");
    assert_eq!(doc["variables"][0]["source"], "flag");
    assert_eq!(doc["variables"][0]["value"], "prod");
    let (out, _) = run(
        d.path(),
        &root,
        &[
            "use",
            FIVE_XX,
            "--var",
            "env=prod",
            "--var",
            "minutes=30",
            "--print",
            "--json",
        ],
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["text"], FIVE_XX_QUERY);
    assert_eq!(doc["method"], "print");
}

#[test]
fn no_remember_and_the_setting_leave_no_values_behind() {
    let (d, root) = vars_kb();
    let (out, _) = run(
        d.path(),
        &root,
        &[
            "use",
            "cmd/repeat",
            "--var",
            "name=api",
            "--no-remember",
            "--print",
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    let s = state(&root);
    assert_eq!(s["usage"]["cmd/repeat"]["count"], 1);
    assert!(s.get("vars").is_none(), "{s}");
    let (_, _) = {
        let (_tool, dir, _copied, env) = fake_clipboard(d.path());
        let mut cmd = kb_in(&root);
        cmd.args(["use", "cmd/repeat", "--var", "name=x", "--print"])
            .env("PATH", path_with(&dir))
            .env("KB_VARS_REMEMBER", "false");
        for (k, v) in env {
            cmd.env(k, v);
        }
        (cmd.output().unwrap(), ())
    };
    assert!(state(&root).get("vars").is_none());
}

#[test]
fn copy_resolves_and_raw_copies_the_template() {
    let (d, root) = vars_kb();
    let (out, copied) = run(
        d.path(),
        &root,
        &["copy", FIVE_XX, "--var", "env=prod", "--var", "minutes=30"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        copied.as_deref(),
        Some(FIVE_XX_QUERY.trim_end_matches('\n'))
    );
    let (out, copied) = run(d.path(), &root, &["copy", FIVE_XX, "--raw"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(copied.unwrap().contains("{{env}}"));
    let (out, _) = run(
        d.path(),
        &root,
        &["copy", FIVE_XX, "--raw", "--var", "env=prod"],
    );
    assert_eq!(out.status.code(), Some(2));
    // an entry without variables is copied exactly as before
    let (out, copied) = run(d.path(), &root, &["copy", "cmd/no-vars"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(copied.as_deref(), Some("echo plain"));
    let (out, _) = run(d.path(), &root, &["copy", "cmd/no-vars", "--print"]);
    assert_eq!(text(&out.stdout), "echo plain\n");
}

#[test]
fn values_land_safely_in_commands_urls_and_queries() {
    let (d, root) = vars_kb();
    let print = |args: &[&str]| {
        let mut a = vec!["use"];
        a.extend_from_slice(args);
        a.push("--print");
        let (out, _) = run(d.path(), &root, &a);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        text(&out.stdout)
    };
    assert_eq!(
        print(&["cmd/git-checkout", "--var", "branch=feature/x; rm -rf ~"]),
        "git checkout 'feature/x; rm -rf ~'\n"
    );
    assert_eq!(
        print(&["cmd/git-checkout", "--var", "branch=main"]),
        "git checkout main\n"
    );
    assert_eq!(
        print(&["link/search", "--var", "query=a&b c"]),
        "https://wiki.example.com/search?q=a%26b%20c"
    );
    assert_eq!(
        print(&["sql/escape-sql", "--var", "name=O'Neil"]),
        "select * from customers where name = 'O''Neil';\n"
    );
    assert_eq!(
        print(&["cmd/escape-none", "--var", "args=-l; ls"]),
        "ls -l; ls\n"
    );
    assert_eq!(
        print(&["cmd/grep-quoted", "--var", "pattern=a \"b\" $HOME"]),
        "grep \"a \\\"b\\\" \\$HOME\" app.log\necho 'a \"b\" $HOME'\n"
    );
    assert_eq!(
        print(&["cmd/escaped-braces", "--var", "name=x"]),
        "echo {{literal}} x\n"
    );
    assert_eq!(
        print(&["prompt/default-empty", "-y"]),
        "Summarize this log.\n"
    );
}

#[test]
fn an_unusable_declared_default_leaves_the_value_missing() {
    let (d, root) = vars_kb();
    let (out, _) = run(d.path(), &root, &["use", "cmd/bad-default", "-y"]);
    assert_eq!(out.status.code(), Some(1));
    let (out, _) = run(
        d.path(),
        &root,
        &["use", "cmd/bad-default", "--var", "count=10", "--print"],
    );
    assert_eq!(text(&out.stdout), "head -n 10 file\n");
}

#[test]
fn a_run_with_stdout_redirected_still_succeeds() {
    // `--print` output is exactly the text: nothing else on stdout
    let (d, root) = vars_kb();
    let (out, _) = run(d.path(), &root, &["use", "cmd/fields", "-y", "--print"]);
    assert_eq!(text(&out.stdout), "ls\n");
}

#[test]
fn json_carries_the_other_resolved_fields() {
    let (d, root) = vars_kb();
    let (out, _) = run(d.path(), &root, &["use", "cmd/fields", "-y", "--json"]);
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["fields"]["cwd"], "/tmp");
    let (out, _) = run(
        d.path(),
        &root,
        &["use", "cw/log-groups", "--var", "svc=api", "--json"],
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["fields"]["log_groups[0]"], "/aws/lambda/api");
    let (out, _) = run(
        d.path(),
        &root,
        &["use", "sumo/range", "-y", "--print", "--json"],
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["fields"]["range"], "-15m");
}

#[test]
fn use_runbook_copies_resolved_checklist_and_records_usage() {
    let (d, root) = vars_kb();
    fs::write(
        root.join("runbook-deploy.md"),
        r#"---
kb_format: 1
type: runbook
title: Deploy service runbook
vars:
  env: { kind: enum, options: [dev, stage, prod], default: stage }
steps:
  - id: step-1
    title: Check 5xx
    entry: sumo/5xx-by-authorizer
---

Follow carefully.
"#,
    )
    .unwrap();

    let (out, copied) = run(
        d.path(),
        &root,
        &["use", "runbook-deploy", "--var", "env=prod"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let copied_text = copied.expect("copied text");
    assert!(copied_text.contains("# Deploy service runbook"));
    assert!(copied_text.contains("- [ ] 1. Check 5xx (`sumo/5xx-by-authorizer`)"));
    assert!(copied_text.contains("```sumo"));
    assert!(copied_text.contains("_sourceCategory=prod/api"));

    let s = state(&root);
    assert_eq!(s["usage"]["runbook-deploy"]["count"], 1);
}
