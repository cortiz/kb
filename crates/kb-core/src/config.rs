//! The settings the TUI reads from the config file (`theme`, `mouse`).

use std::fmt;
use std::path::{Path, PathBuf};

/// Which colors the TUI uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    #[default]
    Auto,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Config {
    pub theme: ThemeChoice,
    pub mouse: bool,
}

/// The config file exists but can't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ConfigError {}

/// Reads `theme` and `mouse`. A missing file (or no file) means the defaults; unknown keys are
/// ignored.
pub fn read_config(path: Option<&Path>) -> Result<Config, ConfigError> {
    let Some(path) = path else {
        return Ok(Config::default());
    };
    let fail = |message: String| ConfigError {
        path: path.to_path_buf(),
        message,
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(fail(e.to_string())),
    };
    let parsed = crate::yaml::parse(&text, 0);
    if let Some(error) = parsed.errors.first() {
        return Err(fail(error.message.clone()));
    }
    let Some(root) = parsed.root else {
        return Ok(Config::default());
    };
    let mut config = Config::default();
    if let Some(node) = root.get("theme").filter(|n| !n.is_null()) {
        config.theme = match node.as_str() {
            Some("auto") => ThemeChoice::Auto,
            Some("dark") => ThemeChoice::Dark,
            Some("light") => ThemeChoice::Light,
            _ => return Err(fail("theme must be auto, dark or light".into())),
        };
    }
    if let Some(node) = root.get("mouse").filter(|n| !n.is_null()) {
        config.mouse = match node.scalar() {
            Some("true") => true,
            Some("false") => false,
            _ => return Err(fail("mouse must be true or false".into())),
        };
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Result<Config, ConfigError> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.yml");
        std::fs::write(&p, text).unwrap();
        read_config(Some(&p))
    }

    #[test]
    fn defaults() {
        assert_eq!(read_config(None).unwrap(), Config::default());
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            read_config(Some(&d.path().join("missing.yml"))).unwrap(),
            Config {
                theme: ThemeChoice::Auto,
                mouse: false
            }
        );
        assert_eq!(read("root: ~/kb\n").unwrap(), Config::default());
    }

    #[test]
    fn values() {
        let c = read("root: ~/kb\ntheme: dark\nmouse: true\nfuture_key: 1\n").unwrap();
        assert_eq!(
            c,
            Config {
                theme: ThemeChoice::Dark,
                mouse: true
            }
        );
        assert_eq!(read("theme: light\n").unwrap().theme, ThemeChoice::Light);
        assert_eq!(
            read("theme: auto\nmouse: false\n").unwrap(),
            Config::default()
        );
    }

    #[test]
    fn bad_values_name_the_key() {
        let e = read("theme: blue\n").unwrap_err();
        assert!(
            e.message.contains("theme") && e.message.contains("auto, dark or light"),
            "{e}"
        );
        assert!(e.to_string().contains("config.yml"), "{e}");
        let e = read("mouse: maybe\n").unwrap_err();
        assert!(e.message.contains("mouse"), "{e}");
        assert!(read("theme: [a\n").is_err());
    }
}
