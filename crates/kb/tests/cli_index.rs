//! `kb reindex` and `kb index status` (User Story 5), and index recovery from the command line.

mod support;

use std::fs;

use rusqlite::Connection;
use serde_json::Value;
use support::{kb_in, search_kb};

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

#[test]
fn status_without_an_index_creates_nothing() {
    let (_d, root) = search_kb();
    let out = kb_in(&root).args(["index", "status"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).starts_with("Index: none ("),
        "{}",
        stdout(&out)
    );
    assert!(!root.join(".kb").exists());
    let out = kb_in(&root)
        .args(["index", "status", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["state"], "missing");
    assert_eq!(doc["entries_indexed"], 0);
    assert_eq!(doc["last_full_build"], Value::Null);
}

#[test]
fn status_reports_the_index() {
    let (_d, root) = search_kb();
    fs::write(root.join("bad.md"), "nothing").unwrap();
    kb_in(&root).args(["search", "auth"]).output().unwrap();
    let out = kb_in(&root).args(["index", "status"]).output().unwrap();
    let text = stdout(&out);
    for prefix in [
        "Index:      ",
        "Version:    1",
        "Entries:    32 indexed, 1 skipped due to errors",
        "Size:       ",
        "Last build: ",
    ] {
        assert!(
            text.lines().any(|l| l.starts_with(prefix)),
            "{prefix:?} in\n{text}"
        );
    }
    assert!(text.contains(" UTC"));
    let out = kb_in(&root)
        .args(["index", "status", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut keys: Vec<_> = doc.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "entries_indexed",
            "entries_skipped",
            "index_version",
            "last_full_build",
            "path",
            "size_bytes",
            "state"
        ]
    );
    assert_eq!(
        (
            doc["state"].as_str(),
            doc["entries_indexed"].as_u64(),
            doc["entries_skipped"].as_u64()
        ),
        (Some("ok"), Some(32), Some(1))
    );
    assert!(doc["size_bytes"].as_u64().unwrap() > 0);
    assert!(doc["last_full_build"].as_i64().unwrap() > 1_700_000_000);
}

#[test]
fn status_does_not_sync() {
    let (_d, root) = search_kb();
    kb_in(&root).args(["search", "auth"]).output().unwrap();
    fs::remove_file(root.join("link/oncall-handbook.md")).unwrap();
    let out = kb_in(&root)
        .args(["index", "status", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["entries_indexed"], 32);
}

#[test]
fn status_names_an_unusable_index() {
    let (_d, root) = search_kb();
    fs::create_dir_all(root.join(".kb")).unwrap();
    fs::write(root.join(".kb/index.db"), "garbage").unwrap();
    let out = kb_in(&root).args(["index", "status"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).starts_with("Index: unusable ("),
        "{}",
        stdout(&out)
    );
    let out = kb_in(&root)
        .args(["index", "status", "--json"])
        .output()
        .unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["state"], "unusable");
    assert!(doc["reason"].is_string());
}

#[test]
fn reindex_rebuilds_and_the_next_search_is_quiet() {
    let (_d, root) = search_kb();
    let out = kb_in(&root).arg("reindex").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).starts_with("Reindexed 32 entries (0 skipped due to errors) in "),
        "{}",
        stdout(&out)
    );
    let out = kb_in(&root).args(["search", "auth"]).output().unwrap();
    assert_eq!(stderr(&out), "");
    let out = kb_in(&root).args(["reindex", "--json"]).output().unwrap();
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["entries_indexed"], 32);
    assert!(doc["duration_ms"].is_u64());
}

