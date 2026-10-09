//! Linux: XDG base directories.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::Platform;

pub struct Linux;

impl Platform for Linux {
    fn home_dir(&self) -> Option<PathBuf> {
        std::env::home_dir()
    }

    fn config_file(&self) -> Option<PathBuf> {
        config_file(
            std::env::var_os("XDG_CONFIG_HOME"),
            self.home_dir().as_deref(),
        )
    }
}

/// `$XDG_CONFIG_HOME/kb/config.yml`, else `~/.config/kb/config.yml`. A relative or empty
/// `XDG_CONFIG_HOME` is ignored, as the XDG spec requires.
fn config_file(xdg_config_home: Option<OsString>, home: Option<&Path>) -> Option<PathBuf> {
    let base = match xdg_config_home.map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        _ => home?.join(".config"),
    };
    Some(base.join("kb").join("config.yml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_xdg_config_home() {
        let got = config_file(Some("/xdg".into()), Some(Path::new("/home/u")));
        assert_eq!(got, Some(PathBuf::from("/xdg/kb/config.yml")));
    }

    #[test]
    fn falls_back_to_dot_config() {
        let home = Some(Path::new("/home/u"));
        let want = Some(PathBuf::from("/home/u/.config/kb/config.yml"));
        assert_eq!(config_file(None, home), want);
        assert_eq!(config_file(Some("".into()), home), want);
        assert_eq!(config_file(Some("relative".into()), home), want);
    }

    #[test]
    fn no_home_no_xdg() {
        assert_eq!(config_file(None, None), None);
    }
}
