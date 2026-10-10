//! Concurrent searches while files change (SC-005, FR-004).

mod support;

use std::collections::HashSet;
use std::fs;
use std::process::Command as Process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use support::{backdate, generate_kb, generated_path, kb_in, search_kb};

fn kb_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(assert_cmd::Command::cargo_bin("kb").unwrap().get_program())
}

/// Editors and kb save through a temporary file and a rename, so a reader never sees half a file.
/// (A plain `fs::write` can be read mid-write, which kb correctly reports as an entry with errors.)
fn write_atomically(path: &std::path::Path, text: &str) {
    let tmp = path.with_extension("tmp-write");
    if fs::write(&tmp, text).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

fn spawn_search(root: &std::path::Path, args: &[&str]) -> std::process::Child {
    Process::new(kb_bin())
        .env_remove("KB_ROOT")
        .arg("--root")
        .arg(root)
        .arg("search")
        .args(args)
        .arg("--ids")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

fn assert_index_healthy(root: &std::path::Path) {
    let conn = Connection::open(root.join(".kb/index.db")).unwrap();
    let ok: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ok, "ok");
    let (files_ok, entries, fts, orphans): (i64, i64, i64, i64) = conn
        .query_row(
            "SELECT (SELECT count(*) FROM files WHERE status='ok'), \
                    (SELECT count(*) FROM entries), (SELECT count(*) FROM fts), \
                    (SELECT count(*) FROM entries WHERE id NOT IN (SELECT id FROM files))",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!((files_ok, fts, orphans), (entries, entries, 0));
}

#[test]
fn fifty_searches_while_files_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    generate_kb(&root, 500);
    backdate(&root);
    assert!(
        kb_in(&root)
            .arg("reindex")
            .output()
            .unwrap()
            .status
            .success()
    );

    let stop = Arc::new(AtomicBool::new(false));
    let deleted: Arc<Mutex<Vec<(String, Instant)>>> = Arc::default();
    let writer = {
        let (root, stop, deleted) = (root.clone(), stop.clone(), deleted.clone());
        thread::spawn(move || {
            let mut n = 0usize;
            while !stop.load(Ordering::Relaxed) {
                let i = 10 + n % 400;
                let p = generated_path(&root, i);
                match n % 4 {
                    0 => {
                        if let Ok(t) = fs::read_to_string(&p) {
                            write_atomically(&p, &format!("{t}\nedit {n} zebrafish\n"));
                        }
                    }
                    1 => {
                        write_atomically(
                            &root.join(format!("group-00/new-{n}.md")),
                            &support::entry_text(&format!("New {n}"), "t", "x", "zebrafish"),
                        );
                    }
                    2 => {
                        let to = root.join(format!("group-01/moved-{n}.md"));
                        let _ = fs::rename(&p, to);
                    }
                    _ => {
                        if fs::remove_file(&p).is_ok() {
                            let id = format!("group-{:02}/entry-{i:05}", i % 50);
                            deleted.lock().unwrap().push((id, Instant::now()));
                        }
                    }
                }
                n += 1;
                thread::sleep(Duration::from_millis(3));
            }
        })
    };

    let mut results = Vec::new();
    for _batch in 0..5 {
        let started = Instant::now();
        let children: Vec<_> = (0..10)
            .map(|k| {
                spawn_search(
                    &root,
                    &[["entry", "query", "zebrafish", "failed", "group"][k % 5]],
                )
            })
            .collect();
        for c in children {
            let out = c.wait_with_output().unwrap();
            results.push((started, out));
        }
    }
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();

    assert_eq!(results.len(), 50);
    let deleted = deleted.lock().unwrap().clone();
    for (started, out) in &results {
        let code = out.status.code();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(matches!(code, Some(0 | 1)), "exit {code:?}: {err}");
        for bad in ["malformed", "locked", "corrupt", "error"] {
            assert!(!err.contains(bad), "{bad}: {err}");
        }
        let got: HashSet<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect();
        for (id, at) in &deleted {
            if at < started {
                assert!(
                    !got.contains(id),
                    "{id} was deleted before this search started"
                );
            }
        }
    }
    assert!(
        kb_in(&root)
            .args(["search", "entry"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_index_healthy(&root);
}

#[test]
fn simultaneous_first_builds_all_succeed() {
    let (_d, root) = search_kb();
    let children: Vec<_> = (0..8).map(|_| spawn_search(&root, &["auth"])).collect();
    for c in children {
        let out = c.wait_with_output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let got = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(got.contains("sumo/5xx-by-authorizer"), "{got}");
    }
    assert_index_healthy(&root);
}

#[test]
fn a_held_write_lock_delays_a_search_by_about_a_second_at_most() {
    let (_d, root) = search_kb();
    assert!(
        kb_in(&root)
            .args(["search", "auth"])
            .output()
            .unwrap()
            .status
            .success()
    );
    support::backdate(&root);
    // make one file differ so the search wants to write
    let p = root.join("prompt/cache.md");
    let text = fs::read_to_string(&p).unwrap();
    fs::write(&p, text.replace("caching", "storing")).unwrap();

    let conn = Connection::open(root.join(".kb/index.db")).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = Instant::now();
    let out = kb_in(&root)
        .args(["search", "latency", "--ids"])
        .output()
        .unwrap();
    let took = started.elapsed();
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    println!("search with the write lock held took {took:?}");
    // one busy timeout (1 s) and no more; two stacked waits would be 2 s
    assert!(took < Duration::from_millis(1600), "{took:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("cw/latency-prod"));
    // once the lock is gone the change is picked up
    let out = kb_in(&root)
        .args(["search", "storing", "--ids"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "prompt/cache");
}

// ---- authoring (spec 007) ----

#[test]
fn racing_adds_for_the_same_title_never_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    fs::create_dir_all(&root).unwrap();
    let handles: Vec<_> = (0..6)
        .map(|n| {
            let root = root.clone();
            thread::spawn(move || {
                let out = kb_in(&root)
                    .args(["add", "--yes", "-t", "Same title", "--type", "sql"])
                    .write_stdin(format!("select {n} from t;"))
                    .output()
                    .unwrap();
                (
                    out.status.code(),
                    String::from_utf8_lossy(&out.stdout).trim().to_string(),
                    String::from_utf8_lossy(&out.stderr).into_owned(),
                )
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let mut ids: Vec<String> = Vec::new();
    for (code, id, err) in &results {
        // a lost race for the same free name is refused (exit 1), never an overwrite
        if *code == Some(0) {
            ids.push(id.clone());
        } else {
            assert_eq!(*code, Some(1), "{err}");
            assert!(err.contains("already exists"), "{err}");
        }
    }
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "two adds got the same ID: {ids:?}");
    assert!(!ids.is_empty());
    for id in &ids {
        let text = fs::read_to_string(root.join(format!("{id}.md"))).unwrap();
        assert!(text.contains("select "), "{id}");
    }
    let leftovers: Vec<_> = fs::read_dir(root.join("sql"))
        .unwrap()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().ends_with(".md"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_rename_while_searching_keeps_the_index_consistent() {
    let (_d, root) = search_kb();
    backdate(&root);
    let stop = Arc::new(AtomicBool::new(false));
    let searcher = {
        let (root, stop) = (root.clone(), Arc::clone(&stop));
        thread::spawn(move || {
            let mut runs = 0;
            while !stop.load(Ordering::Relaxed) {
                let out = kb_in(&root).args(["search", "--ids"]).output().unwrap();
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                runs += 1;
            }
            runs
        })
    };
    for i in 0..10 {
        let (from, to) = if i % 2 == 0 {
            ("link/payments-runbook", "link/moved")
        } else {
            ("link/moved", "link/payments-runbook")
        };
        let out = kb_in(&root).args(["mv", from, to]).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    stop.store(true, Ordering::Relaxed);
    assert!(searcher.join().unwrap() > 0);
    assert_index_healthy(&root);
}
