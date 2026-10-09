//! Performance budgets (SC-002, constitution VII). Ignored by default; run in release mode:
//! `cargo test --release -p kb --test perf -- --ignored --nocapture`.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use assert_cmd::Command;

const ENTRIES: usize = 10_000;

fn entry(i: usize) -> String {
    let notes = "These notes explain when to use the snippet and what to look for. ".repeat(6);
    match i % 7 {
        0 => format!(
            "---\nkb_format: 1\ntype: sql\ntitle: Query {i}\ntags: [db, perf]\ndialect: postgres\nvars:\n  limit: {{ kind: number, default: 50 }}\n---\n\n{notes}\n\n```sql\nselect id, status, created_at\nfrom orders\nwhere status = 'failed'\norder by created_at desc\nlimit {{{{limit}}}};\n```\n"
        ),
        1 => format!(
            "---\nkb_format: 1\ntype: sumo\ntitle: Search {i}\ntags: [logs]\nrange: \"-{{{{minutes}}}}m\"\nvars:\n  env: {{ kind: enum, options: [prod, stage], default: prod }}\n  minutes: {{ kind: number, default: 15 }}\n---\n\n```\n_sourceCategory={{{{env}}}}/app | where status >= 500 | count by host\n```\n\n{notes}\n"
        ),
        2 => format!(
            "---\nkb_format: 1\ntype: cw\ntitle: Insights {i}\nlog_groups: [/aws/lambda/app-{i}]\nrange: -1h\n---\n\n```\nfields @timestamp, @message | filter @message like /ERROR/ | limit 20\n```\n\n{notes}\n"
        ),
        3 => format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Command {i}\nshell: bash\nvars:\n  ns: {{ kind: text, default: default }}\n---\n\n```bash\nkubectl -n {{{{ns}}}} get pods -o wide | grep -v Running\n```\n\n{notes}\n"
        ),
        4 => format!(
            "---\nkb_format: 1\ntype: prompt\ntitle: Prompt {i}\n---\n\n````\nSummarize:\n```\nlog\n```\n````\n\n{notes}\n"
        ),
        5 => format!(
            "---\nkb_format: 1\ntype: link\ntitle: Link {i}\nurl: https://example.com/docs/{i}\n---\n\n{notes}\n"
        ),
        _ => format!(
            "---\nkb_format: 1\ntype: code\ntitle: Code {i}\ncode:\n  repo: example-org/service\n  path: src/module_{i}.rs\n  line: {}\n  ref: main\n---\n\n{notes}\n",
            i % 900 + 1
        ),
    }
}

fn generate(root: &Path) {
    for i in 0..ENTRIES {
        let dir = root.join(format!("group-{:02}", i % 50));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("entry-{i:05}.md")), entry(i)).unwrap();
    }
}

fn kb() -> Command {
    let mut cmd = Command::cargo_bin("kb").unwrap();
    cmd.env_remove("KB_ROOT");
    cmd
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn lint_10k_entries_under_2s() {
    let dir = tempfile::tempdir().unwrap();
    generate(dir.path());
    // Warm the file system cache once, as on a machine that has used the KB before.
    kb().arg("--root")
        .arg(dir.path())
        .args(["lint", "--quiet"])
        .output()
        .unwrap();
    let start = Instant::now();
    let out = kb()
        .arg("--root")
        .arg(dir.path())
        .args(["lint", "--quiet"])
        .output()
        .unwrap();
    let took = start.elapsed();
    let summary = String::from_utf8(out.stdout).unwrap();
    println!(
        "kb lint on {ENTRIES} entries: {took:?} ({})",
        summary.trim()
    );
    assert_eq!(
        summary,
        format!("{ENTRIES} entries, 0 errors, 0 warnings\n")
    );
    assert!(took < Duration::from_secs(2), "lint took {took:?}");
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn cold_start_under_100ms() {
    let mut runs: Vec<Duration> = (0..5)
        .map(|_| {
            let start = Instant::now();
            assert!(kb().arg("--version").output().unwrap().status.success());
            start.elapsed()
        })
        .collect();
    runs.sort();
    let median = runs[2];
    println!("kb --version median: {median:?}");
    assert!(
        median < Duration::from_millis(100),
        "median start {median:?}"
    );
}
