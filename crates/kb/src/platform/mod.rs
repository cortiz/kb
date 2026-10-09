//! OS-specific behavior, behind one trait (constitution V).
//!
//! The rules (which clipboard tool, where the config lives, what the environment means) are pure
//! functions in this module and its children, tested on any host. The `impl Platform` blocks in
//! `macos.rs` and `linux.rs` only read the real environment and run processes.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;

pub mod clipboard;
pub mod env;
pub mod fake;
// both implementations are compiled in tests on every host, so the Linux code is type-checked
// and unit-tested on a Mac too
#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(any(target_os = "macos", test))]
mod macos;
pub mod opener;
pub mod osc52;
mod unix;

pub use env::{Display, Environment, Os};

/// A clipboard program: its name, its arguments and what to tell the user to install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeTool {
    pub name: &'static str,
    pub args: &'static [&'static str],
    pub install_hint: &'static str,
}

/// A program that receives the text on stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyTool {
    Native(NativeTool),
    /// `clipboard.command`, run through `sh -c`.
    Shell(String),
}

impl CopyTool {
    /// What to call it in messages: `pbcopy`, or the first word of a custom command.
    pub fn label(&self) -> String {
        match self {
            CopyTool::Native(t) => t.name.to_string(),
            CopyTool::Shell(c) => c.split_whitespace().next().unwrap_or("command").to_string(),
        }
    }
}

/// Why a copy program did not deliver the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    NotFound,
    Failed(Option<i32>),
    TimedOut,
    Io(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::NotFound => f.write_str("not found"),
            ToolError::Failed(Some(code)) => write!(f, "exited with status {code}"),
            ToolError::Failed(None) => f.write_str("was stopped by a signal"),
            ToolError::TimedOut => f.write_str("timed out after 2 s"),
            ToolError::Io(e) => f.write_str(e),
        }
    }
}

/// The command that opens a URL or a file (`open`, `xdg-open` or `opener.command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenerCommand {
    pub command: String,
}

/// Where the editor command came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorSource {
    /// `editor` in the config, or `KB_EDITOR`.
    Config,
    /// `$VISUAL`.
    Visual,
    /// `$EDITOR`.
    EditorVar,
    /// Nothing was set: `vi`.
    Fallback,
}

/// The editor command: a program with optional arguments, split by the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    pub command: String,
    pub source: EditorSource,
}

/// FR-013: the configured editor (config `editor` or `KB_EDITOR`), then `$VISUAL`, then
/// `$EDITOR`, then `vi`. A value that is empty or only spaces counts as unset.
pub fn pick_editor(configured: Option<&str>, visual: Option<&str>, editor: Option<&str>) -> Editor {
    let set = |v: Option<&str>| {
        v.map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    if let Some(command) = set(configured) {
        Editor {
            command,
            source: EditorSource::Config,
        }
    } else if let Some(command) = set(visual) {
        Editor {
            command,
            source: EditorSource::Visual,
        }
    } else if let Some(command) = set(editor) {
        Editor {
            command,
            source: EditorSource::EditorVar,
        }
    } else {
        Editor {
            command: "vi".to_string(),
            source: EditorSource::Fallback,
        }
    }
}

/// What `open` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    Launched,
    /// There is nowhere to open it (SSH without a display); the caller prints this instead.
    Printed(String),
}

/// What kb needs from the operating system.
pub trait Platform {
    // ---- paths ----
    /// The user's home directory.
    fn home_dir(&self) -> Option<PathBuf>;
    /// Config files in the order they are read: `config.toml` candidates, then legacy `config.yml`.
    fn config_files(&self) -> Vec<PathBuf>;
    /// The file `kb config init` and `kb config set` write.
    fn config_write_path(&self) -> Option<PathBuf>;
    fn data_dir(&self) -> Option<PathBuf>;
    fn cache_dir(&self) -> Option<PathBuf>;

    // ---- environment ----
    fn env(&self) -> Environment;
    /// A program on `PATH`.
    fn find_tool(&self, name: &str) -> Option<PathBuf>;
    /// Whether tmux forwards escape sequences to the outer terminal. `None` outside tmux.
    fn tmux_allows_passthrough(&self) -> Option<bool>;

    // ---- terminal ----
    /// Whether there is a terminal to interact with: stdin is one, or the controlling terminal
    /// can be opened. A redirected stdout does not matter.
    fn has_terminal(&self) -> bool;
    /// The controlling terminal, for drawing and for the editor's stdio.
    fn open_tty(&self) -> io::Result<File>;
    /// Writes `bytes` to the controlling terminal in one go (OSC 52), never to stdout.
    fn write_tty(&self, bytes: &[u8]) -> io::Result<()>;

