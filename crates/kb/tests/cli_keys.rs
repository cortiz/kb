//! `kb keys`, `kb keys check`, `kb keys defaults` and the key check in `kb doctor`.

mod support;

use std::fs;

use serde_json::Value;
use support::{kb, search_kb};

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

struct Home(tempfile::TempDir);

impl Home {
    fn with(config: &str) -> Home {
        let h = Home(tempfile::tempdir().unwrap());
        let file = h.0.path().join(".config/kb/config.toml");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, config).unwrap();
        h
    }

    fn kb(&self, args: &[&str]) -> std::process::Output {
        let (_d, root) = search_kb();
        let out = kb()
            .env("HOME", self.0.path())
            .env("XDG_CONFIG_HOME", self.0.path().join(".config"))
            .env("KB_ROOT", &root)
            .args(args)
            .output()
            .unwrap();
        drop(_d);
        out
    }
}

const SCENARIO: &str = "[tui]\nkeymap = \"vim\"\n\n[tui.keys.normal]\nrun = [\"x\", \"!\"]\n\n[tui.keys.global]\nhistory = []\n";

#[test]
fn a_clean_config_checks_out_with_exit_0() {
    let h = Home::with(SCENARIO);
    let out = h.kb(&["keys", "check"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stdout));
    assert_eq!(
        text(&out.stdout),
        "Key configuration valid (preset: vim, 2 overrides active, 0 warnings).\n"
    );
    let json: Value = serde_json::from_slice(&h.kb(&["keys", "check", "--json"]).stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["problems_count"], 0);
}

#[test]
fn every_kind_of_bad_override_is_one_problem_with_its_line_and_exit_1() {
    let h = Home::with(
        "[tui]\nkeymap = \"vim\"\n\n[tui.keys.normal]\nrunn = [\"x\"]\nopen = [\"ctrl-\"]\nquit = [\"q\"]\n\
         [tui.keys.search]\nrun = [\"x\"]\nhelp = []\n[tui.keys.nowhere]\nrun = [\"x\"]\n[tui.keys.global]\ndown = [\"ctrl-c\"]\n",
    );
    let out = h.kb(&["keys", "check"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("config.toml:5") && stdout.contains("unknown command"),
        "{stdout}"
    );
    assert!(
        stdout.contains("config.toml:6") && stdout.contains("unknown key"),
        "{stdout}"
    );
    assert!(
        stdout.contains("cannot unbind reserved key ctrl-c"),
        "{stdout}"
    );
    assert!(stdout.contains("printable key"), "{stdout}");
    assert!(stdout.contains("without a help key"), "{stdout}");
    assert!(stdout.contains("unknown context"), "{stdout}");
    assert!(
        stdout.contains("reserved and cannot be rebound"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Validation failed: 7 problems found."),
        "{stdout}"
    );
    let json: Value = serde_json::from_slice(&h.kb(&["keys", "check", "--json"]).stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["problems_count"], 7);
    assert!(json["warnings"][0]["line"].is_number());
}

#[test]
fn a_key_moved_off_another_command_is_reported_so_the_change_is_visible() {
    let h = Home::with("[tui.keys.search]\naction-menu = [\"ctrl-p\"]\n");
    let out = h.kb(&["keys", "check"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stdout).contains("ctrl-p now runs `action-menu`"),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn keys_lists_the_effective_bindings_and_where_they_come_from() {
    let h = Home::with(SCENARIO);
    let json: Value = serde_json::from_slice(&h.kb(&["keys", "--json"]).stdout).unwrap();
    assert_eq!(json["keymap"], "vim");
    let normal = json["contexts"]["normal"].as_array().unwrap();
    let run = normal.iter().find(|r| r["command"] == "run").unwrap();
    assert_eq!(run["origin"], "config");
    assert_eq!(run["keys"], serde_json::json!(["x", "!"]));
    assert!(run["source"]["line"].is_number());
    let open = normal.iter().find(|r| r["command"] == "open").unwrap();
    assert_eq!(open["origin"], "preset");
    assert_eq!(open["keys"][0], "o");
    let history = normal.iter().find(|r| r["command"] == "history").unwrap();
    assert_eq!(history["keys"], serde_json::json!([]));
    // the text form and the context filter
    let out = h.kb(&["keys", "--context", "normal"]);
    let t = text(&out.stdout);
    assert!(t.starts_with("Context: normal (keymap: vim)"), "{t}");
    assert!(t.contains("[config:") && t.contains("[preset]"), "{t}");
    assert!(!t.contains("Context: search"));
    assert_eq!(
        h.kb(&["keys", "--context", "nowhere"]).status.code(),
        Some(2)
    );
}

#[test]
fn the_keymap_flag_beats_the_environment_and_the_file() {
    let h = Home::with("[tui]\nkeymap = \"emacs\"\n");
    let out = h.kb(&["--keymap", "vim", "keys", "--json"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["keymap"],
        "vim"
    );
    let json: Value = serde_json::from_slice(&h.kb(&["keys", "--json"]).stdout).unwrap();
    assert_eq!(json["keymap"], "emacs");
    assert!(
        json["contexts"].get("normal").is_none(),
        "emacs has no normal mode"
    );
    assert_eq!(h.kb(&["--keymap", "nano", "keys"]).status.code(), Some(2));
}

#[test]
fn defaults_prints_a_block_that_is_valid_config() {
    let h = Home::with("");
    let out = h.kb(&["--keymap", "vim", "keys", "defaults"]);
    assert_eq!(out.status.code(), Some(0));
    let t = text(&out.stdout);
    assert!(
        t.contains("keymap = \"vim\"") && t.contains("# [tui.keys.normal]"),
        "{t}"
    );
    assert!(
        t.contains("# first = [\"g g\", \"home\", \"ctrl-home\"]"),
        "{t}"
    );
    let emacs = text(&h.kb(&["keys", "defaults"]).stdout);
    assert!(!emacs.contains("tui.keys.normal"));
}

#[test]
fn doctor_includes_the_key_check() {
    let ok = Home::with(SCENARIO);
    let json: Value = serde_json::from_slice(&ok.kb(&["doctor", "--json"]).stdout).unwrap();
    let keys = json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "keys")
        .unwrap();
    assert_eq!(keys["status"], "pass");
    let bad = Home::with("[tui.keys.normal]\nrunn = [\"x\"]\n[tui]\nkeymap = \"vim\"\n");
    let json: Value = serde_json::from_slice(&bad.kb(&["doctor", "--json"]).stdout).unwrap();
    let keys = json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "keys")
        .unwrap();
    assert_eq!(keys["status"], "warn");
    assert!(
        keys["details"][0]
            .as_str()
            .unwrap()
            .contains("unknown command")
    );
}
