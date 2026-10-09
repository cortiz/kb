//! `kb config`: create, inspect and edit the config file, and what the other commands do about it.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use kb_core::config::{render_template, set_value};
use serde_json::Value;
use support::{fixtures, kb, search_kb};

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

/// A home directory and the file `kb config` reads and writes there (`~/.config/kb/config.toml`
/// on both platforms).
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        Home {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn file(&self) -> PathBuf {
        self.path().join(".config/kb/config.toml")
    }

    fn write(&self, content: &str) {
        fs::create_dir_all(self.file().parent().unwrap()).unwrap();
        fs::write(self.file(), content).unwrap();
    }

    /// `kb <args>` with this home and no `--root`.
    fn kb(&self, args: &[&str]) -> std::process::Output {
        let mut cmd = kb();
        cmd.env("HOME", self.path())
            .env("XDG_CONFIG_HOME", self.path().join(".config"))
            .args(args);
        cmd.output().unwrap()
    }

    fn kb_env(&self, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
        let mut cmd = kb();
        cmd.env("HOME", self.path())
            .env("XDG_CONFIG_HOME", self.path().join(".config"))
            .args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }
}

fn good() -> String {
    fs::read_to_string(fixtures().join("config/good.toml")).unwrap()
}

// ---- init and path ----

#[test]
fn init_writes_the_commented_template_and_prints_where() {
    let h = Home::new();
    let out = h.kb(&["config", "init"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout).trim_end(), h.file().display().to_string());
    assert_eq!(fs::read_to_string(h.file()).unwrap(), render_template());
}

