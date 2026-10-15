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
