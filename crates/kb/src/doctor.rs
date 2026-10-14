//! `kb doctor`: what works on this machine, and what to do about what does not. The checks are
//! plain values (`Check`), produced from a `Platform` and the loaded config so they can be tested
//! with `FakePlatform`; printing is separate.

use std::path::Path;

use kb_core::config::Loaded;
use kb_core::index::IndexState;
use kb_core::search::Kb;
use serde_json::{Value, json};

use crate::platform::clipboard::{self, ClipboardSettings, Method};
use crate::platform::env::{TRUE_COLOR_ADVERTISED, TRUE_COLOR_ADVICE};
use crate::platform::{Display, EditorSource, Environment, Platform, nowhere_to_open};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub id: &'static str,
    pub status: Status,
    pub summary: String,
    /// What to do about it.
    pub hint: Option<String>,
    /// Extra facts shown under the check.
    pub details: Vec<String>,
}

impl Check {
    fn new(id: &'static str, status: Status, summary: impl Into<String>) -> Check {
        Check {
            id,
            status,
            summary: summary.into(),
            hint: None,
            details: Vec::new(),
        }
    }

    fn hint(mut self, hint: impl Into<String>) -> Check {
        self.hint = Some(hint.into());
        self
    }

    fn detail(mut self, detail: impl Into<String>) -> Check {
        self.details.push(detail.into());
        self
    }
}

pub struct Report {
    pub version: String,
    pub env: Environment,
    pub checks: Vec<Check>,
    pub config_path: Option<String>,
    pub data_dir: Option<String>,
    pub cache_dir: Option<String>,
}

impl Report {
    /// No check failed (warnings are fine): the exit code is 0 exactly then.
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|c| c.status != Status::Fail)
    }

    pub fn check(&self, id: &str) -> Option<&Check> {
        self.checks.iter().find(|c| c.id == id)
    }

    pub fn to_json(&self) -> Value {
        let e = &self.env;
        json!({
            "version": self.version,
            "ok": self.ok(),
            "environment": {
                "os": e.os.name().to_lowercase(),
                "arch": e.arch,
                "display": e.display.name().to_lowercase(),
                "ssh": e.ssh,
                "tmux": e.tmux,
                "screen": e.screen,
                "true_color": e.true_color,
            },
            "paths": {
                "config": self.config_path,
                "data": self.data_dir,
                "cache": self.cache_dir,
            },
            "checks": self.checks.iter().map(|c| json!({
                "id": c.id,
                "status": c.status.name(),
                "summary": c.summary,
                "hint": c.hint,
                "details": c.details,
            })).collect::<Vec<_>>(),
        })
    }

    /// The checklist as text. `fancy` uses ✓ ⚠ ✗; otherwise `ok`, `warn` and `FAIL`.
    pub fn render(&self, fancy: bool) -> String {
        let mut out = format!(
            "kb {} on {} ({})\n",
            self.version,
            self.env.os.name(),
            self.env.arch
        );
        let width = self.checks.iter().map(|c| c.id.len()).max().unwrap_or(0);
        for c in &self.checks {
            let mark = match (c.status, fancy) {
                (Status::Pass, true) => "✓",
                (Status::Warn, true) => "⚠",
                (Status::Fail, true) => "✗",
                (Status::Pass, false) => "ok  ",
                (Status::Warn, false) => "warn",
                (Status::Fail, false) => "FAIL",
            };
            out.push_str(&format!(" {mark} {:<width$}  {}\n", c.id, c.summary));
            let pad = " ".repeat(width + 8);
            if let Some(h) = &c.hint {
                out.push_str(&format!("{pad}fix: {h}\n"));
            }
            for d in &c.details {
                out.push_str(&format!("{pad}{d}\n"));
            }
        }
        out
    }
}

fn first_word(command: &str) -> &str {
    command.split_whitespace().next().unwrap_or_default()
}

/// Whether `command`'s program can be run: a path that exists, or a name on `PATH`.
fn runnable(platform: &impl Platform, command: &str) -> bool {
    let program = first_word(command);
    if program.contains('/') {
        Path::new(program).exists()
    } else {
        platform.find_tool(program).is_some()
    }
}

fn check_root(root: &Path) -> Check {
    match std::fs::metadata(root) {
        Ok(m) if m.is_dir() => Check::new("root", Status::Pass, root.display().to_string()),
        Ok(_) => Check::new("root", Status::Fail, format!("{} is not a directory", root.display()))
            .hint("point kb at your KB with --root, KB_ROOT or `kb config set root <dir>`"),
        Err(e) => Check::new(
            "root",
            Status::Fail,
            format!("{} cannot be read ({e})", root.display()),
        )
        .hint("run `kb init`, or point kb at your KB with --root, KB_ROOT or `kb config set root <dir>`"),
    }
}