#[test]
fn init_never_overwrites() {
    let h = Home::new();
    h.write("theme = \"dark\"\n");
    let out = h.kb(&["config", "init"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("config file already exists"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(fs::read_to_string(h.file()).unwrap(), "theme = \"dark\"\n");
}

#[test]
fn init_json() {
    let h = Home::new();
    let out = h.kb(&["config", "init", "--json"]);
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["path"], h.file().display().to_string());
    assert_eq!(doc["created"], true);
}

#[test]
fn path_shows_where_the_file_is_written_and_which_one_is_in_use() {
    let h = Home::new();
    let out = h.kb(&["config", "path"]);
    assert_eq!(text(&out.stdout).trim_end(), h.file().display().to_string());
    let out = h.kb(&["config", "path", "--read"]);
    let listing = text(&out.stdout);
    assert!(
        listing.contains(&h.file().display().to_string()),
        "{listing}"
    );
    assert!(!listing.contains("(in use)"), "{listing}");
    h.write("");
    let listing = text(&h.kb(&["config", "path", "--read"]).stdout);
    let first = listing.lines().next().unwrap();
    assert!(
        first.contains("config.toml") && first.contains("(in use)"),
        "{listing}"
    );
    let doc: Value = serde_json::from_slice(&h.kb(&["config", "path", "--json"]).stdout).unwrap();
    assert_eq!(doc["path"], h.file().display().to_string());
    assert_eq!(doc["candidates"][0]["in_use"], true);
}

// ---- get and list ----

#[test]
fn get_prints_the_effective_value_from_whichever_source_wins() {
    let h = Home::new();
    assert_eq!(text(&h.kb(&["config", "get", "theme"]).stdout), "auto\n");
    h.write("theme = \"dark\"\n");
    assert_eq!(text(&h.kb(&["config", "get", "theme"]).stdout), "dark\n");
    let out = h.kb_env(&["config", "get", "theme"], &[("KB_THEME", "light")]);
    assert_eq!(text(&out.stdout), "light\n");
    let out = h.kb(&["config", "get", "colour"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("unknown setting \"colour\""),
        "{}",
        text(&out.stderr)
    );
    let doc: Value =
        serde_json::from_slice(&h.kb(&["config", "get", "theme", "--json"]).stdout).unwrap();
    assert_eq!(
        (
            doc["key"].as_str(),
            doc["value"].as_str(),
            doc["source"].as_str()
        ),
        (Some("theme"), Some("dark"), Some("file"))
    );
}

#[test]
fn list_shows_every_setting_with_its_value_and_where_it_came_from() {
    let h = Home::new();
    h.write("theme = \"dark\"\n[tui]\nmouse = true\n");
    let out = h.kb_env(&["config", "list"], &[("KB_EDITOR", "nvim")]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let listing = text(&out.stdout);
    for want in [
        "theme = dark  (file)",
        "tui.mouse = true  (file)",
        "tui.enter = copy  (default)",
        "editor = nvim  (env KB_EDITOR)",
        "clipboard.osc52_max_bytes = 74000  (default)",
    ] {
        assert!(
            listing
                .lines()
                .any(|l| l.trim_start() == want || l.contains(want)),
            "{want}\n{listing}"
        );
    }
    assert_eq!(listing.lines().count(), 12, "{listing}");
    // a flag is a source too
    let (_d, root) = search_kb();
    let out = h.kb(&["--root", root.to_str().unwrap(), "config", "list"]);
    assert!(
        text(&out.stdout).contains("(flag --root)"),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn list_json_has_a_row_per_setting() {
    let h = Home::new();
    h.write("theme = \"dark\"\n");
    let doc: Value = serde_json::from_slice(&h.kb(&["config", "list", "--json"]).stdout).unwrap();
    let rows = doc.as_array().unwrap();
    assert_eq!(rows.len(), 12);
    let theme = rows.iter().find(|r| r["key"] == "theme").unwrap();
    assert_eq!(theme["value"], "dark");
    assert_eq!(theme["source"], "file");
    assert_eq!(theme["file"], h.file().display().to_string());
    assert_eq!(theme["line"], 1);
    let enter = rows.iter().find(|r| r["key"] == "tui.enter").unwrap();
    assert_eq!(enter["source"], "default");
    assert!(enter["file"].is_null() && enter["line"].is_null());
}

// ---- set ----

#[test]
fn set_changes_one_value_and_keeps_everything_else() {
    let h = Home::new();
    h.write(&good());
    let out = h.kb(&["config", "set", "theme", "dark"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        fs::read_to_string(h.file()).unwrap(),
        set_value(&good(), "theme", "dark").unwrap()
    );
    let after = fs::read_to_string(h.file()).unwrap();
    assert!(
        after.contains("# trailing comment on root") && after.contains("# end of file comment")
    );
    assert_eq!(after.lines().count(), good().lines().count());
    assert_eq!(text(&h.kb(&["config", "get", "theme"]).stdout), "dark\n");
}

#[test]
fn set_creates_the_file_with_only_that_key() {
    let h = Home::new();
    let out = h.kb(&["config", "set", "theme", "dark"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(fs::read_to_string(h.file()).unwrap(), "theme = \"dark\"\n");
    h.kb(&["config", "set", "clipboard.method", "osc52"]);
    assert_eq!(
        fs::read_to_string(h.file()).unwrap(),
        "theme = \"dark\"\n\n[clipboard]\nmethod = \"osc52\"\n"
    );
}

#[test]
fn set_refuses_bad_values_and_unknown_keys_without_touching_the_file() {
    let h = Home::new();
    h.write(&good());
    for args in [
        vec!["config", "set", "theme", "blue"],
        vec!["config", "set", "tui.enter", "maybe"],
        vec!["config", "set", "clipboard.osc52_max_bytes", "0"],
        vec!["config", "set", "colour", "red"],
    ] {
        let out = h.kb(&args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: {}",
            text(&out.stderr)
        );
        assert_eq!(fs::read_to_string(h.file()).unwrap(), good(), "{args:?}");
    }
    let out = h.kb(&["config", "set", "theme", "blue"]);
    assert!(
        text(&out.stderr).contains("expected one of auto, dark, light"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn set_in_a_file_that_is_not_toml_refuses_and_leaves_it() {
    let h = Home::new();
    h.write("theme = [\n");
    let out = h.kb(&["config", "set", "theme", "dark"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read_to_string(h.file()).unwrap(), "theme = [\n");
}

#[test]
fn set_says_when_an_environment_variable_overrides_the_value() {
    let h = Home::new();
    let out = h.kb_env(
        &["config", "set", "theme", "dark"],
        &[("KB_THEME", "light")],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("KB_THEME is set and overrides this value"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(fs::read_to_string(h.file()).unwrap(), "theme = \"dark\"\n");
}

#[test]
fn an_unwritable_config_directory_fails_clearly_and_other_commands_still_work() {
    use std::os::unix::fs::PermissionsExt;
    let h = Home::new();
    let dir = h.path().join(".config");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let out = h.kb(&["config", "set", "theme", "dark"]);
    let writable = fs::write(dir.join("probe"), "x").is_ok(); // running as root ignores modes
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        return;
    }
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("cannot write"),
        "{}",
        text(&out.stderr)
    );
    let (_d, root) = search_kb();
    let out = h.kb(&[
        "--root",
        root.to_str().unwrap(),
        "search",
        "latency",
        "--ids",
    ]);
    assert_eq!(out.status.code(), Some(0));
}

// ---- no file until asked; problems warn once ----

#[test]
fn no_command_creates_a_config_file_by_itself() {
    let h = Home::new();
    let (_d, root) = search_kb();
    let r = root.to_str().unwrap();
    for args in [
        vec!["--root", r, "search", "latency", "--ids"],
        vec!["--root", r, "show", "sumo/5xx-by-authorizer", "--raw"],
        vec!["config", "list"],
        vec!["config", "get", "theme"],
        vec!["config", "path"],
        vec!["--root", r, "lint", "--quiet"],
    ] {
        h.kb(&args);
    }
    assert!(
        !h.path().join(".config").exists(),
        "a config directory appeared"
    );
    assert!(!h.path().join("Library").exists());
}

#[test]
fn a_config_with_a_bad_value_and_an_unknown_key_still_works_with_one_warning_each() {
    let h = Home::new();
    h.write(&fs::read_to_string(fixtures().join("config/bad.toml")).unwrap());
    let (_d, root) = search_kb();
    let out = h.kb(&[
        "--root",
        root.to_str().unwrap(),
        "search",
        "latency",
        "--ids",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains("cw/latency-prod"),
        "{}",
        text(&out.stdout)
    );
    let err = text(&out.stderr);
    let warnings: Vec<&str> = err.lines().filter(|l| l.contains("config.toml:")).collect();
    assert_eq!(warnings.len(), 2, "{err}");
    assert!(
        warnings[0].contains("config.toml:3:") && warnings[0].contains("clipbaord.method"),
        "{err}"
    );
    assert!(
        warnings[1].contains("config.toml:6:") && warnings[1].contains("\"xclipp\""),
        "{err}"
    );
    // --json output stays parseable: the warnings are on stderr only
    let out = h.kb(&[
        "--root",
        root.to_str().unwrap(),
        "search",
        "latency",
        "--json",
    ]);
    assert!(serde_json::from_slice::<Value>(&out.stdout).is_ok());
}

#[test]
fn a_legacy_yaml_config_still_works_with_a_deprecation_warning() {
    let h = Home::new();
    let dir = h.path().join(".config/kb");
    fs::create_dir_all(&dir).unwrap();
    let (_d, root) = search_kb();
    fs::write(
        dir.join("config.yml"),
        format!("root: {}\ntheme: dark\n", root.display()),
    )
    .unwrap();
    // no --root: the legacy file supplies it
    let out = h.kb(&["search", "latency", "--ids"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("cw/latency-prod"));
    assert!(
        text(&out.stderr).contains("config.yml: deprecated"),
        "{}",
        text(&out.stderr)
    );
    assert!(
        !h.file().exists(),
        "the legacy file is never converted by itself"
    );
}

#[test]
fn set_never_hides_the_settings_of_the_file_that_was_in_use() {
    // macOS reads ~/.config first and ~/Library/Application Support second; writing a new file
    // under ~/.config would hide the Library file, so it starts as a copy of it
    if !cfg!(target_os = "macos") {
        return;
    }
    let h = Home::new();
    let library = h.path().join("Library/Application Support/kb/config.toml");
    fs::create_dir_all(library.parent().unwrap()).unwrap();
    fs::write(&library, "# mine\ntheme = \"dark\"\n").unwrap();
    assert_eq!(text(&h.kb(&["config", "get", "theme"]).stdout), "dark\n");
    let out = h.kb(&["config", "set", "tui.mouse", "true"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("copied the settings of"),
        "{}",
        text(&out.stderr)
    );
    let now = fs::read_to_string(h.file()).unwrap();
    assert!(
        now.contains("# mine") && now.contains("theme = \"dark\"") && now.contains("mouse = true"),
        "{now}"
    );
    assert_eq!(
        text(&h.kb(&["config", "get", "theme"]).stdout),
        "dark\n",
        "still dark"
    );
    assert_eq!(
        fs::read_to_string(&library).unwrap(),
        "# mine\ntheme = \"dark\"\n",
        "untouched"
    );
}

#[test]
fn set_moves_the_legacy_yaml_settings_along_so_nothing_is_lost() {
    let h = Home::new();
    let (_d, root) = search_kb();
    let dir = h.path().join(".config/kb");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("config.yml"),
        format!("root: {}\ntheme: dark\n", root.display()),
    )
    .unwrap();
    let out = h.kb(&["config", "set", "tui.mouse", "true"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("config.yml"),
        "{}",
        text(&out.stderr)
    );
    let now = fs::read_to_string(h.file()).unwrap();
    assert!(
        now.contains("theme = \"dark\"") && now.contains("mouse = true"),
        "{now}"
    );
    assert!(
        now.contains(&format!("root = \"{}\"", root.display())),
        "{now}"
    );
    // the root still comes from the config, so searches keep working with no --root
    let out = h.kb(&["search", "latency", "--ids"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("cw/latency-prod"));
    assert!(
        !text(&out.stderr).contains("deprecated"),
        "the new file hides the old one"
    );
}
