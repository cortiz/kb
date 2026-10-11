# Core API Contracts: Git Sync

This document defines the Rust library interfaces exposed by `kb-core` for git operations, synchronization, secret gating, conflict resolution, and history inspection.

---

## 1. Git Runner & Operations Trait

Extends the abstraction in `kb-core::code::clone` into a generalized git engine:

```rust
pub trait GitRunner: Send + Sync {
    /// Executes a git command in the specified directory, returning captured stdout.
    fn run_git(&self, cwd: &Path, args: &[&str]) -> Result<String, GitError>;

    /// Executes a git command with timeout and returns exit code, stdout, and stderr.
    fn run_git_status(&self, cwd: &Path, args: &[&str], timeout: Duration) -> Result<GitCommandResult, GitError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommandResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    NotFound,
    CommandFailed { exit_code: Option<i32>, stderr: String },
    Io(String),
    TimedOut,
}
```

---

## 2. Sync Engine API (`kb_core::sync`)

```rust
pub struct SyncEngine<'a, G: GitRunner> {
    root: &'a Path,
    git: &'a G,
    config: SyncConfig,
}

impl<'a, G: GitRunner> SyncEngine<'a, G> {
    pub fn new(root: &'a Path, git: &'a G, config: SyncConfig) -> Self;

    /// Queries the current repository sync status without network activity.
    pub fn status(&self) -> Result<SyncStatus, SyncError>;

    /// Fetches from the remote upstream branch.
    pub fn fetch(&self, timeout: Duration) -> Result<(), SyncError>;

    /// Plans the synchronization operation without applying changes.
    pub fn plan(&self) -> Result<SyncPlan, SyncError>;

    /// Executes the full sync pipeline (commit hand-edits, fetch, integrate, push, refresh).
    pub fn execute<H: ConflictHandler>(&self, handler: &mut H) -> Result<SyncSummary, SyncError>;

    /// Resumes conflict resolution for an interrupted sync.
    pub fn continue_sync<H: ConflictHandler>(&self, handler: &mut H) -> Result<SyncSummary, SyncError>;

    /// Aborts an in-progress sync and rolls back to pre-sync state.
    pub fn abort(&self) -> Result<(), SyncError>;
}

#[derive(Debug)]
pub enum SyncError {
    NotGitRepository,
    NoUpstreamBranch,
    DirtyUnmanagedFiles(Vec<PathBuf>),
    SecretFound(Vec<Finding>),
    HistoricalSecretFound(Vec<HistoricalSecretFinding>),
    PushRejectedTwice,
    Git(GitError),
    ConflictAborted,
    Io(std::io::Error),
}
```

---

## 3. Conflict Resolution API (`kb_core::conflict`)

```rust
pub trait ConflictHandler {
    /// Invoked for each conflicting entry during integration.
    fn resolve(&mut self, conflict: &Conflict) -> Result<ResolutionChoice, ConflictError>;
}

#[derive(Debug)]
pub enum ConflictError {
    UserAborted,
    ValidationFailed(String),
    Io(std::io::Error),
    Git(GitError),
}

/// Applies a user's resolution choice to the working tree and git index.
pub fn apply_resolution(
    root: &Path,
    git: &impl GitRunner,
    resolution: &Resolution,
) -> Result<(), ConflictError>;
```

---

## 4. Secret Gate API (`kb_core::git::secrets`)

```rust
pub struct SecretGate;

impl SecretGate {
    /// Scans a list of entry files before commit.
    /// Returns any findings that lack `kb:allow-secret`.
    pub fn check_entries(root: &Path, files: &[PathBuf]) -> Result<Vec<Finding>, GitError>;

    /// Scans commits about to be pushed (`@{u}..HEAD`).
    /// Detects any secrets introduced across all commits in the push range.
    pub fn check_unpushed_history(
        root: &Path,
        git: &impl GitRunner,
    ) -> Result<Vec<HistoricalSecretFinding>, GitError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalSecretFinding {
    pub commit_hash: String,
    pub short_hash: String,
    pub rel_path: PathBuf,
    pub line_number: usize,
    pub finding: Finding,
}
```

---

## 5. History & Restore API (`kb_core::history`)

```rust
pub struct HistoryEngine<'a, G: GitRunner> {
    root: &'a Path,
    git: &'a G,
}

impl<'a, G: GitRunner> HistoryEngine<'a, G> {
    pub fn new(root: &'a Path, git: &'a G) -> Self;

    /// Lists commits touching the entry, following renames.
    pub fn log(&self, id: &str) -> Result<EntryHistory, HistoryError>;

    /// Returns git diff of the entry at a given revision.
    pub fn diff(&self, id: &str, rev: &str) -> Result<String, HistoryError>;

    /// Retrieves raw content of the entry at a given revision.
    pub fn show(&self, id: &str, rev: &str) -> Result<String, HistoryError>;
}

#[derive(Debug)]
pub enum HistoryError {
    EntryNotFound(String),
    RevisionNotFound(String),
    Git(GitError),
    Io(std::io::Error),
}
```

---

## 6. Auto-Commit Helper (`kb_core::git::commit`)

```rust
/// Creates an atomic, path-limited commit for modified/renamed/removed entries.
pub fn auto_commit_operation(
    root: &Path,
    git: &impl GitRunner,
    action: &str,
    id: &str,
    touched_paths: &[PathBuf],
    template: &str,
) -> Result<Option<String>, GitError>;
```