/// Writability without writing to the KB itself (only `.kb/`, which kb owns, is touched).
fn check_writable(root: &Path) -> Check {
    let state_dir = root.join(".kb");
    let writable = if state_dir.is_dir() {
        let probe = state_dir.join(format!(".doctor-probe-{}", std::process::id()));
        let ok = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    } else {
        std::fs::metadata(root).is_ok_and(|m| !m.permissions().readonly())
    };
    if writable {
        Check::new("root-writable", Status::Pass, "kb can save here")
    } else {
        Check::new("root-writable", Status::Warn, "read-only")
            .hint("entries can be read and searched, but not edited or used (use counts need .kb/)")
    }
}

fn check_index(root: &Path) -> Check {
    let status = Kb::open(root).map(|kb| kb.status());
    match status {
        Ok(s) => match s.state {
            IndexState::Ok => Check::new(
                "index",
                Status::Pass,
                format!(
                    "{} entries indexed, {} skipped due to errors",
                    s.entries_indexed, s.entries_skipped
                ),
            ),
            IndexState::Missing => Check::new("index", Status::Warn, "not built yet")
                .hint("it is built on the first search; `kb reindex` builds it now"),
            IndexState::Unusable(reason) => {
                Check::new("index", Status::Warn, format!("unusable ({reason})"))
                    .hint("it is rebuilt on the next search; `kb reindex` rebuilds it now")
            }
        },
        Err(e) => Check::new("index", Status::Warn, format!("cannot check: {e}"))
            .hint("fix the root first"),
    }
}

fn check_config(platform: &impl Platform, loaded: &Loaded) -> Check {
    let described = match &loaded.file {
        Some(p) => p.display().to_string(),
        None => match platform.config_write_path() {
            Some(p) => format!(
                "no config file (defaults); `kb config init` creates {}",
                p.display()
            ),
            None => "no config file (defaults)".to_string(),
        },
    };
    let unreadable = loaded.warnings.iter().any(|w| {
        w.message.starts_with("could not read") || w.message.starts_with("could not parse")
    });
    let status = if unreadable {
        Status::Fail
    } else if loaded.warnings.is_empty() && loaded.ignored.is_empty() && !loaded.legacy {
        Status::Pass
    } else {
        Status::Warn
    };
    let mut check = Check::new("config", status, described);
    for w in &loaded.warnings {
        check = check.detail(w.to_string());
    }
    for other in &loaded.ignored {
        check = check.detail(format!(
            "{} also exists but is ignored: the first config.toml wins",
            other.display()
        ));
    }
    if loaded.legacy {
        check = check.hint("run `kb config init`, then move your settings into config.toml");
    } else if !loaded.ignored.is_empty() {
        check = check.hint("delete the file you do not use, or merge it into the one in use");
    } else if status != Status::Pass {
        check = check.hint("fix the lines above (`kb config list` shows what is in effect)");
    }
    check
}

fn check_display(env: &Environment) -> Check {
    let summary = format!("{} on {}", env.display.name(), env.os.name());
    if env.display == Display::None && !env.ssh {
        Check::new("display", Status::Warn, "no display server found")
            .hint("no clipboard tool applies here; kb will use the terminal (OSC 52), which works in iTerm2, kitty, Alacritty, WezTerm and recent GNOME Terminal")
    } else if env.display == Display::None {
        Check::new(
            "display",
            Status::Pass,
            "none (SSH session): copying goes through your terminal",
        )
    } else {
        Check::new("display", Status::Pass, summary)
    }
}

fn check_session(env: &Environment) -> Check {
    let yes = |b: bool| if b { "yes" } else { "no" };
    Check::new(
        "session",
        Status::Pass,
        format!(
            "ssh {}, tmux {}, screen {}",
            yes(env.ssh),
            yes(env.tmux),
            yes(env.screen)
        ),
    )
}

