//! `kb runbook` CLI command handler.

use std::path::Path;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use kb_core::config::Loaded;
use kb_core::entry::Entry;
use kb_core::lint::check_text;
use kb_core::search::Kb;

use crate::output::CliError;
use crate::platform::Platform;

fn io_error(e: impl std::fmt::Display) -> CliError {
    CliError::new("io", e.to_string())
}

#[derive(Subcommand, Debug, Clone)]
pub enum RunbookCommand {
    /// Create a new runbook from existing entries.
    New(RunbookNewArgs),
    /// Run a runbook in the TUI: start a run or resume one.
    Run(RunbookRunArgs),
    /// List runs, or remove finished and abandoned ones.
    Runs(RunbookRunsArgs),
    /// Mark a run abandoned without opening the TUI.
    Abandon(RunbookAbandonArgs),
}

#[derive(Args, Debug, Clone)]
pub struct RunbookRunArgs {
    /// Runbook ID or a query that matches one entry.
    pub target: String,

    /// Skip the list of active runs and start a new one.
    #[arg(long)]
    pub new: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RunbookRunsArgs {
    /// Only the runs of this runbook (ID or query).
    pub runbook: Option<String>,

    /// Include finished and abandoned runs.
    #[arg(long)]
    pub all: bool,

    /// Print JSON.
    #[arg(long)]
    pub json: bool,

    /// Delete finished and abandoned runs (never active ones).
    #[arg(long)]
    pub prune: bool,

    /// With --prune: only runs closed more than this many days ago.
    #[arg(long, value_name = "DAYS", requires = "prune")]
    pub older_than: Option<u64>,
}

#[derive(Args, Debug, Clone)]
pub struct RunbookAbandonArgs {
    /// The run's ID, as `kb runbook runs` shows it.
    pub run_id: String,

