//! Performance budgets (SC-002, constitution VII). Ignored by default; run in release mode:
//! `cargo test --release -p kb --test perf -- --ignored --nocapture`.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use assert_cmd::Command;

mod support;

use support::{backdate, generate_kb, generated_path, kb_in};

const ENTRIES: usize = 10_000;

fn generate(root: &Path) {
    generate_kb(root, ENTRIES);
}

fn kb() -> Command {
    support::kb()
}

/// Timing tests must not run side by side (cargo runs tests in parallel threads).
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn lint_10k_entries_under_2s() {
    let _serial = serial();
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
    let _serial = serial();
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

fn median(mut runs: Vec<Duration>) -> Duration {
    runs.sort();
    runs[runs.len() / 2]
}

fn timed_search(root: &Path, args: &[&str]) -> Duration {
    let start = Instant::now();
    let out = kb_in(root).arg("search").args(args).output().unwrap();
    let took = start.elapsed();
    assert!(
        out.status.success(),
        "search {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    took
}

/// A 10,000-entry KB with an index built and file mtimes an hour old.
fn indexed_kb() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    generate(dir.path());
    backdate(dir.path());
    let start = Instant::now();
    let out = kb_in(dir.path()).arg("reindex").output().unwrap();
    assert!(out.status.success());
    println!("kb reindex on {ENTRIES} entries: {:?}", start.elapsed());
    dir
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn reindex_10k_entries_under_5s() {
    let _serial = serial();
    let dir = tempfile::tempdir().unwrap();
    generate(dir.path());
    backdate(dir.path());
    let start = Instant::now();
    let out = kb_in(dir.path()).arg("reindex").output().unwrap();
    let took = start.elapsed();
    assert!(out.status.success());
    println!(
        "reindex: {took:?} ({})",
        String::from_utf8_lossy(&out.stdout).trim()
    );
    assert!(took < Duration::from_secs(5), "reindex took {took:?}");
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn warm_search_under_50ms() {
    let _serial = serial();
    let dir = indexed_kb();
    timed_search(dir.path(), &["failed", "orders"]); // warm the file system cache
    let m = median(
        (0..20)
            .map(|_| timed_search(dir.path(), &["failed", "orders"]))
            .collect(),
    );
    println!("warm search median: {m:?}");
    assert!(m < Duration::from_millis(50), "median {m:?}");
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn search_after_one_edit_under_100ms() {
    let _serial = serial();
    let dir = indexed_kb();
    timed_search(dir.path(), &["failed", "orders"]);
    let runs: Vec<Duration> = (0..10)
        .map(|i| {
            let p = generated_path(dir.path(), 7 * i + 3);
            let text = fs::read_to_string(&p).unwrap();
            fs::write(&p, format!("{text}\nedited {i}\n")).unwrap();
            timed_search(dir.path(), &["failed", "orders"])
        })
        .collect();
    let m = median(runs);
    println!("search after one edit median: {m:?}");
    assert!(m < Duration::from_millis(100), "median {m:?}");
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn typo_search_under_100ms() {
    let _serial = serial();
    let dir = indexed_kb();
    timed_search(dir.path(), &["kubectll"]);
    let m = median(
        (0..10)
            .map(|_| timed_search(dir.path(), &["kubectll"]))
            .collect(),
    );
    println!("typo search median: {m:?}");
    assert!(m < Duration::from_millis(50), "median {m:?}");
}

// ---- the TUI (SC-001, SC-002, SC-005, FR-019) ----

use support::pty::{ENTER, Session, Spec};

fn tui_home(root: &Path) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    // the KB lives where the test generated it; HOME only has to exist
    let _ = root;
    home
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn first_frame_under_100ms() {
    let _serial = serial();
    let dir = indexed_kb();
    let home = tui_home(dir.path());
    let run = || {
        let started = Instant::now();
        let mut s = Session::spawn(&Spec::new(&[], home.path(), dir.path()));
        s.wait_for(" kb");
        let took = started.elapsed();
        s.send(support::pty::CTRL_C);
        s.wait_exit();
        took
    };
    run(); // warm the file system cache
    let m = median((0..10).map(|_| run()).collect());
    println!("first frame median: {m:?}");
    assert!(m < Duration::from_millis(100), "median {m:?}");
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn keystroke_to_updated_list_under_50ms() {
    let _serial = serial();
    let dir = indexed_kb();
    let home = tui_home(dir.path());
    for (label, timer) in [("refresh timer off", false), ("refresh timer on", true)] {
        let mut spec = Spec::new(&[], home.path(), dir.path());
        if !timer {
            spec = spec.env("KB_TUI_NO_REFRESH_TIMER", "1");
        }
        let mut s = Session::spawn(&spec);
        s.wait_for("10000 entries");
        s.wait_for("Enter pick");
        let mut times = Vec::new();
        for c in "kubectl".chars() {
            let before = s.rows()[1].clone();
            let started = Instant::now();
            s.send(c.to_string().as_bytes());
            s.wait_until("the counter changed", |t| {
                t.lines().nth(1).is_some_and(|l| l != before)
            });
            times.push(started.elapsed());
        }
        let m = median(times.clone());
        println!(
            "keystroke to list ({label}): median {m:?}, worst {:?}",
            times.iter().max().unwrap()
        );
        assert!(m < Duration::from_millis(50), "{label}: median {m:?}");
        s.send(support::pty::CTRL_C);
        s.wait_exit();
    }
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn one_frame_with_ten_thousand_results_under_16ms() {
    let _serial = serial();
    use kb_cli::tui::app::{Action, AppState, Options, Page};
    use kb_cli::tui::theme::Theme;
    use kb_core::entry::EntryType;
    use kb_core::search::{SearchResult, Tier};
    let results: Vec<SearchResult> = (0..10_000)
        .map(|i| SearchResult {
            id: format!("group-{:02}/entry-{i:05}", i % 50),
            use_count: (i % 7) as u64,
            updated: None,
            modified: Some(1_790_000_000),
            entry_type: EntryType::ALL[i % 7],
            title: format!("A reasonably long title for entry number {i} with some words"),
            tags: vec!["prod".into(), "logs".into(), "perf".into()],
            path: format!("group-{:02}/entry-{i:05}.md", i % 50),
            score: 1.0,
            tier: Tier::Exact,
            matches: vec![],
            snippet: None,
        })
        .collect();
    let mut state = AppState::new(Options::default(), (200, 60), "", "~/kb".into());
    state.update(
        Action::Refreshed(kb_cli::tui::app::RefreshInfo {
            indexed: 10_000,
            skipped: 0,
            duration: Duration::ZERO,
        }),
        Instant::now(),
    );
    let g = state.search_gen;
    state.update(
        Action::Results(
            g,
            Page {
                results,
                total: 10_000,
                type_counts: [1_429, 1_429, 1_429, 1_429, 1_428, 1_428, 1_428],
                indexed: 10_000,
                skipped: 0,
                notices: vec![],
            },
        ),
        Instant::now(),
    );
    let theme = Theme::dark();
    let area = ratatui::layout::Rect::new(0, 0, 200, 60);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    kb_cli::tui::view::draw(&state, &theme, area, &mut buf);
    let times: Vec<Duration> = (0..50)
        .map(|_| {
            let started = Instant::now();
            kb_cli::tui::view::draw(&state, &theme, area, &mut buf);
            started.elapsed()
        })
        .collect();
    let m = median(times.clone());
    println!(
        "draw with 10,000 results: median {m:?}, worst {:?}",
        times.iter().max().unwrap()
    );
    assert!(m < Duration::from_millis(16), "median {m:?}");
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn finding_and_picking_a_named_entry_under_5s() {
    let _serial = serial();
    let dir = indexed_kb();
    let home = tui_home(dir.path());
    let text = fs::read_to_string(generated_path(dir.path(), 4_321)).unwrap();
    let title = text
        .lines()
        .find_map(|l| l.strip_prefix("title: "))
        .unwrap()
        .to_string();
    let out = home.path().join("out");
    let mut spec = Spec::new(&[], home.path(), dir.path());
    spec.out = Some(out.clone());
    let started = Instant::now();
    let mut s = Session::spawn(&spec);
    s.wait_for("10000 entries");
    s.type_text(&title);
    s.wait_until("the entry", |t| {
        t.lines()
            .nth(4)
            .is_some_and(|l| l.contains(&title[..title.len().min(30)]))
    });
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);
    let took = started.elapsed();
    println!("start to pick of {title:?}: {took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
    assert!(fs::read_to_string(&out).unwrap().len() > 10);
}

// ---- M4: kb copy, kb config list, kb doctor ----

#[test]
#[ignore = "performance budget; run in release mode"]
fn copy_by_id_under_150ms() {
    // SC-002: `kb copy <id>` end to end, with a local clipboard program (a fake one that just
    // stores its input, so the clipboard itself is not what is timed)
    let _serial = serial();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    generate(&root);
    backdate(&root);
    assert!(
        kb_in(&root)
            .arg("reindex")
            .output()
            .unwrap()
            .status
            .success()
    );
    let (_tool, bin, copied, env) = support::fake_clipboard(dir.path());
    let id = "group-34/entry-01234";
    let run = || {
        let mut cmd = kb_in(&root);
        cmd.args(["copy", id]).env("PATH", support::path_with(&bin));
        for (k, v) in &env {
            cmd.env(k, v);
        }
        let start = Instant::now();
        let out = cmd.output().unwrap();
        let took = start.elapsed();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        took
    };
    run(); // warm the file cache, as after any earlier use
    let runs: Vec<Duration> = (0..7).map(|_| run()).collect();
    let m = median(runs.clone());
    println!("kb copy <id> on {ENTRIES} entries: median {m:?} (runs {runs:?})");
    assert!(copied.exists());
    assert!(m < Duration::from_millis(150), "median {m:?}");
}

#[test]
#[ignore = "performance budget; run in release mode"]
fn config_list_under_100ms_and_doctor_measured() {
    // constitution VII: CLI cold start of 100 ms. `kb doctor` opens the index and probes tools, so
    // it has no budget; its time is printed so a regression is visible.
    let _serial = serial();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    generate(&root);
    backdate(&root);
    assert!(
        kb_in(&root)
            .arg("reindex")
            .output()
            .unwrap()
            .status
            .success()
    );
    let time = |args: &[&str]| {
        let start = Instant::now();
        let out = kb_in(&root).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        start.elapsed()
    };
    time(&["config", "list"]);
    let list = median((0..7).map(|_| time(&["config", "list"])).collect());
    let doctor = median((0..5).map(|_| time(&["doctor"])).collect());
    println!("kb config list: median {list:?}; kb doctor: median {doctor:?} (no budget)");
    assert!(
        list < Duration::from_millis(100),
        "config list median {list:?}"
    );
}
