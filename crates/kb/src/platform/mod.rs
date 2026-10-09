//! OS-specific behavior, behind one trait (constitution V).

use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

/// What kb needs from the operating system.
pub trait Platform {
    /// The user's home directory.
    fn home_dir(&self) -> Option<PathBuf>;
    /// Where kb's config file lives.
    fn config_file(&self) -> Option<PathBuf>;
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
