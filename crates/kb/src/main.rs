//! `kb`: a terminal knowledge base for operational snippets.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use kb_core::root::{RootInputs, resolve_root};

mod output;
mod platform;

use output::CliError;
use platform::Platform;

/// A terminal knowledge base for operational snippets.
#[derive(Parser)]
#[command(name = "kb", version, about)]
struct Cli {
    /// The KB root (default: $KB_ROOT, then `root` in the config file, then ~/kb).
    #[arg(long, global = true, value_name = "PATH")]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a KB, or add what is missing to an existing one.
    Init(InitArgs),
    /// Check entries and report problems.
    Lint(LintArgs),
}

#[derive(Args)]
struct InitArgs {
    /// Directory to initialize (default: the KB root).
    path: Option<PathBuf>,
    /// Don't create the example entries.
    #[arg(long)]
    no_examples: bool,
    /// Print a JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct LintArgs {
    /// Files or directories to check (default: the whole KB).
    paths: Vec<PathBuf>,
    /// Fail on warnings too.
    #[arg(long)]
    strict: bool,
    /// Print only the summary line.
    #[arg(long)]
    quiet: bool,
    /// Print a JSON report.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = match &cli.command {
        Command::Init(a) => a.json,
        Command::Lint(a) => a.json,
    };
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            output::print_error(&e, json);
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, CliError> {
    let platform = platform::current();
    let inputs = RootInputs {
        flag: cli.root,
        env: std::env::var_os("KB_ROOT"),
        config_file: platform.config_file(),
        home: platform.home_dir(),
    };
    let root = resolve_root(&inputs).map_err(|e| CliError::new("config", e.to_string()))?;
    match cli.command {
        Command::Init(_) => Err(CliError::new(
            "usage",
            format!("init is not implemented yet ({})", root.display()),
        )),
        Command::Lint(_) => Err(CliError::new("usage", "lint is not implemented yet")),
    }
}
