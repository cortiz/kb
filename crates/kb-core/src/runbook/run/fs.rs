//! Run persistence: atomic JSON files, advisory locks, listing and pruning.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::state::{Run, RunLock, RunState};

pub fn runs_dir(root: &Path) -> PathBuf {
    root.join(".kb").join("runs")
}

fn run_path(root: &Path, id: &str) -> PathBuf {
    runs_dir(root).join(format!("{id}.json"))
}

fn lock_path(root: &Path, id: &str) -> PathBuf {
    runs_dir(root).join(format!("{id}.lock"))
}

/// Creates `.kb/runs/` with a `.gitignore` that ignores everything in it.
pub fn init_dir(root: &Path) -> io::Result<()> {
    let dir = runs_dir(root);
    fs::create_dir_all(&dir)?;
    let gi = dir.join(".gitignore");
    if !gi.exists() {
        fs::write(gi, "*\n")?;
    }
    Ok(())
}

/// Writes the run atomically: temp file in the same directory, then rename.
pub fn save(root: &Path, run: &Run) -> io::Result<()> {
    init_dir(root)?;
    let tmp = runs_dir(root).join(format!(".{}.tmp", run.id));
    let json = serde_json::to_vec_pretty(run).map_err(io::Error::other)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, run_path(root, &run.id))
}

#[derive(Debug)]
pub enum LoadError {
    NotFound,
    /// The file could not be parsed; a `.bak` copy was kept.
    Unreadable(String),
    Io(io::Error),
}

pub fn load(root: &Path, id: &str) -> Result<Run, LoadError> {
    let path = run_path(root, id);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(LoadError::NotFound),
        Err(e) => return Err(LoadError::Io(e)),
    };
    serde_json::from_slice::<Run>(&bytes).map_err(|e| {
        let _ = fs::copy(&path, path.with_extension("json.bak"));
        LoadError::Unreadable(e.to_string())
    })
}

/// Ids of all run files, sorted.
pub fn ids(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(runs_dir(root))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            name.strip_suffix(".json").map(str::to_string)
        })
        .filter(|n| !n.starts_with('.'))
        .collect();
    out.sort();
    out
}

/// Every readable run (unreadable files are skipped), oldest first.
pub fn list(root: &Path) -> Vec<Run> {
    let mut runs: Vec<Run> = ids(root)
        .iter()
        .filter_map(|id| load(root, id).ok())
        .collect();
    runs.sort_by_key(|r| (r.created_at, r.id.clone()));
    runs
}

/// Deletes finished and abandoned runs (never active ones). With `older_than_days`, only those
/// closed more than that many days before `now`. Returns the removed ids.
pub fn prune(root: &Path, older_than_days: Option<u64>, now: i64) -> io::Result<Vec<String>> {
    let mut removed = Vec::new();
    for run in list(root) {
        if run.state == RunState::Active {
            continue;
        }
        if let Some(days) = older_than_days {
            let closed = run.finished_at.unwrap_or(run.updated_at);
            if now - closed <= days as i64 * 86_400 {
                continue;
            }
        }
        fs::remove_file(run_path(root, &run.id))?;
        let _ = fs::remove_file(lock_path(root, &run.id));
        removed.push(run.id);
    }
    Ok(removed)
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".into())
}

/// The lock is held by another living process.
#[derive(Debug, PartialEq, Eq)]
pub struct Locked {
    pub pid: u32,
}

/// Held while a process may write a run. Dropping it removes the lock file.
#[derive(Debug)]
pub struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// The pid holding a live lock on the run, if any (stale locks do not count). `alive` asks the
/// platform whether a process exists; the OS call is not core's to make.
pub fn locked_by(root: &Path, id: &str, alive: &dyn Fn(u32) -> bool) -> Option<u32> {
    let lock: RunLock = serde_json::from_slice(&fs::read(lock_path(root, id)).ok()?).ok()?;
    (lock.pid != std::process::id() && alive(lock.pid)).then_some(lock.pid)
}

/// Takes the advisory lock, reclaiming one left by a dead process.
pub fn acquire(
    root: &Path,
    id: &str,
    now: i64,
    alive: &dyn Fn(u32) -> bool,
) -> Result<LockGuard, Locked> {
    let _ = init_dir(root);
    if let Some(pid) = locked_by(root, id, alive) {
        return Err(Locked { pid });
    }
    let path = lock_path(root, id);
    let lock = RunLock {
        pid: std::process::id(),
        started_at: now,
        hostname: hostname(),
    };
    let _ = fs::write(&path, serde_json::to_vec(&lock).unwrap_or_default());
    Ok(LockGuard { path })
}
