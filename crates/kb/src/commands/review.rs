//! `kb review` command for human oversight of agent modifications.

use std::path::Path;
use std::process::ExitCode;

use clap::Args;
use kb_core::config::Loaded;
use kb_core::mcp::review::ReviewStateTracker;
use serde_json::json;

use crate::output::CliError;
use crate::platform::Platform;
use crate::platform::SystemGitRunner;
use crate::varcli::{Ask, TtyAsk};

#[derive(Args, Debug, Clone, Default)]
pub struct ReviewArgs {
    /// Specific entry IDs to review (default: all unreviewed).
    #[arg(value_name = "ID")]
    pub ids: Vec<String>,

    /// Approve all unreviewed entries without interactive confirmation.
    #[arg(long)]
    pub approve_all: bool,

    /// Print unreviewed entries and diffs as JSON.
    #[arg(long)]
    pub json: bool,
}

pub fn run_review(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    args: &ReviewArgs,
) -> Result<ExitCode, CliError> {
    let unreviewed_ids = if !args.ids.is_empty() {
        args.ids.clone()
    } else {
        ReviewStateTracker::unreviewed_entries_with_git(root, &SystemGitRunner)
    };

    if args.json {
        let mut entries_json = Vec::new();
        for id in &unreviewed_ids {
            let diff = ReviewStateTracker::review_diff_with_git(root, id, &SystemGitRunner)
                .unwrap_or_default();
            entries_json.push(json!({
                "id": id,
                "unreviewed": ReviewStateTracker::is_unreviewed_with_git(root, id, &SystemGitRunner),
                "diff": diff,
            }));
        }
        println!("{}", serde_json::to_string_pretty(&entries_json).unwrap_or_default());
        return Ok(ExitCode::SUCCESS);
    }

    if unreviewed_ids.is_empty() {
        println!("No unreviewed entries.");
        return Ok(ExitCode::SUCCESS);
    }

    if args.approve_all {
        for id in &unreviewed_ids {
            ReviewStateTracker::approve_with_git(root, id, &SystemGitRunner)
                .map_err(|e| CliError::new("io", e.to_string()))?;
            println!("Approved {id}");
        }
        return Ok(ExitCode::SUCCESS);
    }

    if !platform.has_terminal() {
        return Err(CliError::new(
            "usage",
            "cannot review entries interactively without a terminal; use --approve-all or --json",
        ));
    }

    for id in &unreviewed_ids {
        println!("--- Reviewing: {id} ---");
        match ReviewStateTracker::review_diff_with_git(root, id, &SystemGitRunner) {
            Ok(diff) => {
                print_colored_diff(&diff);
            }
            Err(e) => {
                eprintln!("kb: warning: could not compute diff for {id}: {e}");
            }
        }

        let mut tty = TtyAsk::open(platform).ok_or_else(|| {
            CliError::new("io", "cannot open terminal for interactive review")
        })?;

        loop {
            let answer = tty
                .line("Review choice: [a]pprove, [r]evert, [e]dit, [s]kip (a/r/e/s)? ")
                .map_err(|e| CliError::new("io", e.to_string()))?;

            let choice = match answer {
                Some(ans) => ans.trim().to_ascii_lowercase(),
                None => {
                    println!("\nReview cancelled.");
                    return Ok(ExitCode::from(130));
                }
            };

            match choice.as_str() {
                "a" | "approve" => {
                    ReviewStateTracker::approve_with_git(root, id, &SystemGitRunner)
                        .map_err(|e| CliError::new("io", e.to_string()))?;
                    println!("Approved {id}");
                    break;
                }
                "r" | "revert" => {
                    ReviewStateTracker::revert_with_git(root, id, &SystemGitRunner)
                        .map_err(|e| CliError::new("io", e.to_string()))?;
                    println!("Reverted {id}");
                    break;
                }
                "e" | "edit" => {
                    crate::editfile::edit_entry(platform, root, loaded, id, false)?;
                    ReviewStateTracker::approve_with_git(root, id, &SystemGitRunner)
                        .map_err(|e| CliError::new("io", e.to_string()))?;
                    println!("Approved {id}");
                    break;
                }
                "s" | "skip" => {
                    println!("Skipped {id}");
                    break;
                }
                _ => {
                    eprintln!("Please enter 'a' to approve, 'r' to revert, 'e' to edit, or 's' to skip.");
                }
            }
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn print_colored_diff(diff: &str) {
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            println!("\x1b[32m{line}\x1b[0m");
        } else if line.starts_with('-') && !line.starts_with("---") {
            println!("\x1b[31m{line}\x1b[0m");
        } else if line.starts_with("@@") {
            println!("\x1b[36m{line}\x1b[0m");
        } else {
            println!("{line}");
        }
    }
}
