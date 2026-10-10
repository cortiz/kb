//! macOS: app data under `~/Library`.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::{Path, PathBuf};

use std::time::Duration;

use super::{
    CopyTool, Editor, Environment, NativeTool, Opened, OpenerCommand, Os, Platform, ToolError,
    clipboard,
};

pub struct MacOs;

impl MacOs {
    fn config_paths(&self) -> super::ConfigPaths {
        super::config_paths(Os::MacOs, None, self.home_dir().as_deref())
    }
}

impl Platform for MacOs {
    fn home_dir(&self) -> Option<PathBuf> {
        std::env::home_dir()
    }

    fn config_files(&self) -> Vec<PathBuf> {
        self.config_paths().read
    }

    fn config_write_path(&self) -> Option<PathBuf> {
        self.config_paths().write
    }

    fn data_dir(&self) -> Option<PathBuf> {
        super::data_dir(
            Os::MacOs,
            std::env::var("XDG_DATA_HOME").ok().as_deref(),
            self.home_dir().as_deref(),
        )
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        super::cache_dir(
            Os::MacOs,
            std::env::var("XDG_CACHE_HOME").ok().as_deref(),
            self.home_dir().as_deref(),
        )
    }

    fn env(&self) -> Environment {
        Environment::current()
    }

    fn find_tool(&self, name: &str) -> Option<PathBuf> {
        super::unix::find_tool(name)
    }

    fn tmux_allows_passthrough(&self) -> Option<bool> {
        super::unix::tmux_allows_passthrough(&self.env())
    }

    fn write_tty(&self, bytes: &[u8]) -> std::io::Result<()> {
        super::unix::write_tty(bytes)
    }

    fn native_tools(&self, _env: &Environment) -> Vec<NativeTool> {
        clipboard::mac_tools()
    }

    fn run_copy_tool(
        &self,
        tool: &CopyTool,
        text: &str,
        timeout: Duration,
    ) -> Result<(), ToolError> {
        super::unix::run_copy_tool(tool, text, timeout)
    }

    fn run_read_tool(&self, tool: &NativeTool, timeout: Duration) -> Result<Vec<u8>, ToolError> {
        super::unix::run_read_tool(tool, timeout)
    }

    fn run_git(
        &self,
        root: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<super::GitOutput, ToolError> {
        super::unix::run_git(root, args, timeout)
    }

    fn private_temp_file(
        &self,
        extension: &str,
        contents: &[u8],
    ) -> std::io::Result<super::TempFile> {
        super::unix::private_temp_file(extension, contents)
    }

    fn opener(&self, override_cmd: Option<&str>) -> OpenerCommand {
        OpenerCommand {
            command: override_cmd.unwrap_or("open").to_string(),
        }
    }

    fn open(
        &self,
        target: &str,
        opener: &OpenerCommand,
        env: &Environment,
    ) -> std::io::Result<Opened> {
        super::unix::open(target, opener, env)
    }

    fn has_terminal(&self) -> bool {
        super::unix::has_terminal()
    }

    fn open_tty(&self) -> std::io::Result<std::fs::File> {
        super::unix::open_tty()
    }

    fn resolve_editor(&self, configured: Option<&str>) -> Editor {
        super::unix::resolve_editor(configured)
    }

    fn run_editor(
        &self,
        editor: &Editor,
        path: &Path,
        tty: &std::fs::File,
        interrupt: &dyn Fn() -> bool,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        super::unix::run_editor(editor, path, tty, interrupt)
    }

    fn run_interactive_command(
        &self,
        shell: &str,
        command: &str,
        cwd: Option<&Path>,
        env_vars: &[(String, String)],
    ) -> std::io::Result<super::RunResult> {
        super::unix::run_interactive_command(shell, command, cwd, env_vars)
    }
}
