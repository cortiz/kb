//! Resolving the KB root: `--root`, then `KB_ROOT`, then the config's `root`, then `~/kb`. The
//! first three are merged by `config::load`; `config_root` carries the result.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// Everything root resolution needs. The binary fills it from flags, the environment and its
/// platform layer; the core never reads the environment itself.
#[derive(Debug, Clone, Default)]
pub struct RootInputs {
    pub flag: Option<PathBuf>,
    pub env: Option<OsString>,
    /// The `root` setting (already merged from flag, environment and file by `config::load`).
    pub config_root: Option<String>,
    pub home: Option<PathBuf>,
}

/// Why the root could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootError {
    /// Nothing set the root and there is no home directory.
    NoHome,
}

impl fmt::Display for RootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RootError::NoHome => write!(f, "cannot find the home directory; set KB_ROOT or --root"),
        }
    }
}

impl std::error::Error for RootError {}

/// Resolves the KB root.
pub fn resolve_root(inputs: &RootInputs) -> Result<PathBuf, RootError> {
    let home = inputs.home.as_deref();
    if let Some(flag) = &inputs.flag {
        return Ok(expand_tilde(flag, home));
    }
    if let Some(env) = inputs.env.as_ref().filter(|e| !e.is_empty()) {
        return Ok(expand_tilde(Path::new(env), home));
    }
    if let Some(root) = inputs.config_root.as_deref().filter(|r| !r.is_empty()) {
        return Ok(expand_tilde(Path::new(root), home));
    }
    home.map(|h| h.join("kb")).ok_or(RootError::NoHome)
}

/// Expands a leading `~/` (or a lone `~`) with `home`.
pub fn expand_tilde(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return path.to_path_buf();
    };
    match path.strip_prefix("~") {
        Ok(rest) if path.as_os_str() == "~" || path.to_string_lossy().starts_with("~/") => {
            home.join(rest)
        }
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/u")
    }

    fn inputs() -> RootInputs {
        RootInputs {
            home: Some(home()),
            ..Default::default()
        }
    }

    #[test]
    fn the_config_setting_sits_between_the_env_and_the_default() {
        let i = RootInputs {
            config_root: Some("~/notes".into()),
            ..inputs()
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/home/u/notes"));
        let i = RootInputs {
            env: Some("/from/env".into()),
            ..i
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/from/env"));
        let i = RootInputs {
            flag: Some("/from/flag".into()),
            ..i
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/from/flag"));
        let i = RootInputs {
            config_root: Some(String::new()),
            ..inputs()
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/home/u/kb"));
    }

    #[test]
    fn default_is_home_kb() {
        assert_eq!(
            resolve_root(&inputs()).unwrap(),
            PathBuf::from("/home/u/kb")
        );
    }

    #[test]
    fn flag_beats_everything() {
        let i = RootInputs {
            flag: Some("/from/flag".into()),
            env: Some("/from/env".into()),
            config_root: Some("/from/config".into()),
            ..inputs()
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/from/flag"));
    }

    #[test]
    fn env_beats_config() {
        let i = RootInputs {
            env: Some("/from/env".into()),
            config_root: Some("/from/config".into()),
            ..inputs()
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/from/env"));
    }

    #[test]
    fn empty_env_is_ignored() {
        let i = RootInputs {
            env: Some("".into()),
            ..inputs()
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/home/u/kb"));
    }

    #[test]
    fn no_home_and_nothing_set() {
        assert_eq!(resolve_root(&RootInputs::default()), Err(RootError::NoHome));
    }

    #[test]
    fn tilde_expansion() {
        let h = home();
        assert_eq!(
            expand_tilde(Path::new("~/a/b"), Some(&h)),
            PathBuf::from("/home/u/a/b")
        );
        assert_eq!(
            expand_tilde(Path::new("~"), Some(&h)),
            PathBuf::from("/home/u")
        );
        assert_eq!(
            expand_tilde(Path::new("/abs"), Some(&h)),
            PathBuf::from("/abs")
        );
        assert_eq!(expand_tilde(Path::new("~x"), Some(&h)), PathBuf::from("~x"));
        assert_eq!(expand_tilde(Path::new("~/a"), None), PathBuf::from("~/a"));
    }
}
