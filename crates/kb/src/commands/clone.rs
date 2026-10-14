//! `kb clone`: bring an existing KB onto this machine.

use std::io::BufRead;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Args;
use kb_core::config::Loaded;
use kb_core::git::GitRunner;
use kb_core::search::Kb;

use crate::commands::hooks::{GitCommand, run_git};
use crate::output::CliError;
use crate::platform::{Platform, SystemGitRunner};

#[derive(Args, Debug, Clone)]
pub struct CloneArgs {
    /// The remote's URL (ssh or https).
    pub remote_url: String,
    /// Where to put the KB (default: ~/kb).
    pub path: Option<PathBuf>,
}

/// Asks `question` on the terminal; anything but yes, and no terminal, is no.
fn confirm(platform: &impl Platform, question: &str) -> bool {
    if !platform.has_terminal() {
        return false;
    }
    eprint!("{question} [y/N] ");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim().eq_ignore_ascii_case("y")
}

pub fn run_clone(
    platform: &impl Platform,
    loaded: &Loaded,
    args: &CloneArgs,
) -> Result<ExitCode, CliError> {
    let default = platform.home_dir().map(|h| h.join("kb"));
    let target = args
        .path
        .clone()
        .or_else(|| default.clone())
        .ok_or_else(|| CliError::new("usage", "no home directory; give a path"))?;
    let target = std::path::absolute(&target).map_err(|e| CliError::new("io", e.to_string()))?;
    if target.exists() && std::fs::read_dir(&target).is_ok_and(|mut d| d.next().is_some()) {
        return Err(CliError::new(
            "usage",
            format!("{} exists and is not empty", target.display()),
        ));
    }
    let parent = target.parent().map(PathBuf::from).unwrap_or_default();
    std::fs::create_dir_all(&parent).map_err(|e| CliError::new("io", e.to_string()))?;

    let target_text = target.to_string_lossy();
    let cloned = SystemGitRunner.run_git_status(
        &parent,
        &["clone", &args.remote_url, &target_text],
        Duration::from_secs(loaded.settings.sync_timeout()),
    );
    match cloned {
        Ok(r) if r.success() => {}
        Ok(r) => {
            return Err(CliError::new(
                "io",
                format!("git clone failed: {}", r.stderr.trim()),
            ));
        }
        Err(e) => return Err(CliError::new("io", e.to_string())),
    }
    if SystemGitRunner
        .run_git(&target, &["rev-parse", "--verify", "HEAD"])
        .is_err()
    {
        eprintln!(
            "kb: the remote is empty; run `kb sync setup {}` on the machine that has your entries",
            args.remote_url
        );
        return Ok(ExitCode::from(1));
    }

    let mut kb = Kb::open(&target).map_err(|e| CliError::new("io", e.to_string()))?;
    let report = kb
        .reindex()
        .map_err(|e| CliError::new("io", e.to_string()))?;
    println!(
        "kb: cloned into {} ({} entries indexed)",
        target.display(),
        report.indexed
    );

    if default.as_ref() != Some(&target) {
        if confirm(
            platform,
            &format!("Use {} as your KB root?", target.display()),
        ) {
            crate::commands::config_set(platform, loaded, "root", &target_text, false)?;
        } else {
            println!(
                "kb: to use it, run `kb config set root {}`",
                target.display()
            );
        }
    }
    if confirm(platform, "Install a pre-commit hook that blocks secrets?") {
        run_git(&target, &GitCommand::InstallHooks)?;
    } else {
        println!(
            "kb: to block secrets at commit time, run `kb --root {} git install-hooks`",
            target.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}
