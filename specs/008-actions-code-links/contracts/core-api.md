# Contract: Core Domain API

**Branch**: `008-actions-code-links` | **Date**: 2026-10-09 | **Spec**: [spec.md](../spec.md)

This contract defines the Rust API exposed by `kb-core` for actions, process run plans, console deep link builders, and git code tracking.

---

## 1. Action Registry (`kb_core::action`)

```rust
pub struct ActionService;

impl ActionService {
    /// Returns the list of actions available for a given entry,
    /// combining built-in actions and configured custom actions.
    pub fn list_actions(
        entry: &Entry,
        config: &Config,
        clone_status: Option<&LocalClone>,
    ) -> Vec<Action>;

    /// Identifies the primary open action for an entry type.
    pub fn primary_action(
        entry: &Entry,
        config: &Config,
        clone_status: Option<&LocalClone>,
    ) -> Option<Action>;

    /// Validates custom action configurations against built-in shortcuts,
    /// returning warnings/disables for key conflicts.
    pub fn validate_custom_actions(
        custom_actions: &[CustomActionConfig],
    ) -> Vec<ActionConfigWarning>;
}
```

---

## 2. Command Execution & Run Plans (`kb_core::run`)

```rust
pub struct RunService;

impl RunService {
    /// Builds a RunPlan for a `cmd` entry or a custom action,
    /// resolving environment secrets and detecting destructive commands.
    pub fn build_run_plan(
        entry: &Entry,
        resolved_vars: &ResolvedVariables,
        custom_action: Option<&CustomActionConfig>,
        extra_destructive_patterns: &[String],
    ) -> Result<RunPlan, RunPlanError>;

    /// Scans a command string against built-in and user destructive patterns.
    pub fn check_destructive(
        command: &str,
        extra_patterns: &[String],
    ) -> Vec<DestructiveWarning>;
}

#[derive(Debug, thiserror::Error)]
pub enum RunPlanError {
    #[error("Working directory '{0}' does not exist")]
    MissingCwd(PathBuf),
    #[error("Entry is missing required field '{0}' for action placeholder")]
    MissingPlaceholderField(String),
    #[error("Variable resolution failed: {0}")]
    VarResolution(String),
}
```

---

## 3. Console Deep Links (`kb_core::console`)

```rust
pub struct ConsoleService;

impl ConsoleService {
    /// Constructs a Sumo Logic search deep link.
    pub fn build_sumo_link(
        base_url: &str,
        resolved_query: &str,
        resolved_range: Option<&str>,
    ) -> Result<ConsoleLink, ConsoleLinkError>;

    /// Constructs an AWS CloudWatch Logs Insights console deep link.
    pub fn build_cw_link(
        region: &str,
        log_groups: &[String],
        resolved_query: &str,
        resolved_range: Option<&str>,
    ) -> Result<ConsoleLink, ConsoleLinkError>;

    /// Resolves AWS region using the precedence chain:
    /// entry.region -> config.aws.default_region -> AWS_REGION -> AWS_DEFAULT_REGION
    pub fn resolve_aws_region(
        entry: &Entry,
        config: &Config,
        env_provider: &impl EnvProvider,
    ) -> Result<String, MissingRegionError>;
}
```

---

## 4. Git Code Tracking (`kb_core::code`)

```rust
pub struct CodeService;

impl CodeService {
    /// Discovers a local clone for a given repository owner/name.
    pub fn find_clone(
        repo_id: &str,
        config: &Config,
        cache: &mut CloneCache,
        git_runner: &impl GitRunner,
    ) -> Result<Option<LocalClone>, GitError>;

    /// Fetches the latest remote commits for the clone if requested.
    pub fn fetch_remote(
        clone: &LocalClone,
        git_runner: &impl GitRunner,
    ) -> Result<(), GitError>;

    /// Computes drift status between the pinned commit SHA and the tracking branch tip.
    pub fn check_drift(
        entry: &Entry,
        clone: &LocalClone,
        git_runner: &impl GitRunner,
    ) -> Result<DriftReport, GitError>;

    /// Reads code preview lines around the pinned line from the git object store.
    pub fn get_snippet(
        clone: &LocalClone,
        pinned_ref: &str,
        path: &str,
        line: usize,
        end_line: Option<usize>,
        context_before: usize,
        context_after: usize,
        git_runner: &impl GitRunner,
    ) -> Result<CodeSnippet, GitError>;

    /// Builds a permalink for GitHub, GitLab, Bitbucket, or a custom host.
    pub fn build_permalink(
        clone: Option<&LocalClone>,
        repo_id: &str,
        commit_sha: &str,
        path: &str,
        line: usize,
        end_line: Option<usize>,
        config: &Config,
    ) -> Result<String, PermalinkError>;

    /// Updates entry frontmatter with new ref, path, line, and updated date,
    /// refusing write if file was changed on disk concurrently.
    pub fn repin_entry(
        entry_file_path: &Path,
        current_content: &str,
        report: &DriftReport,
        today_date: &str,
    ) -> Result<String, RepinError>;
}
```
