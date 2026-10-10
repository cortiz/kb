//! Local git clone discovery and caching.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteHost {
    GitHub,
    GitLab,
    Bitbucket,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalClone {
    pub repo_identifier: String,
    pub local_path: PathBuf,
    pub remote_url: String,
    pub host: RemoteHost,
    pub tracking_branch: String,
    pub has_local_changes: bool,
}
