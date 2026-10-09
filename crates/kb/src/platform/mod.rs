//! OS-specific behavior, behind one trait (constitution V).

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod unix;

/// What kb needs from the operating system.
pub trait Platform {
    /// The user's home directory.
    fn home_dir(&self) -> Option<PathBuf>;
    /// Where kb's config file lives.
    fn config_file(&self) -> Option<PathBuf>;
    /// Whether there is a terminal to interact with: stdin is one, or the controlling terminal
    /// can be opened. A redirected stdout does not matter.
    fn has_terminal(&self) -> bool;
    /// The controlling terminal, for drawing and for the editor's stdio.
    fn open_tty(&self) -> io::Result<File>;
    /// Opens `path` in `$VISUAL`, else `$EDITOR`, else `vi`, on the terminal `tty`. When
    /// `interrupt` becomes true the editor is killed and `None` is returned.
    fn run_editor(
        &self,
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
