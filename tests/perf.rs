//! Performance benchmark tests (FR-033, SC-006).

use std::fs;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use assert_cmd::Command as AssertCommand;
use kb_cli::platform::SystemGitRunner;
use kb_core::code::{CodeService, LocalClone, RemoteHost};
use kb_core::entry::parse_entry;

fn create_fast_import_repo(dir: &Path, commit_count: usize) -> (String, String) {
    let init_out = Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(dir)
        .output()
        .expect("git init");
    assert!(init_out.status.success());

    let mut child = Command::new("git")
        .args(["fast-import", "--quiet"])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .spawn()
        .expect("git fast-import spawn");

    let mut stdin = child.stdin.take().expect("stdin");
    let initial_content = b"commit refs/heads/main\ncommitter Perf Test <perf@kb.internal> 1000000000 +0000\ndata <<EOM\ninitial\nEOM\nM 644 inline src/Auth.java\ndata <<EOD\npublic class Auth {\n}\nEOD\n\n";
    stdin.write_all(initial_content).expect("initial write");

    for i in 1..=commit_count {
        let commit_buf = format!(
            "commit refs/heads/main\ncommitter Perf Test <perf@kb.internal> {} +0000\ndata <<EOM\nc{}\nEOM\n\n",
            1000000000 + i,
            i
        );
        stdin
            .write_all(commit_buf.as_bytes())
            .expect("write commit");
    }
    drop(stdin);
    let status = child.wait().expect("wait fast-import");
    assert!(status.success());

    let pinned_commit = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", &format!("refs/heads/main~{commit_count}")])
            .current_dir(dir)
            .output()
            .expect("rev-parse pinned")
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    let tip_commit = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "refs/heads/main"])
            .current_dir(dir)
            .output()
            .expect("rev-parse tip")
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    (pinned_commit, tip_commit)
}

#[test]
fn perf_single_entry_drift_check_under_500ms_on_100k_commits() {
    let temp = tempfile::tempdir().unwrap();
    let repo_dir = temp.path().join("repo-100k");
    fs::create_dir_all(&repo_dir).unwrap();

    let (pinned_ref, _) = create_fast_import_repo(&repo_dir, 100_000);

    let doc = format!(
        r#"---
kb_format: 1
type: code
title: Auth Check
code:
  repo: example/auth
  path: src/Auth.java
  line: 1
  ref: {pinned_ref}
  branch: main
---
"#
    );
    let parsed = parse_entry("auth.md", "auth.md", doc.into_bytes());
    let entry = parsed.entry.expect("parsed entry");

    let clone = LocalClone {
        repo_identifier: "example/auth".to_string(),
        local_path: repo_dir,
        remote_url: "https://github.com/example/auth.git".to_string(),
        host: RemoteHost::GitHub,
        tracking_branch: "main".to_string(),
        has_local_changes: false,
    };

    let git_runner = SystemGitRunner;

    let start = Instant::now();
    let report = CodeService::check_drift(&entry, &clone, &git_runner).expect("check_drift");
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() <= 500,
        "drift check on 100k-commit repo took {:?}, budget is <= 500ms",
        elapsed
    );
    assert!(matches!(report.status, kb_core::code::DriftStatus::Current));
}

#[test]
fn perf_code_check_200_entries_10_repos_under_10_seconds() {
    let temp = tempfile::tempdir().unwrap();
    let kb_root = temp.path().join("kb");
    let home = temp.path().join("home");
    let repos_dir = temp.path().join("repos");
    fs::create_dir_all(&kb_root).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&repos_dir).unwrap();

    let config_dir = home.join(".config/kb");
    fs::create_dir_all(&config_dir).unwrap();

    let mut config_repos = String::from("[code.repos]\n");

    // Create 10 repositories (100 commits each)
    let mut repo_pins = Vec::new();
    for r in 0..10 {
        let repo_name = format!("repo-{r}");
        let repo_path = repos_dir.join(&repo_name);
        fs::create_dir_all(&repo_path).unwrap();
        let (pinned_ref, _) = create_fast_import_repo(&repo_path, 100);
        let repo_id = format!("org/{repo_name}");
        config_repos.push_str(&format!("\"{repo_id}\" = \"{}\"\n", repo_path.display()));
        repo_pins.push((repo_id, pinned_ref));
    }
    fs::write(config_dir.join("config.toml"), config_repos).unwrap();

    // Create 200 entries (20 entries per repo)
    for i in 0..200 {
        let (ref repo_id, ref pinned_ref) = repo_pins[i % 10];
        let doc = format!(
            r#"---
kb_format: 1
type: code
title: Entry {i}
code:
  repo: {repo_id}
  path: src/Auth.java
  line: 1
  ref: {pinned_ref}
  branch: main
---
"#
        );
        fs::write(kb_root.join(format!("entry-{i:03}.md")), doc).unwrap();
    }

    let mut cmd = AssertCommand::cargo_bin("kb").unwrap();
    cmd.env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("KB_ROOT", &kb_root)
        .args(["code", "check", "--json"]);

    let start = Instant::now();
    let output = cmd.output().expect("kb code check");
    let elapsed = start.elapsed();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed.as_secs() <= 10,
        "checking 200 entries took {:?}, budget is <= 10s",
        elapsed
    );
}