#[test]
fn reindex_keeps_usage_and_prunes_the_vanished() {
    let (_d, root) = search_kb();
    let state = serde_json::json!({
        "state_version": 1,
        "usage": {
            "sql/orders-by-status": {"count": 4, "last_used": 1_790_000_000, "hash": "0000000000000001"},
            "gone/entry": {"count": 1, "last_used": 1_790_000_000, "hash": "0000000000000002"}
        },
        "vars": {"region": "eu"}
    });
    fs::create_dir_all(root.join(".kb")).unwrap();
    fs::write(root.join(".kb/state.json"), state.to_string()).unwrap();
    assert!(
        kb_in(&root)
            .arg("reindex")
            .output()
            .unwrap()
            .status
            .success()
    );
    let after: Value =
        serde_json::from_slice(&fs::read(root.join(".kb/state.json")).unwrap()).unwrap();
    assert_eq!(after["usage"]["sql/orders-by-status"]["count"], 4);
    assert!(after["usage"].get("gone/entry").is_none());
    assert_eq!(after["vars"]["region"], "eu");
}

#[cfg(unix)]
#[test]
fn reindex_into_an_unwritable_kb_dir_is_exit_2() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, root) = search_kb();
    fs::create_dir_all(root.join(".kb")).unwrap();
    fs::set_permissions(root.join(".kb"), fs::Permissions::from_mode(0o555)).unwrap();
    let writable = fs::write(root.join(".kb/probe"), "x").is_ok();
    let out = kb_in(&root).arg("reindex").output().unwrap();
    fs::set_permissions(root.join(".kb"), fs::Permissions::from_mode(0o755)).unwrap();
    if !writable {
        assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
        assert!(stderr(&out).starts_with("kb: "));
    }
}

// ---- recovery (US2 scenarios 3 and 4) ----

#[test]
fn missing_corrupt_and_old_indexes_are_rebuilt_without_asking() {
    let (_d, root) = search_kb();
    let search = || {
        kb_in(&root)
            .args(["search", "5xx", "auth", "--ids"])
            .output()
            .unwrap()
    };
    let first = search();
    assert!(stderr(&first).contains("index rebuilt"));
    assert_eq!(
        stdout(&first).lines().next(),
        Some("sumo/5xx-by-authorizer")
    );

    fs::remove_file(root.join(".kb/index.db")).unwrap();
    let out = search();
    assert_eq!(
        stderr(&out).matches("index rebuilt").count(),
        1,
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), stdout(&first));

    fs::write(root.join(".kb/index.db"), "garbage garbage garbage").unwrap();
    let out = search();
    assert_eq!(
        stderr(&out).matches("index rebuilt").count(),
        1,
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), stdout(&first));

    {
        let c = Connection::open(root.join(".kb/index.db")).unwrap();
        c.pragma_update(None, "user_version", 99).unwrap();
    }
    let out = search();
    assert_eq!(
        stderr(&out).matches("index rebuilt").count(),
        1,
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), stdout(&first));
    assert_eq!(stderr(&search()), "");
}

#[test]
fn a_bad_state_file_is_backed_up_and_warned_about_once() {
    let (_d, root) = search_kb();
    fs::create_dir_all(root.join(".kb")).unwrap();
    fs::write(root.join(".kb/state.json"), "{").unwrap();
    let out = kb_in(&root).args(["search", "auth"]).output().unwrap();
    assert_eq!(
        stderr(&out).matches("state.json was invalid").count(),
        1,
        "{}",
        stderr(&out)
    );
    assert_eq!(
        fs::read_to_string(root.join(".kb/state.json.bak")).unwrap(),
        "{"
    );
}

#[cfg(unix)]
#[test]
fn a_read_only_kb_dir_still_searches() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, root) = search_kb();
    fs::create_dir_all(root.join(".kb")).unwrap();
    fs::set_permissions(root.join(".kb"), fs::Permissions::from_mode(0o555)).unwrap();
    let writable = fs::write(root.join(".kb/probe"), "x").is_ok();
    let out = kb_in(&root)
        .args(["search", "5xx", "auth", "--ids"])
        .output()
        .unwrap();
    let files: Vec<_> = fs::read_dir(root.join(".kb"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    fs::set_permissions(root.join(".kb"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(stdout(&out).lines().next(), Some("sumo/5xx-by-authorizer"));
    if !writable {
        assert!(stderr(&out).contains("not writable"), "{}", stderr(&out));
        assert!(files.is_empty(), "{files:?}");
    }
}