/// Which clipboard method kb will use here, and what is wrong with it if anything.
fn check_clipboard(platform: &impl Platform, env: &Environment, loaded: &Loaded) -> (Check, Check) {
    let s = ClipboardSettings::from_settings(&loaded.settings);
    let chain = clipboard::chain(env, &s, &platform.native_tools(env));
    let mut missing_hints: Vec<String> = Vec::new();
    let mut available: Vec<Method> = Vec::new();
    for m in chain {
        let ok = match &m {
            Method::Native(t) => {
                let found = platform.find_tool(t.name).is_some();
                if !found && !missing_hints.iter().any(|h| h == t.install_hint) {
                    missing_hints.push(t.install_hint.to_string());
                }
                found
            }
            Method::Custom(c) => {
                let found = runnable(platform, c);
                if !found {
                    missing_hints.push(format!(
                        "clipboard.command: `{}` was not found",
                        first_word(c)
                    ));
                }
                found
            }
            Method::Osc52 => platform.has_terminal(),
            Method::Stdout => true,
        };
        if ok {
            available.push(m);
        }
    }
    let chosen = available.first().cloned().unwrap_or(Method::Stdout);
    let limit = format!(
        "OSC 52 limit: {} bytes (clipboard.osc52_max_bytes); larger payloads: kb show --raw <id> | <clipboard tool>",
        s.osc52_max_bytes
    );
    let mut check = match &chosen {
        Method::Native(_) | Method::Custom(_) => {
            Check::new("clipboard", Status::Pass, chosen.label())
        }
        Method::Osc52 => {
            let mut c = Check::new("clipboard", Status::Pass, "terminal (OSC 52)").detail(
                "kb cannot confirm the terminal accepted it, so it says \"sent\", not \"copied\"",
            );
            if env.term_program.as_deref() == Some("Apple_Terminal") {
                c.status = Status::Warn;
                c = c.hint("Terminal.app ignores OSC 52; use iTerm2, kitty, Alacritty or WezTerm, or install a clipboard tool and set clipboard.method = \"native\"");
            } else if env.tmux && platform.tmux_allows_passthrough() == Some(false) {
                c.status = Status::Warn;
                c = c.hint("tmux is not forwarding terminal sequences: add \"set -g allow-passthrough on\" to ~/.tmux.conf and reload it");
            } else if !missing_hints.is_empty() {
                c.status = Status::Warn;
                c = c.hint(missing_hints.join("; "));
            }
            c
        }
        Method::Stdout => {
            let hint = if missing_hints.is_empty() {
                "install a clipboard tool, or use a terminal that supports OSC 52".to_string()
            } else {
                missing_hints.join("; ")
            };
            Check::new(
                "clipboard",
                Status::Warn,
                "none available: the text is printed instead",
            )
            .hint(hint)
        }
    };
    check = check.detail(limit);
    let others: Vec<String> = available
        .iter()
        .skip(1)
        .filter(|m| **m != Method::Stdout)
        .map(Method::label)
        .collect();
    let alternatives = if others.is_empty() {
        Check::new("clipboard-alternatives", Status::Pass, "no other method")
    } else {
        Check::new(
            "clipboard-alternatives",
            Status::Pass,
            format!("also available: {}", others.join(", ")),
        )
    };
    (check, alternatives)
}

fn check_true_color(env: &Environment) -> Check {
    if env.true_color {
        Check::new("true-color", Status::Pass, TRUE_COLOR_ADVERTISED)
    } else {
        Check::new(
            "true-color",
            Status::Warn,
            "not advertised: kb uses 256 colors",
        )
        .hint(TRUE_COLOR_ADVICE)
    }
}

fn check_editor(platform: &impl Platform, loaded: &Loaded) -> Check {
    let editor = platform.resolve_editor(loaded.settings.editor());
    let from = match editor.source {
        EditorSource::Config => "config",
        EditorSource::Visual => "$VISUAL",
        EditorSource::EditorVar => "$EDITOR",
        EditorSource::Fallback => "default",
    };
    if runnable(platform, &editor.command) {
        Check::new(
            "editor",
            Status::Pass,
            format!("{} (from {from})", editor.command),
        )
    } else {
        Check::new(
            "editor",
            Status::Warn,
            format!(
                "{} (from {from}) was not found",
                first_word(&editor.command)
            ),
        )
        .hint("set `editor` in the config (kb config set editor nvim) or $EDITOR")
    }
}

fn check_opener(platform: &impl Platform, env: &Environment, loaded: &Loaded) -> Check {
    let opener = platform.opener(loaded.settings.opener_command());
    if nowhere_to_open(env) {
        Check::new(
            "opener",
            Status::Pass,
            "SSH without a display: URLs are printed instead of opened",
        )
    } else if runnable(platform, &opener.command) {
        Check::new("opener", Status::Pass, opener.command)
    } else {
        Check::new(
            "opener",
            Status::Warn,
            format!("{} was not found", first_word(&opener.command)),
        )
        .hint("install xdg-utils, or set `opener.command`")
    }
}

/// Runs every check.
/// Whether `kb add` can read the clipboard here. Only looks for the programs: the clipboard
/// itself is never read, because doctor output gets pasted into bug reports.
fn check_clipboard_read(platform: &impl Platform, env: &Environment) -> Check {
    if env.ssh {
        return Check::new(
            "clipboard-read",
            Status::Warn,
            "cannot read the clipboard over SSH",
        )
        .hint("over SSH kb add uses stdin or an empty form");
    }
    let tools = clipboard::read_tools_for(env);
    let mut details = Vec::new();
    let mut found = None;
    for tool in &tools {
        let there = platform.find_tool(tool.name).is_some();
        details.push(format!(
            "{}: {}",
            tool.name,
            if there { "found" } else { "not found" }
        ));
        if there && found.is_none() {
            found = Some(tool.name);
        }
    }
    let mut check = match found {
        Some(name) => Check::new("clipboard-read", Status::Pass, format!("reads with {name}")),
        None => {
            let hint = tools.first().map_or(
                "install wl-clipboard (Wayland) or xclip / xsel (X11); no display found",
                |t| t.install_hint,
            );
            Check::new(
                "clipboard-read",
                Status::Warn,
                "cannot read the clipboard: no read tool found",
            )
            .hint(hint)
        }
    };
    check.details = details;
    check
}

