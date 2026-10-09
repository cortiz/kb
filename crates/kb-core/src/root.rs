//! Resolving the KB root: `--root`, then `KB_ROOT`, then the config file, then `~/kb` (FR-022).

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// Everything root resolution needs. The binary fills it from flags, the environment and its
/// platform layer; the core never reads the environment itself.
#[derive(Debug, Clone, Default)]
pub struct RootInputs {
    pub flag: Option<PathBuf>,
    pub env: Option<OsString>,
    pub config_file: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

/// Why the root could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootError {
    /// The config file exists but cannot be read or parsed.
    Config { path: PathBuf, message: String },
    /// Nothing set the root and there is no home directory.
    NoHome,
}

impl fmt::Display for RootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RootError::Config { path, message } => {
                write!(f, "cannot read config file {}: {message}", path.display())
            }
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
    if let Some(path) = &inputs.config_file
        && let Some(root) = config_root(path)?
    {
        return Ok(expand_tilde(Path::new(&root), home));
    }
    home.map(|h| h.join("kb")).ok_or(RootError::NoHome)
}

/// Reads `root` from the config file. A missing file is not an error.
fn config_root(path: &Path) -> Result<Option<String>, RootError> {
    let fail = |message: String| RootError::Config {
        path: path.to_path_buf(),
        message,
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(fail(e.to_string())),
    };
    let parsed = crate::yaml::parse(&text, 0);
    if let Some(error) = parsed.errors.first() {
        return Err(fail(error.message.clone()));
    }
    let Some(root) = parsed.root else {
        return Err(fail("expected `key: value` lines".into()));
    };
    match root.get("root") {
        None => Ok(None),
        Some(node) if node.is_null() => Ok(None),
        Some(node) => match node.as_str() {
            Some(s) => Ok(Some(s.to_string())),
            None => Err(fail(format!(
                "`root` must be a path, not a {}",
                node.kind_name()
            ))),
        },
    }
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

    fn config(text: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yml");
        std::fs::write(&path, text).unwrap();
        (dir, path)
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
        let (_d, cfg) = config("root: /from/config\n");
        let i = RootInputs {
            flag: Some("/from/flag".into()),
            env: Some("/from/env".into()),
            config_file: Some(cfg),
            ..inputs()
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/from/flag"));
    }

    #[test]
    fn env_beats_config() {
        let (_d, cfg) = config("root: /from/config\n");
        let i = RootInputs {
            env: Some("/from/env".into()),
            config_file: Some(cfg),
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
    fn config_root_with_tilde() {
        let (_d, cfg) = config("# my config\nroot: ~/notes\nother: ignored\n");
        let i = RootInputs {
            config_file: Some(cfg),
            ..inputs()
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/home/u/notes"));
    }

    #[test]
    fn missing_config_file_is_fine() {
        let i = RootInputs {
            config_file: Some("/nonexistent/kb/config.yml".into()),
            ..inputs()
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/home/u/kb"));
    }

    #[test]
    fn config_without_root_falls_through() {
        let (_d, cfg) = config("");
        let i = RootInputs {
            config_file: Some(cfg),
            ..inputs()
        };
        assert_eq!(resolve_root(&i).unwrap(), PathBuf::from("/home/u/kb"));
    }

    #[test]
    fn broken_config_names_the_file() {
        for text in ["root: [unclosed\n", "- a list\n", "root: 42\n"] {
            let (_d, cfg) = config(text);
            let i = RootInputs {
                config_file: Some(cfg.clone()),
                ..inputs()
            };
            match resolve_root(&i) {
                Err(e @ RootError::Config { .. }) => assert!(e.to_string().contains("config.yml")),
                other => panic!("{text:?}: {other:?}"),
            }
        }
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
