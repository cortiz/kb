//! `kb`: a terminal knowledge base for operational snippets.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand};
use kb_core::config::read_config;
use kb_core::entry_json::PrintMode;
use kb_core::init::{apply_init, plan_init};
use kb_core::lint::{LintError, lint, resolve_scope};
use kb_core::query::Query;
use kb_core::root::{RootInputs, resolve_root};
use kb_core::search::Kb;

use kb_cli::output::CliError;
use kb_cli::platform::{self, Platform};
use kb_cli::tui::app::Options;
use kb_cli::tui::{self, Outcome};
use kb_cli::{color, commands, output};

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

#[derive(Args, Default)]
struct TuiArgs {
    /// Initial text for the search line.
    query: Vec<String>,
    /// What Enter prints: payload, id, path or json.
    #[arg(long, value_name = "MODE", default_value = "payload", value_parser = parse_print)]
    print: PrintMode,
    /// `-tag:x` on the command line arrives here (see `rewrite_not_tag`).
    #[arg(long = "not-tag", hide = true, value_name = "TAG")]
    not_tags: Vec<String>,
}

impl TuiArgs {
    /// The text for the search line: the words, then each excluded tag as `-tag:x`.
    fn initial_query(&self) -> String {
        let mut parts: Vec<String> = self.query.clone();
        parts.extend(self.not_tags.iter().map(|t| format!("-tag:{t}")));
        parts.join(" ")
    }
}

fn parse_print(s: &str) -> Result<PrintMode, String> {
    PrintMode::parse(s).ok_or_else(|| format!("expected one of {}", PrintMode::NAMES.join(", ")))
}

#[derive(Args)]
struct ShowArgs {
    /// The entry ID, for example `sumo/5xx-by-authorizer`.
    id: String,
    /// What to print: payload, id, path or json.
    #[arg(long, value_name = "MODE", default_value = "payload", value_parser = parse_print)]
    print: PrintMode,
    /// Same as `--print json`.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct EditArgs {
    /// The entry ID.
    id: String,
    /// Print errors as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Create a KB, or add what is missing to an existing one.
    Init(InitArgs),
    /// Check entries and report problems.
    Lint(LintArgs),
    /// Find entries.
    Search(SearchArgs),
    /// Rebuild the search index from scratch.
    Reindex(JsonArgs),
    /// Inspect the search index.
    #[command(subcommand)]
    Index(IndexCommand),
    /// Open the terminal UI (the same as running `kb` with no subcommand).
    Tui(TuiArgs),
    /// Print an entry the way picking it in the UI does.
    Show(ShowArgs),
    /// Open an entry's file in $VISUAL / $EDITOR.
    Edit(EditArgs),
}

#[derive(Subcommand)]
enum IndexCommand {
    /// Show what is indexed, how big the index is, and when it was built.
    Status(JsonArgs),
}