    // ---- clipboard ----
    /// The clipboard programs that apply to `env`, in the order they are tried.
    fn native_tools(&self, env: &Environment) -> Vec<NativeTool>;
    /// Runs `tool` with `text` on stdin; gives up after `timeout`.
    fn run_copy_tool(
        &self,
        tool: &CopyTool,
        text: &str,
        timeout: Duration,
    ) -> Result<(), ToolError>;

    // ---- opening ----
    fn opener(&self, override_cmd: Option<&str>) -> OpenerCommand;
    /// Opens a URL or file; over SSH with no display it returns `Opened::Printed` instead.
    fn open(&self, target: &str, opener: &OpenerCommand, env: &Environment) -> io::Result<Opened>;

    // ---- editor ----
    /// The editor to run: `configured` (the `editor` setting, which includes `KB_EDITOR`), else
    /// `$VISUAL`, else `$EDITOR`, else `vi`.
    fn resolve_editor(&self, configured: Option<&str>) -> Editor;
    /// Opens `path` in `editor` on the terminal `tty`. When `interrupt` becomes true the editor
    /// is killed and `None` is returned.
    fn run_editor(
        &self,
        editor: &Editor,
        path: &Path,
        tty: &File,
        interrupt: &dyn Fn() -> bool,
    ) -> io::Result<Option<ExitStatus>>;
}

/// The platform kb is running on.
pub fn current() -> impl Platform {
    #[cfg(target_os = "linux")]
    return linux::Linux;
    #[cfg(target_os = "macos")]
    return macos::MacOs;
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("kb supports Linux and macOS");

/// Over SSH with no display there is nothing to open a browser on.
pub fn nowhere_to_open(env: &Environment) -> bool {
    unix::nowhere_to_open(env)
}

// ---- paths: pure rules ----

/// Where the config file is read from and written to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    pub read: Vec<PathBuf>,
    pub write: Option<PathBuf>,
}

/// FR-011. Linux: `$XDG_CONFIG_HOME/kb` (absolute only) else `~/.config/kb`. macOS: `~/.config/kb`
/// first, then `~/Library/Application Support/kb`. `config.toml` candidates come before the legacy
/// `config.yml` ones, and writes always go to the first candidate.
pub fn config_paths(os: Os, xdg_config_home: Option<&str>, home: Option<&Path>) -> ConfigPaths {
    let mut dirs: Vec<PathBuf> = Vec::new();
    match os {
        Os::Linux => {
            let xdg = xdg_config_home
                .map(PathBuf::from)
                .filter(|p| p.is_absolute());
            if let Some(base) = xdg.or_else(|| home.map(|h| h.join(".config"))) {
                dirs.push(base.join("kb"));
            }
        }
        Os::MacOs => {
            if let Some(h) = home {
                dirs.push(h.join(".config").join("kb"));
                dirs.push(h.join("Library").join("Application Support").join("kb"));
            }
        }
    }
    let mut read: Vec<PathBuf> = dirs.iter().map(|d| d.join("config.toml")).collect();
    read.extend(dirs.iter().map(|d| d.join("config.yml")));
    let write = dirs.first().map(|d| d.join("config.toml"));
    ConfigPaths { read, write }
}

/// Where per-user data and cache would live (kb keeps its own state in `<root>/.kb/`).
pub fn data_dir(os: Os, xdg_data_home: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    match os {
        Os::MacOs => home.map(|h| h.join("Library").join("Application Support").join("kb")),
        Os::Linux => xdg_or(xdg_data_home, home, ".local/share"),
    }
}

pub fn cache_dir(os: Os, xdg_cache_home: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    match os {
        Os::MacOs => home.map(|h| h.join("Library").join("Caches").join("kb")),
        Os::Linux => xdg_or(xdg_cache_home, home, ".cache"),
    }
}

