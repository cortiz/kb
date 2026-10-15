//! `kb lint` end to end (User Story 1).

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A temporary KB holding a copy of `tests/fixtures/valid`.
fn valid_kb() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    copy_dir(&fixtures().join("valid"), &root);
    (dir, root)
}

fn count_md(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| {
            if p.is_dir() {
                count_md(&p)
            } else {
                usize::from(p.extension().is_some_and(|e| e == "md"))
            }
        })
        .sum()
}

fn kb() -> Command {
    let mut cmd = Command::cargo_bin("kb").unwrap();
    cmd.env_remove("KB_ROOT")
        .env("HOME", "/nonexistent-home")
        .env("XDG_CONFIG_HOME", "/nonexistent-config");
    cmd
}

fn stdout(cmd: &mut Command) -> (String, i32) {
    let out = cmd.output().unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap(),
    )
}

#[test]
fn clean_kb_prints_summary_and_exits_0() {
    let (_d, root) = valid_kb();
    let n = count_md(&root);
    let (out, code) = stdout(kb().arg("--root").arg(&root).arg("lint"));
    assert_eq!(out, format!("{n} entries, 0 errors, 0 warnings\n"));
    assert_eq!(code, 0);
}

#[test]
fn unknown_type_is_reported_with_fix() {
    let (_d, root) = valid_kb();
    fs::write(
        root.join("sumo/bad.md"),
        "---\nkb_format: 1\ntype: sumologic\ntitle: T\n---\n\n```\nx\n```\n",
    )
    .unwrap();
    let (out, code) = stdout(kb().arg("--root").arg(&root).arg("lint"));
    assert!(
        out.contains("sumo/bad.md:3:7: error[KB001-unknown-type]: unknown type \"sumologic\"; expected one of sql, sumo, cw, cmd, prompt, link, code, note, runbook\n  fix: use \"sumo\"\n"),
        "{out}"
    );
    assert!(out.ends_with(", 1 error, 0 warnings\n"), "{out}");
    assert_eq!(code, 1);
}

#[test]
fn warnings_fail_only_under_strict() {
    let (_d, root) = valid_kb();
    fs::write(root.join("sql/unused.md"), "---\nkb_format: 1\ntype: sql\ntitle: T\nvars:\n  region: { kind: text }\n---\n\n```\nselect 1;\n```\n").unwrap();
    let (out, code) = stdout(kb().arg("--root").arg(&root).arg("lint"));
    assert!(
        out.contains("sql/unused.md:6:3: warning[KB048-unused-var]"),
        "{out}"
    );
    assert_eq!(code, 0);
    let (_, code) = stdout(kb().arg("--root").arg(&root).args(["lint", "--strict"]));
    assert_eq!(code, 1);
}

#[test]
fn single_file_from_another_directory() {
    let (dir, root) = valid_kb();
    fs::write(root.join("sql/bad.md"), "not an entry\n").unwrap();
    let (out, code) = stdout(
        kb().current_dir(dir.path())
            .arg("--root")
            .arg(&root)
            .args(["lint", "sumo/5xx-by-authorizer.md"]),
    );
    assert_eq!(out, "1 entry, 0 errors, 0 warnings\n");
    assert_eq!(code, 0);
}

#[test]
fn directory_argument_checks_only_that_directory() {
    let (dir, root) = valid_kb();
    fs::write(root.join("sql/bad.md"), "not an entry\n").unwrap();
    let (out, code) = stdout(
        kb().current_dir(dir.path())
            .arg("--root")
            .arg(&root)
            .args(["lint", "sumo"]),
    );
    assert_eq!(out, "1 entry, 0 errors, 0 warnings\n");
    assert_eq!(code, 0);
    let (_, code) = stdout(
        kb().current_dir(dir.path())
            .arg("--root")
            .arg(&root)
            .args(["lint", "sql"]),
    );
    assert_eq!(code, 1);
}

