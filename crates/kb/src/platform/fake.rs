//! A scripted `Platform` for tests: no real clipboard, display, terminal or tools. It records
//! every call so tests can say what happened and in which order.
//!
//! It is part of the library (not `#[cfg(test)]`) so the integration tests in `tests/` can use it.
//! `Platform` has no default methods, so adding a method to the trait does not compile until the
//! fake implements it too.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;

use super::{
    ConfigPaths, CopyTool, Editor, Environment, GitOutput, MAX_READ_BYTES, NativeTool, Opened,
    OpenerCommand, Os, Platform, TempFile, ToolError, clipboard, config_paths,
};

/// How a fake clipboard program behaves. A program that is not in the table is not installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeTool {
    Ok,
    Fail(i32),
    Hang,
}

/// How a fake clipboard read program behaves. A program that is not in the table is not installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeRead {
    /// Prints this text.
    Text(String),
    /// Prints these bytes (not necessarily UTF-8).
    Bytes(Vec<u8>),
    /// Is installed and exits with this status, printing nothing.
    Fail(i32),
    Timeout,
}

type GitScript = Box<dyn Fn(&[String]) -> Result<GitOutput, ToolError>>;
type EditEffect = Box<dyn Fn(&Path)>;

/// Something the code under test asked the platform to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    FindTool(String),
    RunCopyTool { tool: String, text: String },
    WriteTty(Vec<u8>),
    Open { target: String, opener: String },
    TmuxPassthrough,
    ResolveEditor,
    RunEditor { command: String, path: PathBuf },
    RunReadTool { tool: String },
    RunGit { args: Vec<String> },
    PrivateTempFile { extension: String },
}

pub struct FakePlatform {
    pub env: Environment,
    pub home: Option<PathBuf>,
    pub tools: BTreeMap<String, FakeTool>,
    pub tmux_passthrough: Option<bool>,
    /// There is a controlling terminal: `has_terminal` is true and `write_tty` works.
    pub terminal: bool,
    /// What `$VISUAL` and `$EDITOR` hold.
    pub visual: Option<String>,
    pub editor_var: Option<String>,
    pub reads: BTreeMap<String, FakeRead>,
    git: Option<GitScript>,
    /// Runs when the editor "opens" a file: what the user did in it.
    edit: Option<EditEffect>,
    /// The editor's exit status.
    pub editor_exit: i32,
    calls: RefCell<Vec<Call>>,
}

/// A Linux environment built from `vars` (an empty list is a bare terminal).
pub fn linux_env(vars: &[(&str, &str)]) -> Environment {
    env_for(Os::Linux, vars)
}

pub fn mac_env(vars: &[(&str, &str)]) -> Environment {
    env_for(Os::MacOs, vars)
}

fn env_for(os: Os, vars: &[(&str, &str)]) -> Environment {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    Environment::detect(
        &move |k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()),
        os,
        "x86_64",
    )
}

impl FakePlatform {
    pub fn new(env: Environment) -> FakePlatform {
        FakePlatform {
            env,
            home: Some(PathBuf::from("/home/fake")),
            tools: BTreeMap::new(),
            tmux_passthrough: None,
            terminal: true,
            visual: None,
            editor_var: None,
            reads: BTreeMap::new(),
            git: None,
            edit: None,
            editor_exit: 0,
            calls: RefCell::new(Vec::new()),
        }
    }

    pub fn with_tool(mut self, name: &str, behaviour: FakeTool) -> FakePlatform {
        self.tools.insert(name.to_string(), behaviour);
        self
    }

    /// Installs a clipboard read program that behaves like `behaviour`.
    pub fn with_read(mut self, name: &str, behaviour: FakeRead) -> FakePlatform {
        self.tools.insert(name.to_string(), FakeTool::Ok);
        self.reads.insert(name.to_string(), behaviour);
        self
    }

