//! Integration test verifying path-limited auto-commit isolation.
//! Ensures operations (add, edit, mv, rm, repin) create isolated commits without touching
//! other staged or unstaged changes in the repository.

mod support;

use std::fs;
use std::path::Path;
use std::process::Command;
use support::kb_in;

fn init_git_repo(root: &Path) {
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    };

    run(&["init", "-q"]);
    run(&["config", "user.name", "Carlos Ortiz"]);
    run(&["config", "user.email", "carlos@example.com"]);
    // Create initial commit
    fs::write(root.join("init.txt"), "initial repository commit\n").unwrap();
    run(&["add", "init.txt"]);
    run(&["commit", "-m", "initial"]);
}

fn git_porcelain(root: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

fn git_last_commit_files(root: &Path) -> Vec<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    text.lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn git_last_commit_message(root: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn test_auto_commit_isolation_add_mv_rm() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("kb");
    fs::create_dir_all(&root).unwrap();

    init_git_repo(&root);

    // 1. Stage an unrelated file
    fs::write(root.join("staged.txt"), "user staged work\n").unwrap();
    Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["add", "staged.txt"])
        .status()
        .unwrap();

    // 2. Create an unstaged modification
    fs::write(root.join("unstaged.txt"), "untracked user notes\n").unwrap();

    // 3. Create entry via kb add --yes
    let entry_body = "echo hello\n";
    let add_out = kb_in(&root)
        .arg("add")
        .arg("--yes")
        .arg("--title")
        .arg("test auto commit")
        .arg("--type")
        .arg("cmd")
        .arg("--path")
        .arg("cmd/test-commit.md")
        .write_stdin(entry_body)
        .output()
        .unwrap();
    assert!(add_out.status.success(), "kb add failed: {:?}", add_out);

    // 4. Verify commit contains only cmd/test-commit.md
    let files = git_last_commit_files(&root);
    assert_eq!(files, vec!["cmd/test-commit.md"]);
    let msg = git_last_commit_message(&root);
    assert_eq!(msg, "kb: add cmd/test-commit");

    // 5. Verify staged and unstaged state is strictly preserved
    let status = git_porcelain(&root);
    assert!(
        status.contains("A  staged.txt"),
        "staged.txt should remain staged: {status}"
    );
    assert!(
        status.contains("?? unstaged.txt"),
        "unstaged.txt should remain untracked: {status}"
    );

    // 6. Rename entry via kb mv
    let mv_out = kb_in(&root)
        .arg("mv")
        .arg("cmd/test-commit")
        .arg("cmd/renamed-commit")
        .output()
        .unwrap();
    assert!(mv_out.status.success(), "kb mv failed: {:?}", mv_out);

    // Verify commit contains only rename
    let mv_files = git_last_commit_files(&root);
    assert!(mv_files.contains(&"cmd/test-commit.md".to_string()));
    assert!(mv_files.contains(&"cmd/renamed-commit.md".to_string()));
    assert_eq!(mv_files.len(), 2);
    let mv_msg = git_last_commit_message(&root);
    assert_eq!(mv_msg, "kb: mv cmd/test-commit -> cmd/renamed-commit");

    // Verify staged and unstaged files still preserved
    let status2 = git_porcelain(&root);
    assert!(
        status2.contains("A  staged.txt"),
        "staged.txt must remain staged: {status2}"
    );
    assert!(
        status2.contains("?? unstaged.txt"),
        "unstaged.txt must remain untracked: {status2}"
    );

    // 7. Remove entry via kb rm
    let rm_out = kb_in(&root)
        .arg("rm")
        .arg("-y")
        .arg("cmd/renamed-commit")
        .output()
        .unwrap();
    assert!(rm_out.status.success(), "kb rm failed: {:?}", rm_out);

    let rm_files = git_last_commit_files(&root);
    assert_eq!(rm_files, vec!["cmd/renamed-commit.md"]);
    let rm_msg = git_last_commit_message(&root);
    assert_eq!(rm_msg, "kb: rm cmd/renamed-commit");

    let status3 = git_porcelain(&root);
    assert!(
        status3.contains("A  staged.txt"),
        "staged.txt must remain staged: {status3}"
    );
    assert!(
        status3.contains("?? unstaged.txt"),
        "unstaged.txt must remain untracked: {status3}"
    );
}

#[test]
fn test_auto_commit_missing_identity() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();

    // Init git repo WITHOUT user.name or user.email
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    };
    run(&["init", "-q"]);

    let entry_body = "echo missing identity\n";
    let add_out = kb_in(&root)
        .arg("add")
        .arg("-y")
        .arg("--title")
        .arg("No Identity Entry")
        .arg("--type")
        .arg("cmd")
        .arg("--path")
        .arg("cmd/no-identity.md")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .write_stdin(entry_body)
        .output()
        .unwrap();

    assert!(
        add_out.status.success(),
        "kb add should succeed even without identity: {:?}",
        add_out
    );
    let stderr = String::from_utf8_lossy(&add_out.stderr);
    assert!(
        stderr.contains("git commit skipped: author identity unknown"),
        "stderr should warn about missing identity: {stderr}"
    );

    // Verify the file was written
    assert!(root.join("cmd/no-identity.md").is_file());
}

#[test]
fn test_auto_commit_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    init_git_repo(&root);

    let entry_body = "echo disabled\n";
    let add_out = kb_in(&root)
        .arg("add")
        .arg("-y")
        .arg("--title")
        .arg("Disabled Commit Entry")
        .arg("--type")
        .arg("cmd")
        .arg("--path")
        .arg("cmd/disabled.md")
        .env("KB_GIT_AUTO_COMMIT", "false")
        .write_stdin(entry_body)
        .output()
        .unwrap();

    assert!(
        add_out.status.success(),
        "kb add should succeed: {:?}",
        add_out
    );
    assert!(root.join("cmd/disabled.md").is_file());

    // git log HEAD should still be the initial commit
    let msg = git_last_commit_message(&root);
    assert_eq!(msg, "initial");
}

#[test]
fn test_auto_commit_non_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let entry_body = "echo non git\n";
    let add_out = kb_in(&root)
        .arg("add")
        .arg("-y")
        .arg("--title")
        .arg("Non Git Entry")
        .arg("--type")
        .arg("cmd")
        .arg("--path")
        .arg("cmd/non-git.md")
        .write_stdin(entry_body)
        .output()
        .unwrap();

    assert!(
        add_out.status.success(),
        "kb add should succeed in non-git directory: {:?}",
        add_out
    );
    assert!(root.join("cmd/non-git.md").is_file());
}