#[test]
fn relative_path_in_cwd_wins_and_outside_root_is_usage_error() {
    let (dir, root) = valid_kb();
    let cwd = dir.path().join("elsewhere");
    fs::create_dir_all(cwd.join("sumo")).unwrap();
    fs::write(cwd.join("sumo/5xx-by-authorizer.md"), "x").unwrap();
    let out = kb()
        .current_dir(&cwd)
        .arg("--root")
        .arg(&root)
        .args(["lint", "sumo/5xx-by-authorizer.md"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.starts_with("kb: ") && err.contains("outside the KB root"),
        "{err}"
    );
}

#[test]
fn missing_paths_are_usage_errors() {
    let (dir, root) = valid_kb();
    let out = kb()
        .current_dir(dir.path())
        .arg("--root")
        .arg(&root)
        .args(["lint", "nope.md"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = kb()
        .arg("--root")
        .arg(dir.path().join("missing"))
        .arg("lint")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .starts_with("kb: KB root ")
    );
}

#[test]
fn quiet_prints_only_the_summary() {
    let (_d, root) = valid_kb();
    fs::write(root.join("sql/bad.md"), "not an entry\n").unwrap();
    let n = count_md(&root);
    let (out, code) = stdout(kb().arg("--root").arg(&root).args(["lint", "--quiet"]));
    assert_eq!(out, format!("{n} entries, 1 error, 0 warnings\n"));
    assert_eq!(code, 1);
}

#[test]
fn info_and_suppressed_counts_and_lines() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    copy_dir(&fixtures().join("secrets"), &root);
    for name in [
        "aws-access-key",
        "aws-secret-key",
        "bearer-token",
        "jwt",
        "private-key",
        "high-entropy",
        "in-frontmatter",
    ] {
        fs::remove_file(root.join(format!("{name}.md"))).unwrap();
    }
    fs::write(
        root.join("info.md"),
        "---\nkb_format: 1\ntype: sql\ntitle: T\n---\n\n```\nselect '{{who}}';\n```\n",
    )
    .unwrap();
    let (out, code) = stdout(kb().arg("--root").arg(&root).arg("lint"));
    assert!(
        out.contains("suppressed.md:8: suppressed[KB062-secret-bearer-token]\n"),
        "{out}"
    );
    assert!(
        out.contains("suppressed.md:9: suppressed[KB062-secret-bearer-token]\n"),
        "{out}"
    );
    assert!(
        out.ends_with("3 entries, 0 errors, 0 warnings, 1 info, 2 suppressed\n"),
        "{out}"
    );
    assert_eq!(code, 0);
    let (out, _) = stdout(kb().arg("--root").arg(&root).args(["lint", "--quiet"]));
    assert_eq!(
        out,
        "3 entries, 0 errors, 0 warnings, 1 info, 2 suppressed\n"
    );
}

fn keys(v: &serde_json::Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

#[test]
fn json_report_matches_the_schema() {
    let (_d, root) = valid_kb();
    fs::write(
        root.join("sql/bad.md"),
        "---\nkb_format: 1\ntype: sql\ntitle: T\nowner: me\n---\n\n```\nselect 1;\n```\n",
    )
    .unwrap();
    let before = fs::read(root.join("sql/bad.md")).unwrap();
    let (out, code) = stdout(
        kb().arg("--root")
            .arg(&root)
            .args(["lint", "--json", "--strict"]),
    );
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).expect("one JSON document");
    assert_eq!(
        keys(&v),
        vec!["findings", "root", "summary", "suppressions", "version"]
    );
    assert_eq!(v["version"], 1);
    assert_eq!(
        keys(&v["summary"]),
        vec![
            "entries",
            "errors",
            "failed",
            "info",
            "suppressed",
            "warnings"
        ]
    );
    assert_eq!(v["summary"]["info"], 1);
    assert_eq!(v["summary"]["failed"], false);
    let f = &v["findings"][0];
    assert_eq!(
        keys(f),
        vec![
            "code", "column", "fix", "id", "line", "message", "path", "severity"
        ]
    );
    assert_eq!(f["severity"], "info");
    assert_eq!(f["code"], "KB016-unknown-key");
    assert_eq!(f["path"], "sql/bad.md");
    assert_eq!(f["id"], "sql/bad");
    assert_eq!(
        (f["line"].as_u64(), f["column"].as_u64()),
        (Some(5), Some(1))
    );
    assert!(f["fix"].is_null());
    assert_eq!(
        fs::read(root.join("sql/bad.md")).unwrap(),
        before,
        "lint never changes files"
    );
}

#[test]
fn json_codes_and_severities_are_well_formed() {
    let root = fixtures().join("invalid");
    let (out, code) = stdout(kb().arg("--root").arg(&root).args(["lint", "--json"]));
    assert_eq!(code, 1);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["summary"]["failed"], true);
    for f in v["findings"].as_array().unwrap() {
        let code = f["code"].as_str().unwrap();
        assert!(
            code.len() > 6
                && code.starts_with("KB")
                && code[2..5].bytes().all(|b| b.is_ascii_digit())
                && &code[5..6] == "-",
            "{code}"
        );
        assert!(["error", "warning", "info"].contains(&f["severity"].as_str().unwrap()));
    }
}

#[test]
fn json_suppressions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    fs::create_dir_all(&root).unwrap();
    fs::copy(fixtures().join("secrets/suppressed.md"), root.join("s.md")).unwrap();
    let (out, _) = stdout(kb().arg("--root").arg(&root).args(["lint", "--json"]));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["summary"]["suppressed"], 2);
    assert_eq!(keys(&v["suppressions"][0]), vec!["code", "line", "path"]);
}

#[test]
fn json_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let out = kb()
        .arg("--root")
        .arg(dir.path().join("missing"))
        .args(["lint", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8(out.stdout).unwrap()).unwrap();
    assert_eq!(v["error"]["kind"], "usage");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not exist")
    );
}

