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