    /// Do not ask.
    #[arg(short = 'y', long)]
    pub yes: bool,
}

fn resolve_runbook(kb: &mut Kb, text: &str) -> Result<String, ExitCode> {
    match kb.resolve(text) {
        Ok(kb_core::resolve::Resolution::Resolved(id)) => match kb.entry(&id) {
            Ok(e) if e.entry_type() == Some(kb_core::entry::EntryType::Runbook) => Ok(id),
            _ => {
                eprintln!("kb: {id} is not a runbook");
                Err(ExitCode::from(1))
            }
        },
        Ok(kb_core::resolve::Resolution::Ambiguous(c)) => {
            eprintln!(
                "kb: {} close matches for {text:?}; use an exact ID",
                c.len()
            );
            for cand in &c {
                eprintln!("{}\t{}", cand.id, cand.title);
            }
            Err(ExitCode::from(1))
        }
        Ok(kb_core::resolve::Resolution::NotFound) => {
            eprintln!("kb: no entry matches {text:?}");
            Err(ExitCode::from(1))
        }
        Err(e) => {
            eprintln!("kb: {e}");
            Err(ExitCode::from(2))
        }
    }
}

pub fn run_runbook_run(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    args: RunbookRunArgs,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(io_error)?;
    let id = match resolve_runbook(&mut kb, &args.target) {
        Ok(id) => id,
        Err(code) => return Ok(code),
    };
    crate::commands::run_runbook_ui(platform, root, loaded, &id, args.new)
}

pub fn run_runbook_runs(
    platform: &impl Platform,
    root: &Path,
    args: RunbookRunsArgs,
) -> Result<ExitCode, CliError> {
    use kb_core::runbook::run::{self, RunState, fs as runfs};
    let mut kb = Kb::open(root).map_err(io_error)?;
    if args.prune {
        let removed = runfs::prune(root, args.older_than, run::now_secs()).map_err(io_error)?;
        if args.json {
            println!("{}", serde_json::json!({ "removed": removed }));
        } else {
            println!("removed {} run(s)", removed.len());
        }
        return Ok(ExitCode::SUCCESS);
    }
    let filter = match &args.runbook {
        Some(t) => match resolve_runbook(&mut kb, t) {
            Ok(id) => Some(id),
            Err(code) => return Ok(code),
        },
        None => None,
    };
    let mut rows = Vec::new();
    for r in runfs::list(root) {
        if filter.as_ref().is_some_and(|f| *f != r.runbook_id) {
            continue;
        }
        if !args.all && r.state != RunState::Active {
            continue;
        }
        let steps = kb
            .entry(&r.runbook_id)
            .ok()
            .and_then(|e| e.runbook_steps().map(<[_]>::to_vec))
            .unwrap_or_default();
        let (done, skipped, failed) = run::counts(&r, &steps);
        rows.push((
            steps.len(),
            done,
            skipped,
            failed,
            runfs::locked_by(root, &r.id, &|pid| platform.process_alive(pid)).is_some(),
            r,
        ));
    }
    if args.json {
        let doc: Vec<_> = rows
            .iter()
            .map(|(total, done, skipped, failed, locked, r)| {
                serde_json::json!({
                    "id": r.id,
                    "runbook_id": r.runbook_id,
                    "display_name": r.display_name,
                    "state": r.state,
                    "created_at": r.created_at,
                    "updated_at": r.updated_at,
                    "finished_at": r.finished_at,
                    "steps_total": total,
                    "steps_done": done,
                    "steps_skipped": skipped,
                    "steps_failed": failed,
                    "is_locked": locked,
                })
            })
            .collect();
        println!("{}", serde_json::Value::Array(doc));
        return Ok(ExitCode::SUCCESS);
    }
    let active = rows
        .iter()
        .filter(|r| r.5.state == RunState::Active)
        .count();
    println!("ACTIVE RUNS ({active})\n");
    println!("{:<40}  {:<28}  {:<52}  UPDATED", "ID", "RUNBOOK", "NAME");
    let now = run::now_secs();
    for (total, done, _, _, _, r) in &rows {
        let state = if r.state == RunState::Active {
            String::new()
        } else {
            format!(" [{:?}]", r.state).to_lowercase()
        };
        println!(
            "{:<40}  {:<28}  {:<52}  {}",
            r.id,
            r.runbook_id,
            format!(
                "{} · {done}/{total} done{state}",
                kb_core::terminal::raw(&r.display_name)
            ),
            ago(now - r.updated_at)
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn ago(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 86_400 {
        format!("{}d ago", s / 86_400)
    } else if s >= 3600 {
        format!("{}h ago", s / 3600)
    } else if s >= 60 {
        format!("{}m ago", s / 60)
    } else {
        "just now".into()
    }
}

pub fn run_runbook_abandon(
    platform: &impl Platform,
    root: &Path,
    args: RunbookAbandonArgs,
) -> Result<ExitCode, CliError> {
    use kb_core::runbook::run::{self, RunState, fs as runfs};
    let mut r = match runfs::load(root, &args.run_id) {
        Ok(r) => r,
        Err(runfs::LoadError::NotFound) => {
            eprintln!("kb: no run {:?}", args.run_id);
            return Ok(ExitCode::from(1));
        }
        Err(runfs::LoadError::Unreadable(e)) => {
            eprintln!(
                "kb: run {:?} is unreadable ({e}); a .bak copy was kept",
                args.run_id
            );
            return Ok(ExitCode::from(1));
        }
        Err(runfs::LoadError::Io(e)) => return Err(io_error(e)),
    };
    if let Some(pid) = runfs::locked_by(root, &r.id, &|pid| platform.process_alive(pid)) {
        eprintln!("kb: run {} is open in PID {pid}; close it first", r.id);
        return Ok(ExitCode::from(1));
    }
    if r.state != RunState::Active {
        eprintln!("kb: run {} is already {:?}", r.id, r.state);
        return Ok(ExitCode::from(1));
    }
    if !args.yes {
        if !platform.has_terminal() {
            eprintln!("kb: pass --yes to abandon without asking");
            return Ok(ExitCode::from(2));
        }
        eprint!("Abandon run {}? [y/N]: ", r.id);
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        if !line.trim().eq_ignore_ascii_case("y") {
            return Ok(ExitCode::from(130));
        }
    }
    run::abandon_run(&mut r, run::now_secs());
    runfs::save(root, &r).map_err(io_error)?;
    println!("abandoned {}", r.id);
    Ok(ExitCode::SUCCESS)
}

#[derive(Args, Debug, Clone)]
pub struct RunbookNewArgs {
    /// Title for the runbook.
    pub title: String,

    /// Entry IDs to include as steps.
    #[arg(required = true)]
    pub entries: Vec<String>,

    /// Where to write the entry, relative to the KB root (default: `runbook/<slug>`).
    #[arg(long)]
    pub path: Option<String>,

    /// Tags for the runbook.
    #[arg(long = "tag")]
    pub tags: Vec<String>,

    /// Skip the editor and save directly.
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Print what happened as JSON.
    #[arg(long)]
    pub json: bool,
}

pub fn run_runbook_new(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    args: RunbookNewArgs,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(io_error)?;

    // 1. Resolve entries in order
    let mut resolved_entries = Vec::new();
    for id in &args.entries {
        let entry = match kb.entry(id) {
            Ok(e) => e,
            Err(_) => {
                eprintln!("kb: entry '{id}' does not exist");
                return Ok(ExitCode::from(1));
            }
        };
        resolved_entries.push((id.as_str(), entry));
    }
    let entries_slice: Vec<(&str, &Entry)> =
        resolved_entries.iter().map(|(id, e)| (*id, e)).collect();

    // 2. Generate runbook content with variable lifting
    let content = kb_core::author::runbook_new::generate_runbook_content(
        &args.title,
        &args.tags,
        &entries_slice,
    );

    // 3. Resolve path / ID
    let slug = kb_core::author::draft::slugify(&args.title);
    let default_id = format!("runbook/{slug}");
    let path = args.path.unwrap_or(default_id);
    let target_id = path.strip_suffix(".md").unwrap_or(&path).to_string();

    if args.yes {
        let bytes = content.into_bytes();
        let findings = check_text(&target_id, bytes.clone());
        let has_errors = findings
            .iter()
            .any(|f| f.severity == kb_core::lint::Severity::Error);
        if has_errors {
            for f in &findings {
                eprintln!("{}", crate::author::finding_line(f));
            }
            return Ok(ExitCode::from(1));
        }
        for f in &findings {
            if f.severity == kb_core::lint::Severity::Warning {
                eprintln!("{}", crate::author::finding_line(f));
            }
        }
        let check = kb_core::author::render::Check {
            id: target_id.clone(),
            bytes,
            findings,
            too_large: false,
        };
        let file_path = kb.create_entry(&check).map_err(|e| match e {
            kb_core::author::fsops::FsError::Io { .. } => io_error(e),
            other => {
                eprintln!("kb: {other}");
                CliError::new("author", other.to_string())
            }
        })?;
        if args.json {
            let doc = kb
                .entry(&target_id)
                .map(|e| kb_core::entry_json::entry_json(&e, &file_path))
                .unwrap_or_else(|_| serde_json::json!({ "id": target_id }));
            println!("{doc}");
        } else {
            println!("{target_id}");
        }
        let rel = file_path
            .strip_prefix(kb.root())
            .unwrap_or(&file_path)
            .to_path_buf();
        let outcome = kb_core::git::commit::auto_commit_operation(
            kb.root(),
            &crate::platform::SystemGitRunner,
            &loaded.settings,
            kb_core::git::commit::AutoCommitAction::Add,
            &target_id,
            &[rel],
        );
        if let Ok(kb_core::git::commit::AutoCommitOutcome::SkippedMissingIdentity { .. }) = outcome
        {
            eprintln!(
                "kb: git commit skipped: author identity unknown (set user.name and user.email)"
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Editor flow
    if !platform.has_terminal() {
        return Err(CliError::new(
            "usage",
            "kb runbook new needs --yes when there is no terminal",
        ));
    }

    crate::editfile::edit_and_save_runbook(
        platform,
        &mut kb,
        &loaded.settings,
        &content,
        &target_id,
        args.json,
    )
}
