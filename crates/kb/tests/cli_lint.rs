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
        out.contains("sumo/bad.md:3:7: error[KB001-unknown-type]: unknown type \"sumologic\"; expected one of sql, sumo, cw, cmd, prompt, link, code\n  fix: use \"sumo\"\n"),
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