    /// Installs `git`, answering every call with `script`.
    pub fn with_git(
        mut self,
        script: impl Fn(&[String]) -> Result<GitOutput, ToolError> + 'static,
    ) -> FakePlatform {
        self.tools.insert("git".to_string(), FakeTool::Ok);
        self.git = Some(Box::new(script));
        self
    }

    /// What happens to the file when the editor runs on it.
    pub fn with_edit(mut self, effect: impl Fn(&Path) + 'static) -> FakePlatform {
        self.edit = Some(Box::new(effect));
        self
    }

    pub fn with_editor_exit(mut self, code: i32) -> FakePlatform {
        self.editor_exit = code;
        self
    }

    pub fn without_terminal(mut self) -> FakePlatform {
        self.terminal = false;
        self
    }

    pub fn with_tmux_passthrough(mut self, allowed: bool) -> FakePlatform {
        self.tmux_passthrough = Some(allowed);
        self
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }

    /// Everything written to the terminal, joined.
    pub fn tty_output(&self) -> Vec<u8> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::WriteTty(b) => Some(b),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn record(&self, call: Call) {
        self.calls.borrow_mut().push(call);
    }

    fn paths(&self) -> ConfigPaths {
        config_paths(self.env.os, None, self.home.as_deref())
    }
}

impl Platform for FakePlatform {
    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn config_files(&self) -> Vec<PathBuf> {
        self.paths().read
    }

    fn config_write_path(&self) -> Option<PathBuf> {
        self.paths().write
    }

    fn data_dir(&self) -> Option<PathBuf> {
        super::data_dir(self.env.os, None, self.home.as_deref())
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        super::cache_dir(self.env.os, None, self.home.as_deref())
    }

    fn env(&self) -> Environment {
        self.env.clone()
    }

    fn find_tool(&self, name: &str) -> Option<PathBuf> {
        self.record(Call::FindTool(name.to_string()));
        self.tools
            .contains_key(name)
            .then(|| PathBuf::from("/fake/bin").join(name))
    }

    fn tmux_allows_passthrough(&self) -> Option<bool> {
        self.record(Call::TmuxPassthrough);
        if self.env.tmux {
            Some(self.tmux_passthrough.unwrap_or(true))
        } else {
            None
        }
    }

    fn has_terminal(&self) -> bool {
        self.terminal
    }

    fn open_tty(&self) -> io::Result<File> {
        Err(io::Error::other("the fake platform has no terminal device"))
    }