#[test]
fn broken_entry_links_in_notes_are_warnings_with_a_suggestion() {
    let (_d, root) = valid_kb();
    fs::create_dir_all(root.join("note")).unwrap();
    fs::write(
        root.join("note/links.md"),
        "---\nkb_format: 1\ntype: note\ntitle: Links\n---\n\n# Links\n\nGood: [[sumo/5xx-by-authorizer]].\nTypo: [[sumo/5xx-by-authorzer]].\nGone: [[nowhere/at-all]].\nEscaped: \\[[nowhere/escaped]] and `[[nowhere/code]]`.\n",
    )
    .unwrap();
    let (out, code) = stdout(kb().arg("--root").arg(&root).arg("lint"));
    assert_eq!(code, 0, "warnings do not fail: {out}");
    assert!(
        out.contains("note/links.md:10:7: warning[KB074-entry-link-broken]"),
        "{out}"
    );
    assert!(out.contains("[[sumo/5xx-by-authorzer]]"), "{out}");
    assert!(
        out.contains("did you mean `sumo/5xx-by-authorizer`?"),
        "{out}"
    );
    assert!(
        out.contains("note/links.md:11:7: warning[KB074-entry-link-broken]"),
        "{out}"
    );
    assert!(
        !out.contains("escaped") && !out.contains("nowhere/code"),
        "{out}"
    );
    assert!(!out.contains(":9:"), "the good link is fine: {out}");
    assert!(out.ends_with(", 0 errors, 2 warnings\n"), "{out}");
    let (_, code) = stdout(kb().arg("--root").arg(&root).args(["lint", "--strict"]));
    assert_eq!(code, 1, "--strict fails on warnings");
    // linting only the note still knows the whole KB
    let (out, _) = stdout(
        kb().arg("--root")
            .arg(&root)
            .args(["lint", "note/links.md"]),
    );
    assert!(
        out.contains("2 warnings") && !out.contains("5xx-by-authorizer]]"),
        "{out}"
    );
}

#[test]
fn runbook_lint_rules_reported_and_exit_1() {
    let (_d, root) = valid_kb();
    fs::write(
        root.join("runbook-bad.md"),
        r#"---
kb_format: 1
type: runbook
title: Bad Runbook
steps:
  - id: step-1
    title: Bad step
    entry: nonexistent/missing-entry
---
"#,
    )
    .unwrap();
    let (out, code) = stdout(kb().arg("--root").arg(&root).arg("lint"));
    assert_eq!(code, 1);
    assert!(out.contains("KB082-step-missing-entry"), "{out}");
    assert!(
        out.contains("referenced entry `nonexistent/missing-entry` does not exist"),
        "{out}"
    );
}

#[test]
fn runbook_lint_json_output() {
    let (_d, root) = valid_kb();
    fs::write(
        root.join("runbook-bad.md"),
        r#"---
kb_format: 1
type: runbook
title: Bad Runbook
steps:
  - id: step-1
    title: Bad step
    entry: nonexistent/missing-entry
---
"#,
    )
    .unwrap();
    let (out, code) = stdout(kb().arg("--root").arg(&root).args(["lint", "--json"]));
    assert_eq!(code, 1);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["summary"]["failed"], true);
    let finding = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["code"] == "KB082-step-missing-entry")
        .expect("finding for KB082-step-missing-entry");
    assert_eq!(finding["severity"], "error");
    assert_eq!(finding["path"], "runbook-bad.md");
}

#[test]
fn runbook_lint_fix_broken_reference_with_auto_commit() {
    let (_d, root) = valid_kb();
    // Initialize git repo in root
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Tester"])
        .current_dir(&root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "tester@example.com"])
        .current_dir(&root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&root)
        .output()
        .unwrap();

    // Create a runbook with a typo pointing near an existing entry in valid_kb: sumo/5xx-by-authorizer
    let rb_path = root.join("deploy.md");
    fs::write(
        &rb_path,
        r#"---
kb_format: 1
type: runbook
title: Deploy
steps:
  - id: check
    title: Check error rate
    # Notice comment
    entry: sumo/5xx-by-authorzer
---
Notes.
"#,
    )
    .unwrap();

    // Lint should fail with suggestion
    let (out, code) = stdout(kb().arg("--root").arg(&root).arg("lint"));
    assert_eq!(code, 1);
    assert!(out.contains("KB082-step-missing-entry"));
    assert!(out.contains("did you mean `sumo/5xx-by-authorizer`?"));

    // Run kb lint --fix --yes
    let (out_fix, code_fix) = stdout(
        kb().arg("--root")
            .arg(&root)
            .args(["lint", "--fix", "--yes"]),
    );
    assert_eq!(code_fix, 0, "fixed runbook should pass lint: {out_fix}");

    // Check file content updated preserving comments
    let updated_content = fs::read_to_string(&rb_path).unwrap();
    assert!(updated_content.contains("entry: sumo/5xx-by-authorizer"));
    assert!(updated_content.contains("# Notice comment"));

    // Check git commit message
    let git_log = std::process::Command::new("git")
        .args(["log", "-1", "--pretty=%B"])
        .current_dir(&root)
        .output()
        .unwrap();
    let commit_msg = String::from_utf8(git_log.stdout).unwrap();
    assert!(
        commit_msg.contains("fix: update broken runbook references"),
        "commit msg: {commit_msg}"
    );
}
