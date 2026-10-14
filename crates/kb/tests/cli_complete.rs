//! CLI integration tests for `kb __complete` and `kb completions`.

mod support;

use support::{kb_in, search_kb};

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn warm_search_kb() -> (tempfile::TempDir, std::path::PathBuf) {
    let (d, root) = search_kb();
    let out = kb_in(&root).arg("reindex").output().unwrap();
    assert!(out.status.success());
    (d, root)
}

#[test]
fn test_cli_complete_types() {
    let (_d, root) = search_kb();
    let out = kb_in(&root).args(["__complete", "types"]).output().unwrap();
    assert!(out.status.success());
    let s = stdout(&out);
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(
        lines,
        vec!["cmd", "sql", "sumo", "cw", "prompt", "link", "code"]
    );

    let out_filtered = kb_in(&root)
        .args(["__complete", "types", "c"])
        .output()
        .unwrap();
    assert!(out_filtered.status.success());
    let s_filtered = stdout(&out_filtered);
    let lines_filtered: Vec<&str> = s_filtered.lines().collect();
    assert_eq!(lines_filtered, vec!["cmd", "cw", "code"]);
}

#[test]
fn test_cli_complete_entries() {
    let (_d, root) = warm_search_kb();
    let out = kb_in(&root)
        .args(["__complete", "entries"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(!s.is_empty(), "must return entries");
    assert!(
        s.contains("cmd/restart-worker\tRestart worker"),
        "must format as <id>\\t<title>"
    );

    let out_filtered = kb_in(&root)
        .args(["__complete", "entries", "cmd/restart"])
        .output()
        .unwrap();
    assert!(out_filtered.status.success());
    let s_filtered = stdout(&out_filtered);
    assert!(s_filtered.contains("cmd/restart-worker\tRestart worker"));
    assert!(!s_filtered.contains("sumo/"));
}

#[test]
fn test_cli_complete_tags() {
    let (_d, root) = warm_search_kb();
    let out = kb_in(&root).args(["__complete", "tags"]).output().unwrap();
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.lines().any(|l| l == "ops"));

    let out_filtered = kb_in(&root)
        .args(["__complete", "tags", "op"])
        .output()
        .unwrap();
    assert!(out_filtered.status.success());
    let s_filtered = stdout(&out_filtered);
    assert_eq!(s_filtered.trim(), "ops");
}

#[test]
fn test_cli_complete_vars() {
    let (_d, root) = warm_search_kb();
    let out = kb_in(&root)
        .args(["__complete", "vars", "cmd/deploy-service"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = stdout(&out);
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(lines, vec!["env=", "region="]);

    let out_filtered = kb_in(&root)
        .args(["__complete", "vars", "cmd/deploy-service", "e"])
        .output()
        .unwrap();
    assert!(out_filtered.status.success());
    let s_filtered = stdout(&out_filtered);
    assert_eq!(s_filtered.trim(), "env=");
}

#[test]
fn test_cli_complete_var_values() {
    let (_d, root) = warm_search_kb();
    let out = kb_in(&root)
        .args(["__complete", "var-values", "cmd/deploy-service", "env"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = stdout(&out);
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(lines, vec!["prod", "stage"]);

    let out_filtered = kb_in(&root)
        .args(["__complete", "var-values", "cmd/deploy-service", "env", "p"])
        .output()
        .unwrap();
    assert!(out_filtered.status.success());
    let s_filtered = stdout(&out_filtered);
    assert_eq!(s_filtered.trim(), "prod");
}

#[test]
fn test_cli_completions_generation() {
    let (_d, root) = search_kb();

    // zsh
    let out_zsh = kb_in(&root).args(["completions", "zsh"]).output().unwrap();
    assert!(out_zsh.status.success());
    let s_zsh = stdout(&out_zsh);
    assert!(s_zsh.contains("#compdef kb"));
    assert!(s_zsh.contains("_kb_custom"));
    assert!(s_zsh.contains("_kb_entries"));
    assert!(s_zsh.contains("compdef _kb_custom kb"));

    // bash
    let out_bash = kb_in(&root).args(["completions", "bash"]).output().unwrap();
    assert!(out_bash.status.success());
    let s_bash = stdout(&out_bash);
    assert!(s_bash.contains("_kb_custom"));
    assert!(s_bash.contains("complete -F _kb_custom"));

    // fish
    let out_fish = kb_in(&root).args(["completions", "fish"]).output().unwrap();
    assert!(out_fish.status.success());
    let s_fish = stdout(&out_fish);
    assert!(s_fish.contains("complete -c kb"));
    assert!(s_fish.contains("__complete entries"));
}
