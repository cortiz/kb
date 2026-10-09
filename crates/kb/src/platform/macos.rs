//! macOS: app data under `~/Library`.

use std::path::{Path, PathBuf};

use super::Platform;

pub struct MacOs;

impl Platform for MacOs {
    fn home_dir(&self) -> Option<PathBuf> {
        std::env::home_dir()
    }

    fn config_file(&self) -> Option<PathBuf> {
        self.home_dir().map(|h| config_file(&h))
    }
}

/// `~/Library/Application Support/kb/config.yml`.
fn config_file(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("kb")
        .join("config.yml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_support() {
        assert_eq!(
            config_file(Path::new("/Users/u")),
            PathBuf::from("/Users/u/Library/Application Support/kb/config.yml")
        );
    }
}
