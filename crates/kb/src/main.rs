//! `kb`: a terminal knowledge base for operational snippets.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use kb_core::init::{apply_init, plan_init};
use kb_core::lint::{LintError, lint, resolve_scope};
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
        Command::Init(args) => run_init(&root, inputs.config_file.as_deref(), &args),
        Command::Lint(args) => run_lint(&root, &args),
    }
}

fn run_init(
    resolved_root: &std::path::Path,
    config_file: Option<&std::path::Path>,
    args: &InitArgs,
) -> Result<ExitCode, CliError> {
    let path = args
        .path
        .clone()
        .unwrap_or_else(|| resolved_root.to_path_buf());
    let plan =
        plan_init(&path, !args.no_examples).map_err(|e| CliError::new("io", e.to_string()))?;
    let report = apply_init(&plan).map_err(|e| CliError::new("io", e.to_string()))?;
    let default_root = resolved_root
        .canonicalize()
        .or_else(|_| std::path::absolute(resolved_root))
        .ok();
    let hint = (default_root.as_deref() != Some(report.root.as_path())).then(|| {
        let shown = report.root.display();
        let config = config_file
            .map(|c| format!(" or add `root: {shown}` to {}", c.display()))
            .unwrap_or_default();
        format!(
            "kb uses {} by default; to use this KB, set KB_ROOT={shown}{config}",
            resolved_root.display()
        )
    });
    if args.json {
        output::print_init_json(&report, hint.as_deref());
    } else {
        output::print_init(&report, hint.as_deref());
    }
    Ok(ExitCode::SUCCESS)
}

fn run_lint(root: &std::path::Path, args: &LintArgs) -> Result<ExitCode, CliError> {
    let cwd = std::env::current_dir().map_err(|e| CliError::new("io", e.to_string()))?;
    let scope = resolve_scope(root, &cwd, &args.paths).map_err(lint_error)?;
    let report = lint(root, &scope).map_err(lint_error)?;
    if args.json {
        println!("{}", report.to_json(args.strict));
    } else {
        output::print_lint(&report, args.quiet);
    }
    Ok(ExitCode::from(
        u8::try_from(report.exit_code(args.strict)).unwrap_or(1),
    ))
}

fn lint_error(e: LintError) -> CliError {
    let kind = if matches!(e, LintError::Io { .. }) {
        "io"
    } else {
        "usage"
    };
    CliError::new(kind, e.to_string())
}