    fn write_tty(&self, bytes: &[u8]) -> io::Result<()> {
        if !self.terminal {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "no controlling terminal",
            ));
        }
        self.record(Call::WriteTty(bytes.to_vec()));
        Ok(())
    }

    fn native_tools(&self, env: &Environment) -> Vec<NativeTool> {
        match env.os {
            Os::MacOs => clipboard::mac_tools(),
            Os::Linux => clipboard::linux_tools(env),
        }
    }

    fn run_copy_tool(
        &self,
        tool: &CopyTool,
        text: &str,
        _timeout: Duration,
    ) -> Result<(), ToolError> {
        let name = tool.label();
        self.record(Call::RunCopyTool {
            tool: name.clone(),
            text: text.to_string(),
        });
        match self.tools.get(&name) {
            None => Err(ToolError::NotFound),
            Some(FakeTool::Ok) => Ok(()),
            Some(FakeTool::Fail(code)) => Err(ToolError::Failed(Some(*code))),
            Some(FakeTool::Hang) => Err(ToolError::TimedOut),
        }
    }

    fn run_read_tool(&self, tool: &NativeTool, _timeout: Duration) -> Result<Vec<u8>, ToolError> {
        self.record(Call::RunReadTool {
            tool: tool.name.to_string(),
        });
        let bytes = match self.reads.get(tool.name) {
            None => return Err(ToolError::NotFound),
            Some(FakeRead::Text(s)) => s.clone().into_bytes(),
            Some(FakeRead::Bytes(b)) => b.clone(),
            Some(FakeRead::Fail(code)) => return Err(ToolError::Failed(Some(*code))),
            Some(FakeRead::Timeout) => return Err(ToolError::TimedOut),
        };
        if bytes.len() > MAX_READ_BYTES {
            return Err(ToolError::TooLarge);
        }
        Ok(bytes)
    }

    fn run_git(
        &self,
        _root: &Path,
        args: &[&str],
        _timeout: Duration,
    ) -> Result<GitOutput, ToolError> {
        let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
        self.record(Call::RunGit { args: args.clone() });
        match &self.git {
            Some(script) => script(&args),
            None => Err(ToolError::NotFound),
        }
    }

    fn private_temp_file(&self, extension: &str, contents: &[u8]) -> io::Result<TempFile> {
        self.record(Call::PrivateTempFile {
            extension: extension.to_string(),
        });
        super::unix::private_temp_file(extension, contents)
    }

    fn opener(&self, override_cmd: Option<&str>) -> OpenerCommand {
        let default = match self.env.os {
            Os::MacOs => "open",
            Os::Linux => "xdg-open",
        };
        OpenerCommand {
            command: override_cmd.unwrap_or(default).to_string(),
        }
    }

    fn open(&self, target: &str, opener: &OpenerCommand, env: &Environment) -> io::Result<Opened> {
        if super::unix::nowhere_to_open(env) {
            return Ok(Opened::Printed(target.to_string()));
        }
        self.record(Call::Open {
            target: target.to_string(),
            opener: opener.command.clone(),
        });
        Ok(Opened::Launched)
    }

    fn resolve_editor(&self, configured: Option<&str>) -> Editor {
        self.record(Call::ResolveEditor);
        super::pick_editor(
            configured,
            self.visual.as_deref(),
            self.editor_var.as_deref(),
        )
    }

    fn run_editor(
        &self,
        editor: &Editor,
        path: &Path,
        _tty: &File,
        _interrupt: &dyn Fn() -> bool,
    ) -> io::Result<Option<ExitStatus>> {
        use std::os::unix::process::ExitStatusExt;
        self.record(Call::RunEditor {
            command: editor.command.clone(),
            path: path.to_path_buf(),
        });
        if let Some(effect) = &self.edit {
            effect(path);
        }
        Ok(Some(ExitStatus::from_raw(self.editor_exit << 8)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SC-004: every method of the trait is exercised on the fake. A new trait method does not
    /// compile in `FakePlatform` until it is implemented, and this test is where it is called.
    #[test]
    fn every_platform_method_works_on_the_fake() {
        let p = FakePlatform::new(linux_env(&[("TMUX", "x"), ("WAYLAND_DISPLAY", "w")]))
            .with_tool("wl-copy", FakeTool::Ok)
            .with_tmux_passthrough(false);
        assert_eq!(p.home_dir(), Some(PathBuf::from("/home/fake")));
        assert_eq!(
            p.config_files()[0],
            PathBuf::from("/home/fake/.config/kb/config.toml")
        );
        assert_eq!(
            p.config_write_path(),
            Some(PathBuf::from("/home/fake/.config/kb/config.toml"))
        );
        assert!(p.data_dir().is_some() && p.cache_dir().is_some());
        assert_eq!(p.env().display, super::super::Display::Wayland);
        assert_eq!(
            p.find_tool("wl-copy"),
            Some(PathBuf::from("/fake/bin/wl-copy"))
        );
        assert_eq!(p.find_tool("xclip"), None);
        assert_eq!(p.tmux_allows_passthrough(), Some(false));
        assert!(p.has_terminal());
        assert!(p.open_tty().is_err());
        p.write_tty(b"\x1b]52;c;\x07").unwrap();
        assert_eq!(p.tty_output(), b"\x1b]52;c;\x07");
        let tools = p.native_tools(&p.env());
        assert_eq!(tools, [clipboard::WL_COPY]);
        let ok = CopyTool::Native(clipboard::WL_COPY);
        assert_eq!(p.run_copy_tool(&ok, "hi", Duration::from_secs(2)), Ok(()));
        let missing = CopyTool::Native(clipboard::XCLIP);
        assert_eq!(
            p.run_copy_tool(&missing, "hi", Duration::from_secs(2)),
            Err(ToolError::NotFound)
        );
        let opener = p.opener(None);
        assert_eq!(opener.command, "xdg-open");
        assert_eq!(p.opener(Some("firefox")).command, "firefox");
        assert_eq!(
            p.open("https://example.com", &opener, &p.env()).unwrap(),
            Opened::Launched
        );
        let tty = tempfile::tempfile().unwrap();
        let editor = p.resolve_editor(Some("nvim"));
        assert_eq!(editor.command, "nvim");
        let status = p
            .run_editor(&editor, Path::new("/x/e.md"), &tty, &|| false)
            .unwrap()
            .unwrap();
        assert!(status.success());
        assert!(p.calls().contains(&Call::RunEditor {
            command: "nvim".into(),
            path: PathBuf::from("/x/e.md")
        }));
        let p = FakePlatform::new(linux_env(&[]))
            .with_read("xclip", FakeRead::Text("pasted".into()))
            .with_git(|args| {
                Ok(GitOutput {
                    code: Some(0),
                    stdout: args.join(" "),
                    stderr: String::new(),
                })
            });
        let t = Duration::from_secs(2);
        assert_eq!(
            p.run_read_tool(&clipboard::XCLIP_OUT, t),
            Ok(b"pasted".to_vec())
        );
        assert_eq!(
            p.run_read_tool(&clipboard::XSEL_OUT, t),
            Err(ToolError::NotFound)
        );
        let git = p.run_git(Path::new("/kb"), &["status", "-s"], t).unwrap();
        assert!(git.success() && git.stdout == "status -s");
        let temp = p.private_temp_file("md", b"hello").unwrap();
        assert_eq!(temp.read_to_string().unwrap(), "hello");
        let path = temp.path().to_path_buf();
        drop(temp);
        assert!(!path.exists(), "dropped temp files are deleted");
    }

    #[test]
    fn scripted_tools_fail_and_hang_as_told() {
        let p = FakePlatform::new(mac_env(&[]))
            .with_tool("pbcopy", FakeTool::Fail(7))
            .with_tool("xclip", FakeTool::Hang);
        let t = Duration::from_secs(2);
        let pb = CopyTool::Native(clipboard::PBCOPY);
        assert_eq!(
            p.run_copy_tool(&pb, "x", t),
            Err(ToolError::Failed(Some(7)))
        );
        let xc = CopyTool::Native(clipboard::XCLIP);
        assert_eq!(p.run_copy_tool(&xc, "x", t), Err(ToolError::TimedOut));
    }

    #[test]
    fn without_a_terminal_writes_fail_and_nothing_is_recorded() {
        let p = FakePlatform::new(linux_env(&[])).without_terminal();
        assert!(!p.has_terminal());
        assert!(p.write_tty(b"x").is_err());
        assert!(p.tty_output().is_empty());
    }

    #[test]
    fn opening_over_ssh_without_a_display_prints() {
        let p = FakePlatform::new(linux_env(&[("SSH_CONNECTION", "a b c d")]));
        let opener = p.opener(None);
        assert_eq!(
            p.open("https://example.com", &opener, &p.env()).unwrap(),
            Opened::Printed("https://example.com".into())
        );
        assert!(p.calls().is_empty(), "nothing was launched");
    }

    #[test]
    fn calls_are_recorded_in_order() {
        let p = FakePlatform::new(mac_env(&[])).with_tool("pbcopy", FakeTool::Ok);
        let _ = p.find_tool("pbcopy");
        let _ = p.run_copy_tool(
            &CopyTool::Native(clipboard::PBCOPY),
            "payload",
            Duration::from_secs(2),
        );
        assert_eq!(
            p.calls(),
            [
                Call::FindTool("pbcopy".into()),
                Call::RunCopyTool {
                    tool: "pbcopy".into(),
                    text: "payload".into()
                }
            ]
        );
    }
}