fn xdg_or(xdg: Option<&str>, home: Option<&Path>, fallback: &str) -> Option<PathBuf> {
    xdg.map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| h.join(fallback)))
        .map(|base| base.join("kb"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn linux_uses_an_absolute_xdg_config_home() {
        let got = config_paths(Os::Linux, Some("/xdg"), Some(Path::new("/home/u")));
        assert_eq!(
            got.read,
            [p("/xdg/kb/config.toml"), p("/xdg/kb/config.yml")]
        );
        assert_eq!(got.write, Some(p("/xdg/kb/config.toml")));
    }

    #[test]
    fn linux_falls_back_to_dot_config_for_missing_empty_or_relative_xdg() {
        let home = Some(Path::new("/home/u"));
        for xdg in [None, Some(""), Some("relative/dir")] {
            let got = config_paths(Os::Linux, xdg, home);
            assert_eq!(
                got.read,
                [
                    p("/home/u/.config/kb/config.toml"),
                    p("/home/u/.config/kb/config.yml")
                ],
                "{xdg:?}"
            );
            assert_eq!(got.write, Some(p("/home/u/.config/kb/config.toml")));
        }
    }

    #[test]
    fn macos_reads_dot_config_first_then_library_and_writes_dot_config() {
        let got = config_paths(Os::MacOs, None, Some(Path::new("/Users/u")));
        assert_eq!(
            got.read,
            [
                p("/Users/u/.config/kb/config.toml"),
                p("/Users/u/Library/Application Support/kb/config.toml"),
                p("/Users/u/.config/kb/config.yml"),
                p("/Users/u/Library/Application Support/kb/config.yml"),
            ]
        );
        assert_eq!(got.write, Some(p("/Users/u/.config/kb/config.toml")));
    }

    #[test]
    fn macos_ignores_xdg_config_home() {
        let got = config_paths(Os::MacOs, Some("/xdg"), Some(Path::new("/Users/u")));
        assert!(got.read.iter().all(|f| !f.starts_with("/xdg")));
    }

    #[test]
    fn without_a_home_there_is_nowhere_to_look() {
        for os in [Os::Linux, Os::MacOs] {
            let got = config_paths(os, None, None);
            assert!(got.read.is_empty() && got.write.is_none(), "{os:?}");
        }
        // an absolute XDG_CONFIG_HOME still works without a home on Linux
        let got = config_paths(Os::Linux, Some("/xdg"), None);
        assert_eq!(got.write, Some(p("/xdg/kb/config.toml")));
    }

    #[test]
    fn data_and_cache_dirs_follow_each_platforms_convention() {
        let h = Some(Path::new("/home/u"));
        assert_eq!(
            data_dir(Os::Linux, None, h),
            Some(p("/home/u/.local/share/kb"))
        );
        assert_eq!(data_dir(Os::Linux, Some("/data"), h), Some(p("/data/kb")));
        assert_eq!(cache_dir(Os::Linux, None, h), Some(p("/home/u/.cache/kb")));
        assert_eq!(
            cache_dir(Os::Linux, Some("rel"), h),
            Some(p("/home/u/.cache/kb"))
        );
        let m = Some(Path::new("/Users/u"));
        assert_eq!(
            data_dir(Os::MacOs, None, m),
            Some(p("/Users/u/Library/Application Support/kb"))
        );
        assert_eq!(
            cache_dir(Os::MacOs, None, m),
            Some(p("/Users/u/Library/Caches/kb"))
        );
        assert_eq!(data_dir(Os::MacOs, None, None), None);
    }

    #[test]
    fn the_editor_comes_from_config_then_visual_then_editor_then_vi() {
        let pick = |c, v, e| pick_editor(c, v, e);
        let all = pick(Some("nvim"), Some("code -w"), Some("nano"));
        assert_eq!(
            (all.command.as_str(), all.source),
            ("nvim", EditorSource::Config)
        );
        let v = pick(None, Some("code -w"), Some("nano"));
        assert_eq!(
            (v.command.as_str(), v.source),
            ("code -w", EditorSource::Visual)
        );
        let e = pick(None, None, Some("nano"));
        assert_eq!(
            (e.command.as_str(), e.source),
            ("nano", EditorSource::EditorVar)
        );
        let f = pick(None, None, None);
        assert_eq!(
            (f.command.as_str(), f.source),
            ("vi", EditorSource::Fallback)
        );
    }

    #[test]
    fn empty_or_blank_values_count_as_unset_at_every_step() {
        let e = pick_editor(Some(""), Some("  "), Some("nano"));
        assert_eq!(
            (e.command.as_str(), e.source),
            ("nano", EditorSource::EditorVar)
        );
        let e = pick_editor(Some(" "), Some(""), Some(""));
        assert_eq!(
            (e.command.as_str(), e.source),
            ("vi", EditorSource::Fallback)
        );
        let e = pick_editor(Some("  nvim -u NONE "), None, None);
        assert_eq!(e.command, "nvim -u NONE", "surrounding spaces are trimmed");
    }

    #[test]
    fn tool_errors_read_well_in_messages() {
        assert_eq!(ToolError::NotFound.to_string(), "not found");
        assert_eq!(
            ToolError::Failed(Some(1)).to_string(),
            "exited with status 1"
        );
        assert_eq!(ToolError::TimedOut.to_string(), "timed out after 2 s");
    }

    #[test]
    fn a_tool_is_labelled_by_its_name_or_its_first_word() {
        let t = NativeTool {
            name: "pbcopy",
            args: &[],
            install_hint: "",
        };
        assert_eq!(CopyTool::Native(t).label(), "pbcopy");
        assert_eq!(
            CopyTool::Shell("xclip -selection clipboard".into()).label(),
            "xclip"
        );
    }
}
