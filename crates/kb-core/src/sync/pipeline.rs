//! `kb sync`: commit hand edits, fetch, integrate, push, then carry state across renames and
//! refresh the index. Every step either completes or leaves a repository `--abort` can restore.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::git::secrets::SecretGate;
use crate::git::status::GitStatus;
use crate::git::{GitError, GitRunner};
use crate::search::Kb;
use crate::usage::State;

use super::conflict::{
    ConflictError, ConflictHandler, Resolution, ResolutionChoice, apply_resolution, entry_id,
    load_conflict, parse_unmerged,
};
use super::state::{self, SyncInProgressState};
use super::{LOG_FORMAT, SyncError, SyncPlan, SyncStrategy, SyncSummary, parse_log};

#[derive(Debug, Clone, Copy)]
pub struct SyncConfig {
    pub strategy: SyncStrategy,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    pub no_push: bool,
    /// Replaces `kb: sync N entries` as the hand-edit commit message.
    pub message: Option<String>,
}

pub struct SyncEngine<'a, G: GitRunner> {
    root: &'a Path,
    git: &'a G,
    config: SyncConfig,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl<'a, G: GitRunner> SyncEngine<'a, G> {
    pub fn new(root: &'a Path, git: &'a G, config: SyncConfig) -> Self {
        Self { root, git, config }
    }

    fn git(&self, args: &[&str]) -> Result<String, GitError> {
        self.git.run_git(self.root, args)
    }

    /// Repository checks every sync starts with; returns the status and `(remote, branch)`.
    fn check(&self) -> Result<(GitStatus, String, String), SyncError> {
        let st = GitStatus::query(self.git, self.root)?;
        if !st.is_git {
            return Err(SyncError::NotGitRepository);
        }
        let upstream = st.upstream.clone().ok_or(SyncError::NoUpstreamBranch)?;
        if state::load(self.root).is_some() {
            return Err(SyncError::InProgress);
        }
        if !st.unmanaged_changes.is_empty() {
            return Err(SyncError::DirtyUnmanagedFiles(st.unmanaged_changes));
        }
        let (remote, branch) = upstream
            .split_once('/')
            .ok_or(SyncError::NoUpstreamBranch)?;
        let (remote, branch) = (remote.to_string(), branch.to_string());
        Ok((st, remote, branch))
    }

    fn log(&self, range: &str) -> Result<Vec<super::CommitSummary>, GitError> {
        Ok(parse_log(&self.git(&["log", LOG_FORMAT, range])?))
    }

    /// What a sync would do right now, from the refs as last fetched. No changes are made.
    pub fn plan(&self) -> Result<SyncPlan, SyncError> {
        let (st, ..) = self.check()?;
        Ok(SyncPlan {
            pre_sync_commit: self.git(&["rev-parse", "HEAD"])?.trim().to_string(),
            hand_edits: st.uncommitted.into_iter().map(|e| e.rel_path).collect(),
            commits_to_push: self.log("@{u}..HEAD")?,
            commits_to_integrate: self.log("HEAD..@{u}")?,
            strategy: self.config.strategy,
        })
    }

    /// `git fetch` of the upstream's remote, within `sync.timeout`.
    pub fn fetch(&self) -> Result<(), SyncError> {
        let upstream = self.git(&["rev-parse", "--abbrev-ref", "@{u}"])?;
        let remote = upstream.trim().split('/').next().unwrap_or("origin");
        let res = self
            .git
            .run_git_status(self.root, &["fetch", remote], self.config.timeout)?;
        if res.success() {
            Ok(())
        } else {
            Err(GitError::CommandFailed {
                exit_code: res.exit_code,
                stderr: res.stderr,
            }
            .into())
        }
    }

    /// A/M/D counts of `.md` files in a `git diff --name-status` over `range`.
    fn count_changes(&self, range: &[&str]) -> (usize, usize, usize) {
        let mut args = vec!["diff", "--name-status", "-M"];
        args.extend_from_slice(range);
        let out = self.git(&args).unwrap_or_default();
        let (mut a, mut m, mut d) = (0, 0, 0);
        for line in out.lines() {
            let mut f = line.split('\t');
            let (Some(code), Some(path)) = (f.next(), f.next_back()) else {
                continue;
            };
            if !path.ends_with(".md") {
                continue;
            }
            match code.chars().next() {
                Some('A') => a += 1,
                Some('D') => d += 1,
                _ => m += 1,
            }
        }
        (a, m, d)
    }

    /// Runs the whole pipeline. Conflicts go to `handler` one entry at a time.
    pub fn execute<H: ConflictHandler>(
        &self,
        opts: &SyncOptions,
        handler: &mut H,
    ) -> Result<SyncSummary, SyncError> {
        let (st, remote, branch) = self.check()?;
        let mut summary = SyncSummary::default();

        let hand_edits: Vec<PathBuf> = st.uncommitted.iter().map(|e| e.rel_path.clone()).collect();
        let report = SecretGate::check_entries(self.root, &hand_edits)?;
        if !report.is_clean() {
            return Err(SyncError::SecretFound(report.blocked));
        }
        summary.allowed_secrets = report
            .allowed
            .iter()
            .map(|a| format!("{}:{}", a.path, a.line))
            .collect();
        if !hand_edits.is_empty() {
            let paths: Vec<String> = hand_edits.iter().map(|p| p.display().to_string()).collect();
            let mut add = vec!["add", "-A", "--"];
            add.extend(paths.iter().map(String::as_str));
            self.git(&add)?;
            let default = format!("kb: sync {} entries", hand_edits.len());
            let msg = opts.message.as_deref().unwrap_or(&default);
            self.git(&["commit", "-m", msg])?;
        }

        let pre_sync_commit = self.git(&["rev-parse", "HEAD"])?.trim().to_string();
        self.fetch()?;

        if let Ok(base) = self.git(&["merge-base", "HEAD", "@{u}"]) {
            let base = base.trim();
            (
                summary.local_added,
                summary.local_modified,
                summary.local_deleted,
            ) = self.count_changes(&[&format!("{base}..HEAD")]);
            (
                summary.remote_added,
                summary.remote_modified,
                summary.remote_deleted,
            ) = self.count_changes(&[&format!("{base}..@{{u}}")]);
        }

        let state = SyncInProgressState {
            pre_sync_commit: pre_sync_commit.clone(),
            remote,
            branch,
            strategy: match self.config.strategy {
                SyncStrategy::Rebase => "rebase".into(),
                SyncStrategy::Merge => "merge".into(),
            },
            remaining_conflicts: Vec::new(),
            started_at: now(),
        };
        self.integrate(&state, handler, &mut summary)?;
        self.finish(&state, opts, handler, &mut summary)?;
        Ok(summary)
    }

    /// Resumes after an interrupted or deferred conflict resolution.
    pub fn continue_sync<H: ConflictHandler>(
        &self,
        opts: &SyncOptions,
        handler: &mut H,
    ) -> Result<SyncSummary, SyncError> {
        let state = state::load(self.root).ok_or(SyncError::NothingInProgress)?;
        let mut summary = SyncSummary::default();
        self.drive(&state, handler, &mut summary)?;
        self.finish(&state, opts, handler, &mut summary)?;
        Ok(summary)
    }

    /// Restores the repository to where it was before integration started.
    pub fn abort(&self) -> Result<(), SyncError> {
        let state = state::load(self.root).ok_or(SyncError::NothingInProgress)?;
        Ok(state::rollback(self.root, self.git, &state)?)
    }

    fn counts(&self) -> Result<(usize, usize), GitError> {
        let out = self.git(&["rev-list", "--left-right", "--count", "HEAD...@{u}"])?;
        let mut n = out.split_whitespace().map(|s| s.parse().unwrap_or(0));
        Ok((n.next().unwrap_or(0), n.next().unwrap_or(0)))
    }

    /// Brings upstream commits in: fast-forward, rebase or merge. Conflicts go to [`Self::drive`].
    fn integrate<H: ConflictHandler>(
        &self,
        state: &SyncInProgressState,
        handler: &mut H,
        summary: &mut SyncSummary,
    ) -> Result<(), SyncError> {
        let (ahead, behind) = self.counts()?;
        if behind == 0 {
            return Ok(());
        }
        if ahead == 0 {
            self.git(&["merge", "--ff-only", "@{u}"])?;
            return Ok(());
        }
        state::save(self.root, state).map_err(|e| SyncError::Io(e.to_string()))?;
        let args: &[&str] = match state.strategy() {
            SyncStrategy::Rebase => &["rebase", "@{u}"],
            SyncStrategy::Merge => &["merge", "--no-edit", "@{u}"],
        };
        let res = self
            .git
            .run_git_status(self.root, args, Duration::from_secs(120))?;
        if res.success() {
            state::clear(self.root);
            return Ok(());
        }
        let porcelain = self.git(&["status", "--porcelain"])?;
        if parse_unmerged(&porcelain, state.strategy() == SyncStrategy::Rebase).is_empty() {
            state::rollback(self.root, self.git, state)?;
            return Err(GitError::CommandFailed {
                exit_code: res.exit_code,
                stderr: res.stderr,
            }
            .into());
        }
        self.drive(state, handler, summary)
    }

    /// Resolves conflicts entry by entry until the rebase or merge completes.
    fn drive<H: ConflictHandler>(
        &self,
        state: &SyncInProgressState,
        handler: &mut H,
        summary: &mut SyncSummary,
    ) -> Result<(), SyncError> {
        let rebase = state.strategy() == SyncStrategy::Rebase;
        loop {
            let porcelain = self.git(&["status", "--porcelain"])?;
            let unmerged = parse_unmerged(&porcelain, rebase);
            if unmerged.is_empty() {
                let args: &[&str] = if rebase {
                    &["-c", "core.editor=true", "rebase", "--continue"]
                } else {
                    &["-c", "core.editor=true", "commit", "--no-edit"]
                };
                let res = self
                    .git
                    .run_git_status(self.root, args, Duration::from_secs(120))?;
                if res.success() {
                    // a rebase stops again at the next conflicting commit, with exit 0 never
                    state::clear(self.root);
                    return Ok(());
                }
                let again = self.git(&["status", "--porcelain"])?;
                if !parse_unmerged(&again, rebase).is_empty() {
                    continue; // the next replayed commit conflicted
                }
                // a resolution that left a commit empty: nothing to keep
                if rebase
                    && self
                        .git
                        .run_git_status(self.root, &["rebase", "--skip"], Duration::from_secs(120))?
                        .success()
                {
                    state::clear(self.root);
                    return Ok(());
                }
                return Err(GitError::CommandFailed {
                    exit_code: res.exit_code,
                    stderr: res.stderr,
                }
                .into());
            }
            for (rel, kind) in &unmerged {
                let conflict = load_conflict(self.root, self.git, rel, *kind, rebase);
                let choice = match handler.resolve(&conflict) {
                    Ok(c) => c,
                    Err(ConflictError::UserAborted) => {
                        state::rollback(self.root, self.git, state)?;
                        return Err(SyncError::ConflictAborted);
                    }
                    Err(ConflictError::Deferred) => {
                        let mut s = state.clone();
                        s.remaining_conflicts = unmerged.iter().map(|(p, _)| p.clone()).collect();
                        let _ = state::save(self.root, &s);
                        return Err(SyncError::ConflictsPending(s.remaining_conflicts));
                    }
                    Err(e) => return Err(SyncError::Io(e.to_string())),
                };
                self.apply(conflict, choice, handler)?;
                summary.conflicts_resolved += 1;
            }
        }
    }

    fn apply<H: ConflictHandler>(
        &self,
        conflict: super::Conflict,
        mut choice: ResolutionChoice,
        handler: &mut H,
    ) -> Result<(), SyncError> {
        loop {
            let r = Resolution {
                conflict: conflict.clone(),
                choice: choice.clone(),
            };
            match apply_resolution(self.root, self.git, &r) {
                Ok(()) => return Ok(()),
                Err(ConflictError::IdCollision(taken)) => {
                    let new_id = handler
                        .new_id(&taken)
                        .map_err(|e| SyncError::Io(e.to_string()))?;
                    choice = ResolutionChoice::KeepBoth { new_id };
                }
                Err(e) => return Err(SyncError::Io(e.to_string())),
            }
        }
    }

    /// Push (with one retry), then rename migration and index refresh.
    fn finish<H: ConflictHandler>(
        &self,
        state: &SyncInProgressState,
        opts: &SyncOptions,
        handler: &mut H,
        summary: &mut SyncSummary,
    ) -> Result<(), SyncError> {
        let result = if opts.no_push {
            Ok(())
        } else {
            self.push(state, handler, summary)
        };
        state::clear(self.root);
        result?;

        self.migrate_renames(&state.pre_sync_commit, summary);
        match Kb::open(self.root).and_then(|mut kb| kb.refresh()) {
            Ok(_) => {}
            Err(e) => summary.warnings.push(format!("index refresh failed: {e}")),
        }
        let st = GitStatus::query(self.git, self.root)?;
        summary.unmanaged_files = st.unmanaged_changes;
        summary.new_status = match (&st.upstream, st.ahead, st.behind) {
            (Some(u), 0, 0) => format!("in sync with {u}"),
            (Some(u), a, 0) => format!("ahead of {u} by {a}"),
            (Some(u), a, b) => format!("{a} ahead, {b} behind {u}"),
            (None, ..) => "no upstream".into(),
        };
        Ok(())
    }

    fn push<H: ConflictHandler>(
        &self,
        state: &SyncInProgressState,
        handler: &mut H,
        summary: &mut SyncSummary,
    ) -> Result<(), SyncError> {
        let report = SecretGate::check_unpushed_history(self.root, self.git)?;
        if !report.is_clean() {
            return Err(SyncError::HistoricalSecretFound(report.blocked));
        }
        let refspec = format!("HEAD:{}", state.branch);
        for attempt in 0..2 {
            let res = self.git.run_git_status(
                self.root,
                &["push", &state.remote, &refspec],
                self.config.timeout,
            )?;
            if res.success() {
                return Ok(());
            }
            let rejected = ["rejected", "fetch first", "non-fast-forward"]
                .iter()
                .any(|s| res.stderr.contains(s));
            if !rejected {
                return Err(GitError::CommandFailed {
                    exit_code: res.exit_code,
                    stderr: res.stderr,
                }
                .into());
            }
            if attempt == 1 {
                return Err(SyncError::PushRejectedTwice);
            }
            // the remote moved while we worked: fetch and integrate once more
            self.fetch()?;
            self.integrate(state, handler, summary)?;
        }
        Err(SyncError::PushRejectedTwice)
    }

    /// Entries renamed since `pre`: their usage and remembered values follow the new ID.
    fn migrate_renames(&self, pre: &str, summary: &mut SyncSummary) {
        let Ok(out) = self.git(&["diff", "-M", "--name-status", pre, "HEAD"]) else {
            return;
        };
        let (mut st, _) = State::load(self.root);
        let mut moved = false;
        for line in out.lines().filter(|l| l.starts_with('R')) {
            let f: Vec<&str> = line.split('\t').collect();
            if let [_, old, new] = f[..]
                && old.ends_with(".md")
                && new.ends_with(".md")
            {
                moved |= st.move_entry(&entry_id(Path::new(old)), &entry_id(Path::new(new)));
            }
        }
        if moved && let Err(e) = st.save(self.root) {
            summary
                .warnings
                .push(format!("could not save usage state: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{FakeGitRunner, GitCommandResult};

    struct NoConflicts;
    impl ConflictHandler for NoConflicts {
        fn resolve(
            &mut self,
            _: &super::super::Conflict,
        ) -> Result<ResolutionChoice, ConflictError> {
            Err(ConflictError::Deferred)
        }
    }

    fn cfg() -> SyncConfig {
        SyncConfig {
            strategy: SyncStrategy::Rebase,
            timeout: Duration::from_secs(5),
        }
    }

    /// A fake whose answers come from `(command prefix, exit code, stdout, stderr)`; unknown
    /// commands succeed with no output. `push_results` are consumed in order by `push`.
    fn fake(status: &'static str, push_stderr: Vec<&'static str>) -> FakeGitRunner {
        let pushes = std::sync::Mutex::new(push_stderr.into_iter());
        FakeGitRunner::with_handler(move |_, args| {
            let ok = |out: &str| {
                Ok(GitCommandResult {
                    exit_code: Some(0),
                    stdout: out.into(),
                    stderr: String::new(),
                })
            };
            match args {
                ["rev-parse", "--is-inside-work-tree"] => ok("true\n"),
                ["status", ..] => ok(status),
                ["rev-parse", "HEAD"] => ok("headsha\n"),
                ["rev-parse", "--abbrev-ref", _] => ok("origin/main\n"),
                ["rev-list", "--left-right", ..] => ok("1\t1\n"),
                ["push", ..] => match pushes.lock().unwrap().next() {
                    Some(stderr) if !stderr.is_empty() => Ok(GitCommandResult {
                        exit_code: Some(1),
                        stdout: String::new(),
                        stderr: stderr.into(),
                    }),
                    _ => ok(""),
                },
                _ => ok(""),
            }
        })
    }

    fn cmds(git: &FakeGitRunner) -> Vec<String> {
        git.calls().into_iter().map(|(_, a)| a.join(" ")).collect()
    }

    const CLEAN: &str = "## main...origin/main\n";

    #[test]
    fn hand_edits_become_one_commit_and_pre_sync_is_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let git = fake("## main...origin/main\n M a.md\n?? b.md\n", vec![""]);
        let eng = SyncEngine::new(dir.path(), &git, cfg());
        // integration reports both sides moved; the fake rebase "succeeds"
        eng.execute(&SyncOptions::default(), &mut NoConflicts)
            .unwrap();
        let c = cmds(&git);
        let commit = c
            .iter()
            .position(|s| s == "commit -m kb: sync 2 entries")
            .unwrap();
        let head = c.iter().position(|s| s == "rev-parse HEAD").unwrap();
        let fetch = c.iter().position(|s| s.starts_with("fetch")).unwrap();
        assert!(commit < head && head < fetch, "{c:?}");
        assert!(c.contains(&"add -A -- a.md b.md".to_string()));
    }

    #[test]
    fn custom_message_and_no_push() {
        let dir = tempfile::tempdir().unwrap();
        let git = fake("## main...origin/main\n M a.md\n", vec![]);
        let eng = SyncEngine::new(dir.path(), &git, cfg());
        let opts = SyncOptions {
            no_push: true,
            message: Some("my msg".into()),
        };
        eng.execute(&opts, &mut NoConflicts).unwrap();
        let c = cmds(&git);
        assert!(c.contains(&"commit -m my msg".to_string()));
        assert!(!c.iter().any(|s| s.starts_with("push")));
    }

    #[test]
    fn dirty_unmanaged_file_stops_before_anything_changes() {
        let dir = tempfile::tempdir().unwrap();
        let git = fake("## main...origin/main\n M README.md\n", vec![]);
        let eng = SyncEngine::new(dir.path(), &git, cfg());
        let err = eng
            .execute(&SyncOptions::default(), &mut NoConflicts)
            .unwrap_err();
        assert!(
            matches!(err, SyncError::DirtyUnmanagedFiles(v) if v == [PathBuf::from("README.md")])
        );
        assert!(
            !cmds(&git)
                .iter()
                .any(|s| s.starts_with("commit") || s.starts_with("fetch"))
        );
    }

    #[test]
    fn needs_upstream() {
        let dir = tempfile::tempdir().unwrap();
        let git = fake("## main\n", vec![]);
        let eng = SyncEngine::new(dir.path(), &git, cfg());
        assert!(matches!(eng.plan(), Err(SyncError::NoUpstreamBranch)));
    }

    #[test]
    fn rejected_push_is_retried_once() {
        let dir = tempfile::tempdir().unwrap();
        let git = fake(CLEAN, vec![" ! [rejected] main -> main (fetch first)", ""]);
        let eng = SyncEngine::new(dir.path(), &git, cfg());
        eng.execute(&SyncOptions::default(), &mut NoConflicts)
            .unwrap();
        let c = cmds(&git);
        assert_eq!(c.iter().filter(|s| s.starts_with("push")).count(), 2);
        assert_eq!(c.iter().filter(|s| s.starts_with("fetch")).count(), 2);
    }

    #[test]
    fn second_rejection_stops_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let rej = " ! [rejected] main -> main (non-fast-forward)";
        let git = fake(CLEAN, vec![rej, rej]);
        let eng = SyncEngine::new(dir.path(), &git, cfg());
        let err = eng
            .execute(&SyncOptions::default(), &mut NoConflicts)
            .unwrap_err();
        assert!(matches!(err, SyncError::PushRejectedTwice));
        assert_eq!(
            cmds(&git).iter().filter(|s| s.starts_with("push")).count(),
            2
        );
        assert!(state::load(dir.path()).is_none());
    }
}
