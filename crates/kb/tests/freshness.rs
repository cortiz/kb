//! The index is never stale (User Story 2), from the command line.

mod support;

use std::fs;

use support::{kb_in, search_kb};

fn ids(root: &std::path::Path, q: &[&str]) -> Vec<String> {
    let out = kb_in(root)
        .arg("search")
        .args(q)
        .arg("--ids")
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn edit_delete_create_and_rename_are_seen_by_the_next_search() {
    let (_d, root) = search_kb();
    assert_eq!(ids(&root, &["5xx", "auth"])[0], "sumo/5xx-by-authorizer");

    let p = root.join("sumo/5xx-by-authorizer.md");
    let text = fs::read_to_string(&p).unwrap();
    fs::write(
        &p,
        text.replace("5xx by authorizer", "gateway timeouts by authorizer"),
    )
    .unwrap();
    assert_eq!(
        ids(&root, &["gateway", "timeouts"])[0],
        "sumo/5xx-by-authorizer"
    );

    fs::rename(&p, root.join("sumo/renamed.md")).unwrap();
    assert_eq!(ids(&root, &["gateway", "timeouts"])[0], "sumo/renamed");
    assert!(!ids(&root, &["authorizer"]).contains(&"sumo/5xx-by-authorizer".to_string()));

    fs::remove_file(root.join("sumo/renamed.md")).unwrap();
    assert!(!ids(&root, &["timeouts"]).contains(&"sumo/renamed".to_string()));

    fs::write(
        root.join("sumo/fresh.md"),
        support::entry_text("Brand new", "t", "x", ""),
    )
    .unwrap();
    assert_eq!(ids(&root, &["brand"]), ["sumo/fresh"]);
}

#[test]
fn deleting_the_kb_directory_changes_nothing_but_frecency() {
    let (_d, root) = search_kb();
    let before = ids(&root, &["latency"]);
    fs::remove_dir_all(root.join(".kb")).unwrap();
    assert_eq!(ids(&root, &["latency"]), before);
    assert_eq!(
        fs::read_to_string(root.join(".kb/.gitignore")).unwrap(),
        "*\n"
    );
}

#[test]
fn an_edit_in_the_same_second_with_the_same_size_is_still_seen() {
    let (_d, root) = search_kb();
    let p = root.join("prompt/cache.md");
    // copying may keep the old timestamp; the racy window only applies to recent edits
    fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(std::time::SystemTime::now())
        .unwrap();
    assert_eq!(ids(&root, &["caching", "layers"]), ["prompt/cache"]);
    let text = fs::read_to_string(&p).unwrap();
    let mtime = fs::metadata(&p).unwrap().modified().unwrap();
    fs::write(&p, text.replace("caching", "storing")).unwrap();
    assert_eq!(
        text.len(),
        fs::read_to_string(&p).unwrap().len(),
        "same-size edit"
    );
    fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    assert_eq!(ids(&root, &["storing", "layers"]), ["prompt/cache"]);
}