fn check_shell(platform: &impl Platform, loaded: &Loaded) -> Check {
    let dialect = crate::commands::shell::detect_current_shell();
    let home = platform.home_dir();
    let widget_key = loaded.settings.widget_key();
    let save_key = loaded.settings.widget_save_key();

    let target_file = home
        .as_ref()
        .map(|h| crate::commands::shell::get_shell_startup_file(dialect, h));
    let installed = target_file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|content| content.contains(kb_core::shell::templates::KB_STARTUP_BLOCK_BEGIN))
        .unwrap_or(false);

    let has_clash = if let Some(f) = &target_file {
        if let Ok(content) = std::fs::read_to_string(f) {
            let outside = if let (Some(start), Some(end)) = (
                content.find(kb_core::shell::templates::KB_STARTUP_BLOCK_BEGIN),
                content.find(kb_core::shell::templates::KB_STARTUP_BLOCK_END),
            ) {
                format!(
                    "{}{}",
                    &content[..start],
                    &content[end + kb_core::shell::templates::KB_STARTUP_BLOCK_END.len()..]
                )
            } else {
                content
            };
            outside.contains(widget_key)
        } else {
            false
        }
    } else {
        false
    };

    let mut check = if has_clash {
        Check::new(
            "shell",
            Status::Warn,
            format!("{} (clash on key '{}')", dialect.as_str(), widget_key),
        )
        .hint(format!("key '{widget_key}' is already bound; use KB_WIDGET_KEY or widget.key to rebind, or KB_WIDGET_FORCE=1"))
    } else if installed {
        Check::new(
            "shell",
            Status::Pass,
            format!(
                "{} (widget: {}, save: {})",
                dialect.as_str(),
                widget_key,
                save_key
            ),
        )
    } else {
        Check::new(
            "shell",
            Status::Pass,
            format!(
                "{} (widget not loaded; run `kb shell install`)",
                dialect.as_str()
            ),
        )
    };

    if let Some(f) = &target_file {
        check = check.detail(format!("startup file: {}", f.display()));
    }
    check = check.detail(format!("widget key: {widget_key}, save key: {save_key}"));
    if has_clash && let Some(f) = &target_file {
        check = check.detail(format!(
            "warning: key '{widget_key}' is bound elsewhere in {}",
            f.display()
        ));
    }

    check
}

fn check_mcp(root: &Path, loaded: &Loaded) -> Check {
    let mut server = crate::commands::mcp::McpServer::with_settings(
        root,
        loaded.settings.clone(),
        false,
        loaded.settings.mcp_prompts(),
    );
    let init_req = crate::commands::mcp::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!(1)),
        method: "initialize".to_string(),
        params: None,
    };
    let init_ok = server
        .handle_request(init_req)
        .is_some_and(|r| r.error.is_none());

    let ping_req = crate::commands::mcp::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!(2)),
        method: "ping".to_string(),
        params: None,
    };
    let ping_ok = server
        .handle_request(ping_req)
        .is_some_and(|r| r.error.is_none());

    let kb_ok = server.ensure_kb().is_ok();

    if init_ok && ping_ok && kb_ok {
        let mut check = Check::new("mcp", Status::Pass, "stdio server responsive");
        check = check.detail("protocol: 2024-11-05");
        if loaded.settings.mcp_prompts() {
            check = check.detail("prompts: enabled");
        } else {
            check = check.detail("prompts: disabled");
        }
        if loaded.settings.mcp_write() {
            check = check.detail("write: enabled");
        } else {
            check = check.detail("write: disabled (read-only)");
        }
        check
    } else {
        Check::new("mcp", Status::Warn, "mcp stdio server failed self-test")
            .hint("check permissions or run `kb mcp` directly to inspect errors")
    }
}