#[derive(Args)]
struct JsonArgs {
    /// Print JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct SearchArgs {
    /// Words, "quoted phrases" and filters (type:, tag:, -tag:, var:, repo:, in:).
    query: Vec<String>,
    /// Only this entry type (repeatable; any of them).
    #[arg(long = "type", value_name = "TYPE")]
    types: Vec<String>,
    /// Only entries with this tag (repeatable; all of them).
    #[arg(long = "tag", value_name = "TAG")]
    tags: Vec<String>,
    /// Leave out entries with this tag (repeatable).
    #[arg(long = "not-tag", value_name = "TAG")]
    not_tags: Vec<String>,
    /// Maximum number of results (0 for no limit).
    #[arg(long, default_value_t = kb_core::query::DEFAULT_LIMIT)]
    limit: usize,
    /// Print only IDs, one per line.
    #[arg(long, conflicts_with = "json")]
    ids: bool,
    /// Print a JSON document.
    #[arg(long)]
    json: bool,
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

/// `-tag:x` in a query is `--not-tag x`, but clap would take it for a flag, so it is rewritten
/// before parsing. Text after the value in the same argument (`'-tag:x foo'`) stays a query word.
fn rewrite_not_tag(args: impl Iterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let mut out = Vec::new();
    for arg in args {
        let Some(text) = arg.to_str() else {
            out.push(arg);
            continue;
        };
        let lower = text.to_ascii_lowercase();
        if !lower.starts_with("-tag:") || text.len() <= 5 {
            out.push(arg);
            continue;
        }
        let rest = &text[5..];
        let (value, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        out.push(format!("--not-tag={value}").into());
        if !tail.trim().is_empty() {
            out.push(tail.trim().into());
        }
    }
    out
}

/// `kb 5xx auth` and plain `kb` open the UI: when the first thing after the global options is
/// not a subcommand (or a help or version flag), `tui` is inserted there. This is done before
/// clap sees the arguments because clap cannot mix free words with subcommands after a global
/// option.
fn insert_tui(args: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let known: Vec<String> = Cli::command()
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .chain(["help".to_string()])
        .collect();
    let mut i = 1;
    while i < args.len() {
        let Some(a) = args[i].to_str() else {
            break;
        };
        if a == "--root" {
            i += 2;
        } else if a.starts_with("--root=") {
            i += 1;
        } else {
            break;
        }
    }
    let first = args.get(i).and_then(|a| a.to_str());
    let leave = match first {
        Some("-h" | "--help" | "-V" | "--version") => true,
        Some(a) => known.iter().any(|k| k == a),
        None => false,
    };
    if leave {
        return args;
    }
    let mut out = args;
    out.insert(i.min(out.len()), "tui".into());
    out
}

fn main() -> ExitCode {
    let cli = Cli::parse_from(insert_tui(rewrite_not_tag(std::env::args_os())));
    let json = match &cli.command {
        Command::Init(a) => a.json,
        Command::Lint(a) => a.json,
        Command::Search(a) => a.json,
        Command::Reindex(a) => a.json,
        Command::Index(IndexCommand::Status(a)) => a.json,
        Command::Show(a) => a.json,
        Command::Edit(a) => a.json,
        Command::Tui(_) => false,
    };
    // A panic has already restored the terminal and printed its message (see the hook in
    // `tui::terminal`); it ends the program with the usual error code instead of 101.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(cli))) {
        Ok(Ok(code)) => code,
        Ok(Err(e)) => {
            output::print_error(&e, json);
            ExitCode::from(2)
        }
        Err(_) => ExitCode::from(2),
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
        Command::Search(args) => run_search(&root, &args),
        Command::Reindex(args) => run_reindex(&root, &args),
        Command::Index(IndexCommand::Status(args)) => run_index_status(&root, &args),
        Command::Show(args) => run_show(&root, &args),
        Command::Edit(args) => commands::run_edit(&platform, &root, &args.id),
        Command::Tui(args) => run_tui(&platform, &root, &args),
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

fn run_show(root: &std::path::Path, args: &ShowArgs) -> Result<ExitCode, CliError> {
    use std::io::Write;
    let mode = if args.json {
        PrintMode::Json
    } else {
        args.print
    };
    let out = commands::run_show(root, &args.id, mode)?;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
    Ok(ExitCode::SUCCESS)
}

fn run_tui(
    platform: &impl Platform,
    root: &std::path::Path,
    args: &TuiArgs,
) -> Result<ExitCode, CliError> {
    use std::io::Write;
    if !platform.has_terminal() {
        let mut help = Cli::command();
        let _ = help.write_long_help(&mut std::io::stderr());
        eprintln!("\nkb: the terminal UI needs a terminal; use `kb search` in scripts");
        return Ok(ExitCode::from(2));
    }
    let config = read_config(platform.config_file().as_deref())
        .map_err(|e| CliError::new("config", e.to_string()))?;
    let options = Options {
        print: args.print,
        mouse: config.mouse,
        theme: config.theme,
        // Timing tests set this so the background refresh does not add noise.
        refresh_timer: std::env::var_os("KB_TUI_NO_REFRESH_TIMER").is_none(),
    };
    match tui::run(platform, root, &args.initial_query(), options)
        .map_err(|e| CliError::new("io", e))?
    {
        Outcome::Picked { output, warnings } => {
            for w in warnings {
                eprintln!("kb: warning: {w}");
            }
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(output.as_bytes());
            let _ = stdout.flush();
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Exit(code) => Ok(ExitCode::from(u8::try_from(code).unwrap_or(2))),
    }
}

fn kb_error(e: kb_core::search::KbError) -> CliError {
    CliError::new("io", e.to_string())
}

fn run_search(root: &std::path::Path, args: &SearchArgs) -> Result<ExitCode, CliError> {
    let raw = args.query.join(" ");
    let query = Query::from_parts(
        &raw,
        &args.types,
        &args.tags,
        &args.not_tags,
        Some(args.limit),
    )
    .map_err(|e| CliError::new("usage", e.to_string()))?;
    let mut kb = Kb::open(root).map_err(kb_error)?;
    let outcome = kb
        .search(&query, std::time::SystemTime::now())
        .map_err(kb_error)?;
    for notice in &outcome.notices {
        eprintln!("kb: {notice}");
    }
    if args.json {
        output::print_search_json(&raw, outcome.total, &outcome.results);
    } else if args.ids {
        output::print_ids(&outcome.results);
    } else {
        output::print_search(&outcome.results, color::enabled());
    }
    if outcome.results.is_empty() {
        eprintln!(
            "kb: {}",
            if outcome.kb_empty {
                "no entries in KB (run kb init)"
            } else {
                "no matches"
            }
        );
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

fn run_reindex(root: &std::path::Path, args: &JsonArgs) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(kb_error)?;
    let report = kb.reindex().map_err(kb_error)?;
    output::print_reindex(&report, &kb.status(), args.json);
    Ok(ExitCode::SUCCESS)
}

fn run_index_status(root: &std::path::Path, args: &JsonArgs) -> Result<ExitCode, CliError> {
    let kb = Kb::open(root).map_err(kb_error)?;
    let status = kb.status();
    output::print_index_status(&status, args.json);
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::{insert_tui, rewrite_not_tag};

    fn tui(args: &[&str]) -> Vec<String> {
        insert_tui(args.iter().map(Into::into).collect())
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn free_words_and_bare_kb_open_the_ui() {
        assert_eq!(tui(&["kb"]), ["kb", "tui"]);
        assert_eq!(tui(&["kb", "5xx", "auth"]), ["kb", "tui", "5xx", "auth"]);
        assert_eq!(
            tui(&["kb", "--root", "/k", "5xx"]),
            ["kb", "--root", "/k", "tui", "5xx"]
        );
        assert_eq!(tui(&["kb", "--root=/k"]), ["kb", "--root=/k", "tui"]);
        assert_eq!(
            tui(&["kb", "--print", "id", "x"]),
            ["kb", "tui", "--print", "id", "x"]
        );
        assert_eq!(tui(&["kb", "serach", "x"]), ["kb", "tui", "serach", "x"]);
    }

    #[test]
    fn subcommands_and_help_are_left_alone() {
        for a in [
            &["kb", "lint"][..],
            &["kb", "--root", "/k", "search", "x"],
            &["kb", "--root=/k", "index", "status"],
            &["kb", "tui", "x"],
            &["kb", "show", "a/b"],
            &["kb", "--help"],
            &["kb", "-V"],
            &["kb", "help", "lint"],
            &["kb", "--root", "/k", "--version"],
        ] {
            assert_eq!(tui(a), a, "{a:?}");
        }
    }

    fn run(args: &[&str]) -> Vec<String> {
        rewrite_not_tag(args.iter().map(Into::into))
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn minus_tag_works_for_the_ui_too() {
        use clap::Parser;
        for args in [
            &["kb", "latency", "-tag:stage"][..],
            &["kb", "-tag:stage"],
            &[
                "kb",
                "--root",
                "/k",
                "type:cw",
                "-tag:stage",
                "-tag:old",
                "x",
            ],
            &["kb", "tui", "x", "-tag:stage"],
        ] {
            let rewritten = insert_tui(rewrite_not_tag(args.iter().map(Into::into)));
            let cli =
                super::Cli::try_parse_from(rewritten).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            let super::Command::Tui(t) = cli.command else {
                panic!("{args:?} is not the UI");
            };
            assert!(
                t.initial_query().contains("-tag:stage"),
                "{args:?}: {}",
                t.initial_query()
            );
        }
        let cli = super::Cli::try_parse_from(insert_tui(rewrite_not_tag(
            ["kb", "type:cw", "-tag:stage", "x"].iter().map(Into::into),
        )))
        .unwrap();
        let super::Command::Tui(t) = cli.command else {
            panic!()
        };
        assert_eq!(t.initial_query(), "type:cw x -tag:stage");
    }

    #[test]
    fn minus_tag_becomes_a_flag() {
        assert_eq!(
            run(&["kb", "search", "auth", "-tag:stage", "--limit", "2"]),
            ["kb", "search", "auth", "--not-tag=stage", "--limit", "2"]
        );
        assert_eq!(
            run(&["kb", "search", "-TAG:x y"]),
            ["kb", "search", "--not-tag=x", "y"]
        );
        assert_eq!(
            run(&["kb", "search", "-tag:", "tag:x", "-t"]),
            ["kb", "search", "-tag:", "tag:x", "-t"]
        );
    }
}