#[test]
fn perf_dynamic_completion_under_50ms() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let kb_root = temp.path().join("kb");
    fs::create_dir_all(&kb_root).unwrap();

    // Create 1,000 entries
    for i in 0..1000 {
        let doc = format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Command {i}\ntags: [perf, test]\nshell: bash\n---\n\n```\necho {i}\n```\n"
        );
        fs::write(kb_root.join(format!("cmd-{i:04}.md")), doc).unwrap();
    }

    // Warm index
    let mut reindex_cmd = AssertCommand::cargo_bin("kb").unwrap();
    reindex_cmd
        .env("HOME", &home)
        .env("KB_ROOT", &kb_root)
        .arg("reindex");
    let reindex_out = reindex_cmd.output().expect("reindex");
    assert!(reindex_out.status.success());

    // Benchmark dynamic completion
    let mut comp_cmd = AssertCommand::cargo_bin("kb").unwrap();
    comp_cmd
        .env("HOME", &home)
        .env("KB_ROOT", &kb_root)
        .args(["__complete", "entries", "cmd-00"]);

    let start = Instant::now();
    let output = comp_cmd.output().expect("kb __complete entries");
    let elapsed = start.elapsed();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "must complete entries");
    assert!(
        elapsed.as_millis() <= 50,
        "dynamic completion took {:?}, budget is <= 50ms",
        elapsed
    );
}

#[test]
fn perf_picker_startup_under_80ms() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let kb_root = temp.path().join("kb");
    fs::create_dir_all(&kb_root).unwrap();

    // Warm index with entries
    for i in 0..100 {
        let doc = format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Entry {i}\nshell: bash\n---\n\n```\necho {i}\n```\n"
        );
        fs::write(kb_root.join(format!("entry-{i:03}.md")), doc).unwrap();
    }

    let mut reindex_cmd = AssertCommand::cargo_bin("kb").unwrap();
    reindex_cmd
        .env("HOME", &home)
        .env("KB_ROOT", &kb_root)
        .arg("reindex");
    assert!(reindex_cmd.output().unwrap().status.success());

    // Benchmark completions type listing startup
    let mut comp_cmd = AssertCommand::cargo_bin("kb").unwrap();
    comp_cmd
        .env("HOME", &home)
        .env("KB_ROOT", &kb_root)
        .args(["__complete", "types"]);

    let start = Instant::now();
    let output = comp_cmd.output().expect("kb __complete types");
    let elapsed = start.elapsed();

    assert!(output.status.success());
    assert!(
        elapsed.as_millis() <= 80,
        "startup took {:?}, budget is <= 80ms",
        elapsed
    );
}

#[test]
fn perf_mcp_startup_under_150ms() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let kb_root = temp.path().join("kb");
    fs::create_dir_all(&kb_root).unwrap();

    let bin_path = AssertCommand::cargo_bin("kb")
        .unwrap()
        .get_program()
        .to_os_string();

    // Warm binary in OS cache
    let _ = Command::new(&bin_path).arg("--version").output();

    let t0 = Instant::now();
    let mut child = Command::new(&bin_path)
        .env("HOME", &home)
        .env("KB_ROOT", &kb_root)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kb mcp");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = std::io::BufReader::new(stdout);

    writeln!(
        stdin,
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}}"
    )
    .unwrap();
    stdin.flush().unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let elapsed = t0.elapsed();

    drop(stdin);
    let _ = child.wait();

    assert!(line.contains("\"result\""), "valid initialize response");
    assert!(
        elapsed.as_millis() <= 150,
        "mcp startup took {:?}, budget is <= 150ms",
        elapsed
    );
}

#[test]
fn perf_mcp_search_under_50ms() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let kb_root = temp.path().join("kb");
    fs::create_dir_all(&kb_root).unwrap();

    // Populate KB with 100 entries and warm index
    for i in 0..100 {
        let doc = format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Command {i} service handler\ntags: [auth, prod]\nshell: bash\n---\n\n```\necho {i}\n```\n"
        );
        fs::write(kb_root.join(format!("cmd-{i:03}.md")), doc).unwrap();
    }

    let mut reindex_cmd = AssertCommand::cargo_bin("kb").unwrap();
    reindex_cmd
        .env("HOME", &home)
        .env("KB_ROOT", &kb_root)
        .arg("reindex");
    assert!(reindex_cmd.output().unwrap().status.success());

    let bin_path = AssertCommand::cargo_bin("kb")
        .unwrap()
        .get_program()
        .to_os_string();
    let mut child = Command::new(&bin_path)
        .env("HOME", &home)
        .env("KB_ROOT", &kb_root)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kb mcp");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = std::io::BufReader::new(stdout);

    // Initialize first
    writeln!(
        stdin,
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}}"
    )
    .unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();

    // Measure tool search latency
    let search_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "search",
            "arguments": {
                "query": "service handler"
            }
        }
    });

    let start = Instant::now();
    writeln!(stdin, "{}", serde_json::to_string(&search_req).unwrap()).unwrap();
    stdin.flush().unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    let elapsed = start.elapsed();

    drop(stdin);
    let _ = child.wait();

    assert!(line.contains("\"result\""), "valid search response");
    assert!(
        elapsed.as_millis() <= 50,
        "mcp tool search latency took {:?}, budget is <= 50ms",
        elapsed
    );
}