pub fn run(platform: &impl Platform, root: &Path, loaded: &Loaded) -> Report {
    let env = platform.env();
    let (clipboard_check, alternatives) = check_clipboard(platform, &env, loaded);
    let root_check = check_root(root);
    let root_ok = root_check.status != Status::Fail;
    let mut checks = vec![
        Check::new(
            "kb",
            Status::Pass,
            format!(
                "{} on {} ({})",
                env!("CARGO_PKG_VERSION"),
                env.os.name(),
                env.arch
            ),
        ),
        root_check,
    ];
    if root_ok {
        checks.push(check_writable(root));
        checks.push(check_index(root));
    }
    checks.extend([
        check_config(platform, loaded),
        check_display(&env),
        check_session(&env),
        clipboard_check,
        alternatives,
        check_clipboard_read(platform, &env),
        check_true_color(&env),
        check_editor(platform, loaded),
        check_opener(platform, &env, loaded),
        check_shell(platform, loaded),
    ]);
    if root_ok {
        checks.push(check_mcp(root, loaded));
    }
    Report {
        version: env!("CARGO_PKG_VERSION").to_string(),
        config_path: loaded
            .file
            .as_ref()
            .or(platform.config_write_path().as_ref())
            .map(|p| p.display().to_string()),
        data_dir: platform.data_dir().map(|p| p.display().to_string()),
        cache_dir: platform.cache_dir().map(|p| p.display().to_string()),
        env,
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::fake::{FakePlatform, FakeTool, linux_env, mac_env};
    use kb_core::config::{Flags, load};
    use std::path::PathBuf;

    fn loaded_from(files: &[PathBuf], env: &[(&str, &str)]) -> Loaded {
        let env: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        load(
            files,
            &move |k| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()),
            &Flags::default(),
        )
    }

    fn defaults() -> Loaded {
        loaded_from(&[], &[])
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config")
            .join(name)
    }

    /// A temporary root (an empty KB directory).
    fn root() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("kb");
        std::fs::create_dir_all(&r).unwrap();
        (d, r)
    }

    /// A Mac with everything in place.
    fn healthy_mac() -> FakePlatform {
        FakePlatform::new(mac_env(&[("COLORTERM", "truecolor")]))
            .with_tool("pbcopy", FakeTool::Ok)
            .with_tool("pbpaste", FakeTool::Ok)
            .with_tool("vi", FakeTool::Ok)
            .with_tool("open", FakeTool::Ok)
    }

    fn status_of(r: &Report, id: &str) -> Status {
        r.check(id)
            .unwrap_or_else(|| panic!("no check {id}"))
            .status
    }

    #[test]
    fn a_healthy_mac_passes_every_check() {
        let (_d, root) = root();
        Kb::open(&root).unwrap().reindex().unwrap();
        let r = run(&healthy_mac(), &root, &defaults());
        for c in &r.checks {
            assert_eq!(c.status, Status::Pass, "{c:?}");
        }
        assert!(r.ok());
        assert_eq!(r.check("clipboard").unwrap().summary, "pbcopy");
        let ids: Vec<&str> = r.checks.iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            [
                "kb",
                "root",
                "root-writable",
                "index",
                "config",
                "display",
                "session",
                "clipboard",
                "clipboard-alternatives",
                "clipboard-read",
                "true-color",
                "editor",
                "opener",
                "shell",
                "mcp",
            ]
        );
    }

    #[test]
    fn a_missing_root_fails_and_skips_the_checks_that_need_it() {
        let d = tempfile::tempdir().unwrap();
        let r = run(&healthy_mac(), &d.path().join("nope"), &defaults());
        assert_eq!(status_of(&r, "root"), Status::Fail);
        assert!(
            r.check("root")
                .unwrap()
                .hint
                .as_deref()
                .unwrap()
                .contains("kb init")
        );
        assert!(
            r.check("index").is_none()
                && r.check("root-writable").is_none()
                && r.check("mcp").is_none()
        );
        assert!(!r.ok());
        // a file where the root should be
        let f = d.path().join("file");
        std::fs::write(&f, "x").unwrap();
        assert_eq!(
            status_of(&run(&healthy_mac(), &f, &defaults()), "root"),
            Status::Fail
        );
    }

    #[test]
    fn a_read_only_root_is_a_warning() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, root) = root();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();
        let writable = std::fs::write(root.join("probe"), "x").is_ok(); // root ignores modes
        let r = run(&healthy_mac(), &root, &defaults());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !writable {
            assert_eq!(status_of(&r, "root-writable"), Status::Warn);
            assert!(r.ok(), "a warning alone is not a failure");
        }
    }

    #[test]
    fn the_writable_probe_stays_inside_the_state_directory() {
        let (_d, root) = root();
        std::fs::create_dir(root.join(".kb")).unwrap();
        let before: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let r = run(&healthy_mac(), &root, &defaults());
        assert_eq!(status_of(&r, "root-writable"), Status::Pass);
        let after: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(before, after, "nothing was added to the root");
        assert_eq!(
            std::fs::read_dir(root.join(".kb")).unwrap().count(),
            0,
            "and the probe is removed"
        );
    }

    #[test]
    fn the_index_is_missing_then_current() {
        let (_d, root) = root();
        let r = run(&healthy_mac(), &root, &defaults());
        assert_eq!(status_of(&r, "index"), Status::Warn);
        assert!(
            r.check("index")
                .unwrap()
                .hint
                .as_deref()
                .unwrap()
                .contains("kb reindex")
        );
        Kb::open(&root).unwrap().reindex().unwrap();
        assert_eq!(
            status_of(&run(&healthy_mac(), &root, &defaults()), "index"),
            Status::Pass
        );
    }

    #[test]
    fn config_problems_are_warnings_with_the_lines_and_a_fix() {
        let (_d, root) = root();
        let bad = loaded_from(&[fixture("bad.toml")], &[]);
        let r = run(&healthy_mac(), &root, &bad);
        let c = r.check("config").unwrap();
        assert_eq!(c.status, Status::Warn);
        assert_eq!(c.details.len(), 2, "{c:?}");
        assert!(c.details[0].contains("bad.toml:3:") && c.details[1].contains("bad.toml:6:"));
        assert!(c.hint.is_some());
        // a good file passes and is named
        let good = loaded_from(&[fixture("good.toml")], &[]);
        let c = run(&healthy_mac(), &root, &good);
        assert_eq!(status_of(&c, "config"), Status::Pass);
        assert!(c.check("config").unwrap().summary.ends_with("good.toml"));
    }

    #[test]
    fn a_file_that_is_not_toml_fails_the_config_check() {
        let (_d, root) = root();
        let broken = loaded_from(&[fixture("syntax-error.toml")], &[]);
        let r = run(&healthy_mac(), &root, &broken);
        assert_eq!(status_of(&r, "config"), Status::Fail);
        assert!(!r.ok());
    }

    #[test]
    fn a_legacy_yaml_file_and_an_ignored_second_toml_are_warnings() {
        let (_d, root) = root();
        let legacy = loaded_from(&[fixture("legacy/config.yml")], &[]);
        let c = run(&healthy_mac(), &root, &legacy);
        assert_eq!(status_of(&c, "config"), Status::Warn);
        assert!(
            c.check("config")
                .unwrap()
                .hint
                .as_deref()
                .unwrap()
                .contains("kb config init")
        );
        let two = loaded_from(&[fixture("good.toml"), fixture("bad.toml")], &[]);
        let c = run(&healthy_mac(), &root, &two)
            .check("config")
            .cloned()
            .unwrap();
        assert_eq!(c.status, Status::Warn);
        assert!(
            c.details
                .iter()
                .any(|d| d.contains("bad.toml") && d.contains("ignored")),
            "{c:?}"
        );
    }

    #[test]
    fn wayland_without_wl_copy_says_what_to_install() {
        let (_d, root) = root();
        let p = FakePlatform::new(linux_env(&[("WAYLAND_DISPLAY", "w")]));
        let r = run(&p, &root, &defaults());
        let c = r.check("clipboard").unwrap();
        assert_eq!(c.status, Status::Warn);
        assert_eq!(c.summary, "terminal (OSC 52)");
        assert!(
            c.hint.as_deref().unwrap().contains("install wl-clipboard"),
            "{c:?}"
        );
    }

    #[test]
    fn x11_without_tools_says_xclip_or_xsel() {
        let (_d, root) = root();
        let p = FakePlatform::new(linux_env(&[("DISPLAY", ":0")]));
        let r = run(&p, &root, &defaults());
        let c = r.check("clipboard").unwrap();
        assert_eq!(c.status, Status::Warn);
        assert_eq!(c.hint.as_deref(), Some("install xclip or xsel"));
        // with xsel installed the clipboard is fine and xclip is not required
        let p = FakePlatform::new(linux_env(&[("DISPLAY", ":0")])).with_tool("xsel", FakeTool::Ok);
        let r = run(&p, &root, &defaults());
        assert_eq!(r.check("clipboard").unwrap().status, Status::Pass);
        assert_eq!(r.check("clipboard").unwrap().summary, "xsel");
    }

    #[test]
    fn no_display_and_no_ssh_warns_on_the_display_and_uses_the_terminal() {
        let (_d, root) = root();
        let p = FakePlatform::new(linux_env(&[]));
        let r = run(&p, &root, &defaults());
        assert_eq!(status_of(&r, "display"), Status::Warn);
        assert!(
            r.check("display")
                .unwrap()
                .hint
                .as_deref()
                .unwrap()
                .contains("OSC 52")
        );
        assert_eq!(r.check("clipboard").unwrap().status, Status::Pass);
        assert_eq!(r.check("clipboard").unwrap().summary, "terminal (OSC 52)");
        // over SSH that is expected
        let p = FakePlatform::new(linux_env(&[("SSH_CONNECTION", "a b c d")]));
        assert_eq!(
            status_of(&run(&p, &root, &defaults()), "display"),
            Status::Pass
        );
    }

    #[test]
    fn terminal_app_ignores_osc_52() {
        let (_d, root) = root();
        let p = FakePlatform::new(mac_env(&[
            ("SSH_CONNECTION", "a b c d"),
            ("TERM_PROGRAM", "Apple_Terminal"),
        ]));
        let c = run(&p, &root, &defaults())
            .check("clipboard")
            .cloned()
            .unwrap();
        assert_eq!(c.status, Status::Warn);
        assert!(
            c.hint
                .as_deref()
                .unwrap()
                .contains("Terminal.app ignores OSC 52"),
            "{c:?}"
        );
        // another terminal over ssh is fine
        let p = FakePlatform::new(mac_env(&[
            ("SSH_CONNECTION", "a b c d"),
            ("TERM_PROGRAM", "iTerm.app"),
        ]));
        assert_eq!(
            run(&p, &root, &defaults())
                .check("clipboard")
                .unwrap()
                .status,
            Status::Pass
        );
    }

    #[test]
    fn tmux_that_does_not_forward_gets_the_allow_passthrough_line() {
        let (_d, root) = root();
        let env = [("SSH_CONNECTION", "a b c d"), ("TMUX", "x")];
        let p = FakePlatform::new(linux_env(&env)).with_tmux_passthrough(false);
        let c = run(&p, &root, &defaults())
            .check("clipboard")
            .cloned()
            .unwrap();
        assert_eq!(c.status, Status::Warn);
        assert!(
            c.hint
                .as_deref()
                .unwrap()
                .contains("set -g allow-passthrough on"),
            "{c:?}"
        );
        let p = FakePlatform::new(linux_env(&env)).with_tmux_passthrough(true);
        assert_eq!(
            run(&p, &root, &defaults())
                .check("clipboard")
                .unwrap()
                .status,
            Status::Pass
        );
    }

    #[test]
    fn the_stdout_method_is_a_warning() {
        let (_d, root) = root();
        let forced = loaded_from(&[], &[("KB_CLIPBOARD_METHOD", "stdout")]);
        let r = run(&healthy_mac(), &root, &forced);
        let c = r.check("clipboard").unwrap();
        assert_eq!(c.status, Status::Warn);
        assert!(c.summary.contains("printed"), "{c:?}");
        // and a machine with no terminal and no tools lands there by itself
        let p = FakePlatform::new(linux_env(&[])).without_terminal();
        let c = run(&p, &root, &defaults())
            .check("clipboard")
            .cloned()
            .unwrap();
        assert_eq!(c.status, Status::Warn);
    }

    #[test]
    fn a_custom_clipboard_command_is_checked() {
        let (_d, root) = root();
        let custom = loaded_from(&[], &[("KB_CLIPBOARD_COMMAND", "mycopy --flag")]);
        let p = healthy_mac().with_tool("mycopy", FakeTool::Ok);
        assert_eq!(
            run(&p, &root, &custom).check("clipboard").unwrap().summary,
            "mycopy"
        );
        let r = run(&healthy_mac(), &root, &custom);
        let c = r.check("clipboard").unwrap();
        assert!(
            c.hint
                .as_deref()
                .unwrap()
                .contains("`mycopy` was not found"),
            "{c:?}"
        );
    }

    #[test]
    fn the_size_limit_and_the_workaround_are_always_shown() {
        let (_d, root) = root();
        let big = loaded_from(&[], &[("KB_CLIPBOARD_OSC52_MAX_BYTES", "5000")]);
        for l in [defaults(), big] {
            let r = run(&healthy_mac(), &root, &l);
            let d = &r.check("clipboard").unwrap().details;
            assert!(
                d.iter().any(|x| x.contains("OSC 52 limit:")
                    && x.contains("kb show --raw <id> | <clipboard tool>")),
                "{d:?}"
            );
        }
        let big = loaded_from(&[], &[("KB_CLIPBOARD_OSC52_MAX_BYTES", "5000")]);
        let d = run(&healthy_mac(), &root, &big)
            .check("clipboard")
            .unwrap()
            .details
            .join(" ");
        assert!(d.contains("5000 bytes"), "{d}");
    }

    #[test]
    fn the_alternatives_are_listed() {
        let (_d, root) = root();
        let r = run(&healthy_mac(), &root, &defaults());
        assert_eq!(
            r.check("clipboard-alternatives").unwrap().summary,
            "also available: terminal (OSC 52)"
        );
    }

    #[test]
    fn true_color_is_a_warning_when_not_advertised() {
        let (_d, root) = root();
        let p = FakePlatform::new(mac_env(&[])).with_tool("pbcopy", FakeTool::Ok);
        assert_eq!(
            status_of(&run(&p, &root, &defaults()), "true-color"),
            Status::Warn
        );
    }

    #[test]
    fn the_editor_is_resolved_and_looked_up_on_path() {
        let (_d, root) = root();
        let nvim = loaded_from(&[], &[("KB_EDITOR", "nvim -u NONE")]);
        let found = healthy_mac().with_tool("nvim", FakeTool::Ok);
        let c = run(&found, &root, &nvim).check("editor").cloned().unwrap();
        assert_eq!(
            (c.status, c.summary.as_str()),
            (Status::Pass, "nvim -u NONE (from config)")
        );
        let c = run(&healthy_mac(), &root, &nvim)
            .check("editor")
            .cloned()
            .unwrap();
        assert_eq!(c.status, Status::Warn);
        assert!(
            c.hint.as_deref().unwrap().contains("kb config set editor"),
            "{c:?}"
        );
        // the default is vi
        let c = run(&healthy_mac(), &root, &defaults())
            .check("editor")
            .cloned()
            .unwrap();
        assert!(c.summary.starts_with("vi (from"), "{c:?}");
    }

    #[test]
    fn the_opener_is_checked_unless_there_is_nowhere_to_open() {
        let (_d, root) = root();
        let p = FakePlatform::new(linux_env(&[("DISPLAY", ":0")]));
        let c = run(&p, &root, &defaults())
            .check("opener")
            .cloned()
            .unwrap();
        assert_eq!(c.status, Status::Warn);
        assert!(c.hint.as_deref().unwrap().contains("xdg-utils"));
        let p =
            FakePlatform::new(linux_env(&[("DISPLAY", ":0")])).with_tool("xdg-open", FakeTool::Ok);
        assert_eq!(
            status_of(&run(&p, &root, &defaults()), "opener"),
            Status::Pass
        );
        let p = FakePlatform::new(linux_env(&[("SSH_TTY", "t")]));
        let c = run(&p, &root, &defaults())
            .check("opener")
            .cloned()
            .unwrap();
        assert_eq!(c.status, Status::Pass);
        assert!(c.summary.contains("printed"), "{c:?}");
    }

    #[test]
    fn only_a_failure_makes_the_report_not_ok() {
        let (_d, root) = root();
        // warnings everywhere (a bare Linux box) but nothing failed
        let p = FakePlatform::new(linux_env(&[]));
        let r = run(&p, &root, &defaults());
        assert!(r.checks.iter().any(|c| c.status == Status::Warn));
        assert!(r.ok());
    }

    #[test]
    fn the_json_has_the_documented_shape() {
        let (_d, root) = root();
        let r = run(&healthy_mac(), &root, &defaults());
        let doc = r.to_json();
        assert_eq!(doc["ok"], true);
        assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(doc["environment"]["os"], "macos");
        assert_eq!(doc["environment"]["ssh"], false);
        assert!(doc["paths"]["config"].is_string() && doc["paths"]["data"].is_string());
        let checks = doc["checks"].as_array().unwrap();
        assert_eq!(checks.len(), r.checks.len());
        let clip = checks.iter().find(|c| c["id"] == "clipboard").unwrap();
        assert_eq!(
            (clip["status"].as_str(), clip["summary"].as_str()),
            (Some("pass"), Some("pbcopy"))
        );
        assert!(clip["hint"].is_null() && clip["details"].is_array());
    }

    #[test]
    fn the_text_has_a_mark_an_id_and_a_fix_line() {
        let (_d, root) = root();
        let p = FakePlatform::new(linux_env(&[("WAYLAND_DISPLAY", "w")]));
        let r = run(&p, &root, &defaults());
        let plain = r.render(false);
        assert!(
            plain.starts_with(&format!(
                "kb {} on Linux (x86_64)\n",
                env!("CARGO_PKG_VERSION")
            )),
            "{plain}"
        );
        assert!(
            plain
                .lines()
                .any(|l| l.contains("warn") && l.contains("clipboard")),
            "{plain}"
        );
        assert!(plain.contains("fix: install wl-clipboard"), "{plain}");
        assert!(plain.contains(" ok   root"), "{plain}");
        let fancy = r.render(true);
        assert!(
            fancy.contains(" ✓ root") && fancy.contains(" ⚠ clipboard"),
            "{fancy}"
        );
        let (_d2, missing) = (0, root.join("nope"));
        let failed = run(&p, &missing, &defaults()).render(false);
        assert!(failed.contains("FAIL root"), "{failed}");
    }

    #[test]
    fn the_clipboard_read_check_looks_for_the_read_tools_only() {
        let (_d, root) = root();
        let r = run(&healthy_mac(), &root, &defaults());
        let c = r.check("clipboard-read").unwrap();
        assert_eq!(
            (c.status, c.summary.as_str()),
            (Status::Pass, "reads with pbpaste")
        );
        assert_eq!(c.details, ["pbpaste: found"]);
        // never reads the clipboard
        let p = healthy_mac();
        let _ = run(&p, &root, &defaults());
        assert!(
            p.calls()
                .iter()
                .all(|c| !matches!(c, crate::platform::fake::Call::RunReadTool { .. }))
        );

        let wayland = FakePlatform::new(linux_env(&[("WAYLAND_DISPLAY", "w")]));
        let r = run(&wayland, &root, &defaults());
        let c = r.check("clipboard-read").unwrap();
        assert_eq!(c.status, Status::Warn);
        assert_eq!(c.hint.as_deref(), Some("install wl-clipboard"));

        let x11 =
            FakePlatform::new(linux_env(&[("DISPLAY", ":0")])).with_tool("xsel", FakeTool::Ok);
        let r = run(&x11, &root, &defaults());
        let c = r.check("clipboard-read").unwrap();
        assert_eq!(c.summary, "reads with xsel");
        assert_eq!(c.details, ["xclip: not found", "xsel: found"]);

        let ssh = FakePlatform::new(mac_env(&[("SSH_TTY", "/dev/pts/1")]))
            .with_tool("pbpaste", FakeTool::Ok);
        let c = run(&ssh, &root, &defaults())
            .check("clipboard-read")
            .unwrap()
            .clone();
        assert_eq!(c.status, Status::Warn);
        assert!(c.hint.unwrap().contains("stdin or an empty form"));

        let none = FakePlatform::new(linux_env(&[]));
        let c = run(&none, &root, &defaults())
            .check("clipboard-read")
            .unwrap()
            .clone();
        assert_eq!(c.status, Status::Warn);
        assert!(c.hint.unwrap().contains("wl-clipboard"));
    }
}
