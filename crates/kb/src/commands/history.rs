//! `kb history` and `kb restore`: an entry's past, read from git.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use kb_core::config::Loaded;
use kb_core::git::commit::{AutoCommitAction, AutoCommitOutcome, auto_commit_operation};
use kb_core::history::{HistoryEngine, HistoryError};
use kb_core::resolve::Resolution;
use kb_core::search::Kb;
use kb_core::sync::conflict::validate_entry_text;

use crate::output::CliError;
use crate::platform::SystemGitRunner;

#[derive(Args, Debug, Clone)]
pub struct HistoryArgs {
    /// An entry ID, or words that find exactly one entry.
    pub id: String,
    /// Show the diff the entry got at this revision.
    #[arg(long, value_name = "REV")]
    pub diff: Option<String>,
    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RestoreArgs {
    /// The entry's ID.
    pub id: String,
    /// The commit (or any git revision) to take the entry's text from.
    #[arg(long, value_name = "REV")]
    pub at: String,
}

fn io(e: impl std::fmt::Display) -> CliError {
    CliError::new("io", e.to_string())
}

/// `text` as an entry ID that has history, else the one entry it finds.
fn entry_id(root: &Path, text: &str) -> Result<Option<String>, CliError> {
    let git = SystemGitRunner;
    if HistoryEngine::new(root, &git).log(text).is_ok() {
        return Ok(Some(text.to_string()));
    }
    let mut kb = Kb::open(root).map_err(io)?;
    Ok(match kb.resolve(text).map_err(io)? {
        Resolution::Resolved(id) => Some(id),
        _ => None,
    })
}

fn report(e: &HistoryError) -> ExitCode {
    eprintln!("kb: {e}");
    ExitCode::from(match e {
        HistoryError::Git(_) => 2,
        _ => 1,
    })
}

pub fn run_history(root: &Path, args: &HistoryArgs) -> Result<ExitCode, CliError> {
    let git = SystemGitRunner;
    let Some(id) = entry_id(root, &args.id)? else {
        eprintln!("kb: no single entry matches {:?}", args.id);
        return Ok(ExitCode::from(1));
    };
    let engine = HistoryEngine::new(root, &git);
    if let Some(rev) = &args.diff {
        return Ok(match engine.diff(&id, rev) {
            Ok(d) => {
                print!("{d}");
                ExitCode::SUCCESS
            }
            Err(e) => report(&e),
        });
    }
    Ok(match engine.log(&id) {
        Ok(h) if args.json => {
            println!("{}", serde_json::to_string_pretty(&h).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Ok(h) => {
            for c in &h.commits {
                let cats: Vec<String> = c
                    .change_categories
                    .iter()
                    .map(|k| format!("{k:?}").to_lowercase())
                    .collect();
                println!(
                    "{}  {}  {}  {}  [{}]",
                    c.short_hash,
                    c.date.get(..10).unwrap_or(&c.date),
                    c.author,
                    c.message,
                    cats.join(", ")
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => report(&e),
    })
}

/// Writes the entry as it was at `rev` and commits that. Returns the short revision and any commit warning. The text
/// goes through lint and the secret check first; nothing changes if it fails either.
pub fn restore_entry(
    root: &Path,
    settings: &kb_core::config::Settings,
    id: &str,
    rev: &str,
) -> Result<(String, Option<String>), String> {
    let git = SystemGitRunner;
    let engine = HistoryEngine::new(root, &git);
    let text = engine.show(id, rev).map_err(|e| e.to_string())?;
    if let Some(problems) = validate_entry_text(id, &text) {
        return Err(format!(
            "that revision does not pass lint or the secret check:\n{problems}"
        ));
    }
    let mut kb = Kb::open(root).map_err(|e| e.to_string())?;
    kb.restore(id, text.as_bytes()).map_err(|e| e.to_string())?;
    let short = engine.short(rev);
    // the commit message wants "from <rev>", so the rev rides along with the ID
    let label = format!("{id} from {short}");
    let rel = PathBuf::from(format!("{id}.md"));
    let warning = match auto_commit_operation(
        root,
        &git,
        settings,
        AutoCommitAction::Restore,
        &label,
        &[rel],
    ) {
        Ok(AutoCommitOutcome::SkippedMissingIdentity { .. }) => Some(
            "git commit skipped: author identity unknown (set user.name and user.email)"
                .to_string(),
        ),
        Ok(_) => None,
        Err(e) => Some(format!("restored, but the commit failed: {e}")),
    };
    Ok((short, warning))
}

pub fn run_restore(root: &Path, loaded: &Loaded, args: &RestoreArgs) -> Result<ExitCode, CliError> {
    Ok(
        match restore_entry(root, &loaded.settings, &args.id, &args.at) {
            Ok((short, warning)) => {
                if let Some(w) = warning {
                    eprintln!("kb: {w}");
                }
                println!("{} restored from {short}", args.id);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("kb: {e}");
                ExitCode::from(1)
            }
        },
    )
}
