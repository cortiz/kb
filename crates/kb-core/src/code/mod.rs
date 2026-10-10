//! Code link management: clone discovery, git diff drift detection, permalinks, and re-pinning.

pub mod clone;
pub mod drift;
pub mod permalink;
pub mod repin;
pub mod snippet;

pub use clone::{
    CachedCloneEntry, CloneCache, GitError, GitRunner, LocalClone, RemoteHost,
    normalize_remote_url,
};
pub use drift::{DriftConfidence, DriftReport, DriftStatus};
pub use permalink::PermalinkError;
pub use repin::RepinError;
pub use snippet::CodeSnippet;

use crate::config::Settings;

pub struct CodeService;

impl CodeService {
    /// Discovers a local clone for a given repository owner/name.
    pub fn find_clone(
        repo_id: &str,
        settings: &Settings,
        cache: &mut CloneCache,
        git_runner: &impl GitRunner,
    ) -> Result<Option<LocalClone>, GitError> {
        clone::find_clone_impl(repo_id, settings, cache, git_runner)
    }

    /// Fetches the latest remote commits for the clone.
    pub fn fetch_remote(
        clone: &LocalClone,
        git_runner: &impl GitRunner,
    ) -> Result<(), GitError> {
        git_runner.run_git(&clone.local_path, &["fetch"])?;
        Ok(())
    }

    /// Computes drift status between the pinned commit SHA and the tracking branch tip.
    pub fn check_drift(
        entry: &crate::entry::Entry,
        clone: &LocalClone,
        git_runner: &impl GitRunner,
    ) -> Result<DriftReport, GitError> {
        drift::check_drift_impl(entry, clone, git_runner)
    }

    /// Updates entry frontmatter with new ref, path, line, and updated date,
    /// refusing write if file was changed on disk concurrently.
    pub fn repin_entry(
        entry_file_path: &std::path::Path,
        current_content: &str,
        report: &DriftReport,
        today_date: &str,
    ) -> Result<String, RepinError> {
        repin::repin_entry_impl(entry_file_path, current_content, report, today_date)
    }
}
