//! `kb init` end to end (User Story 2).

use std::fs;
use std::path::Path;

use assert_cmd::Command;

const EXAMPLES: [&str; 7] = [
    "sql/example-active-connections.md",
    "sumo/example-5xx-by-authorizer.md",
    "cw/example-lambda-errors.md",
    "cmd/example-tail-pod-logs.md",
    "prompt/example-summarize-incident.md",
    "link/example-runbook.md",
    "code/example-request-handler.md",
];

fn kb(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("kb").unwrap();
    cmd.env_remove("KB_ROOT")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"));
    cmd
}

fn run(cmd: &mut Command) -> (String, String, i32) {
    let out = cmd.output().unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
        out.status.code().unwrap(),
    )
}

#[test]
fn new_kb_with_examples_that_lint_clean() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    let (out, _, code) = run(kb(dir.path()).arg("init").arg(&root));
    assert_eq!(code, 0);
    assert!(
        out.starts_with(&format!(
            "Initialized KB at {}\n",
            root.canonicalize().unwrap().display()
        )),
        "{out}"
    );
    assert!(
        out.contains("  created  .kb/\n  created  .kb/.gitignore\n"),
        "{out}"
    );
    assert_eq!(
        fs::read_to_string(root.join(".kb/.gitignore")).unwrap(),
        "*\n"
    );
    assert!(!root.join(".gitignore").exists());
    for p in EXAMPLES {
        assert!(root.join(p).is_file(), "{p}");
        assert!(out.contains(&format!("  created  {p}\n")), "{out}");
    }
    let (out, _, code) = run(kb(dir.path())
        .arg("--root")
        .arg(&root)
        .args(["lint", "--strict"]));
    assert_eq!(
        (out.as_str(), code),
        ("8 entries, 0 errors, 0 warnings\n", 0)
    );
}

#[test]
fn existing_kb_is_completed_without_touching_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    fs::create_dir_all(root.join("sql")).unwrap();
    let entry = "---\nkb_format: 1\ntype: sql\ntitle: Mine\n---\n\n```\nselect 1;\n```\n";
    fs::write(root.join("sql/mine.md"), entry).unwrap();
    fs::write(root.join(".gitignore"), "target\n").unwrap();
    let (out, _, code) = run(kb(dir.path()).arg("init").arg(&root));
    assert_eq!(code, 0);
    assert!(
        out.contains("  exists   .\n  created  .kb/\n  created  .kb/.gitignore\n"),
        "{out}"
    );
    assert_eq!(fs::read_to_string(root.join("sql/mine.md")).unwrap(), entry);
    assert_eq!(
        fs::read_to_string(root.join(".gitignore")).unwrap(),
        "target\n"
    );
    assert!(
        !root.join("sumo").exists(),
        "no examples in a KB that has entries"
    );
    let (out, _, code) = run(kb(dir.path()).arg("init").arg(&root));
    assert_eq!(code, 0);
    assert!(
        out.starts_with(&format!(
            "KB at {} is up to date\n",
            root.canonicalize().unwrap().display()
        )),
        "{out}"
    );
}

#[test]
fn no_examples_flag() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    let (_, _, code) = run(kb(dir.path()).arg("init").arg(&root).arg("--no-examples"));
    assert_eq!(code, 0);
    let mut entries: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    entries.sort();
    assert_eq!(entries, vec![".kb"]);
}

#[test]
fn json_report() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    let (out, _, code) = run(kb(dir.path()).arg("init").arg(&root).arg("--json"));
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["created", "examples", "existing", "hint", "root", "version"]
    );
    assert_eq!(v["version"], 1);
    assert_eq!(v["examples"], "created");
    assert_eq!(v["created"][0], ".");
    assert_eq!(v["created"].as_array().unwrap().len(), 11);
    let (out, _, _) = run(kb(dir.path()).arg("init").arg(&root).arg("--json"));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["created"].as_array().unwrap().len(), 0);
    assert_eq!(v["examples"], "skipped-has-entries");
}

#[test]
fn hint_when_not_the_default_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("elsewhere");
    let (out, _, _) = run(kb(dir.path()).arg("init").arg(&root));
    assert!(out.contains("KB_ROOT"), "{out}");
    let (out, _, _) = run(kb(dir.path()).arg("init").arg(&root).arg("--json"));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["hint"].as_str().unwrap().contains("KB_ROOT"));
    // The default root: no hint.
    let (out, _, _) = run(kb(dir.path()).arg("init").arg("--json"));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["hint"].is_null(), "{v}");
    assert!(dir.path().join("kb/.kb/.gitignore").is_file());
}

#[test]
fn path_that_is_a_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("f");
    fs::write(&file, "x").unwrap();
    let (_, err, code) = run(kb(dir.path()).arg("init").arg(&file));
    assert_eq!(code, 2);
    assert!(
        err.starts_with("kb: ") && err.contains("not a directory"),
        "{err}"
    );
}
