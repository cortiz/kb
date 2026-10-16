//! `kb`: a terminal knowledge base for operational snippets.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand};
use kb_core::config::{self, Flags, Loaded};
use kb_core::entry_json::PrintMode;
use kb_core::init::{apply_init, plan_init};
use kb_core::lint::{LintError, lint, resolve_scope};
use kb_core::query::Query;
use kb_core::root::{RootInputs, resolve_root};
use kb_core::search::Kb;

use kb_cli::commands::pick::PickArgs;
use kb_cli::commands::{self, CodeCommand, ShowMode, ShowOptions};
use kb_cli::output::CliError;
use kb_cli::platform::{self, Platform};
use kb_cli::varcli::VarArgs;
use kb_cli::{color, output};

/// A terminal knowledge base for operational snippets.
#[derive(Parser)]
#[command(name = "kb", version, about)]
struct Cli {
    /// The KB root (default: $KB_ROOT, then `root` in the config file, then ~/kb).
    #[arg(long, global = true, value_name = "PATH")]
    root: Option<PathBuf>,

    /// The UI keymap for this run: emacs or vim (default: `tui.keymap`, then emacs).
    #[arg(long, global = true, value_name = "NAME", value_parser = ["emacs", "vim"])]
    keymap: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Default)]
struct TuiArgs {
    /// Initial text for the search line.
    query: Vec<String>,
    /// Print the pick instead of copying it: payload, id, path or json. Without this, Enter
    /// follows `tui.enter` (copy by default).
    #[arg(long, value_name = "MODE", value_parser = parse_print)]
    print: Option<PrintMode>,
    /// `-tag:x` on the command line arrives here (see `rewrite_not_tag`).
    #[arg(long = "not-tag", hide = true, value_name = "TAG")]
    not_tags: Vec<String>,
}

impl TuiArgs {
    /// The text for the search line: the words, then each excluded tag as `-tag:x`.
    fn initial_query(&self) -> String {
        join_query(&self.query, &self.not_tags)
    }
}

fn parse_print(s: &str) -> Result<PrintMode, String> {
    PrintMode::parse(s).ok_or_else(|| format!("expected one of {}", PrintMode::NAMES.join(", ")))
}

#[derive(Args)]
struct CopyArgs {
    /// An entry ID, or words to search for. A clear best match is copied; close matches open the
    /// UI (or are listed when there is no terminal).
    #[arg(required = true, num_args = 1..)]
    query: Vec<String>,
    /// How to copy, for this run: auto, native, osc52 or stdout.
    #[arg(long, value_name = "METHOD", value_parser = ["auto", "native", "osc52", "stdout"])]
    method: Option<String>,
    /// Print what happened as JSON.
    #[arg(long)]
    json: bool,
    /// Copy the payload as it is in the file, `{{variables}}` included.
    #[arg(long, conflicts_with_all = ["vars", "yes", "no_remember"])]
    raw: bool,
    #[command(flatten)]
    vars: VarArgs,
    /// `-tag:x` on the command line arrives here (see `rewrite_not_tag`).
    #[arg(long = "not-tag", hide = true, value_name = "TAG")]
    not_tags: Vec<String>,
}

#[derive(Args)]
struct OpenArgs {
    /// An entry ID, or words to search for.
    #[arg(required = true, num_args = 1..)]
    query: Vec<String>,
    /// Override default open target: web, vscode, idea, console.
    #[arg(long = "with", value_name = "TARGET")]
    with: Option<String>,
    #[command(flatten)]
    vars: VarArgs,
    /// `-tag:x` on the command line arrives here (see `rewrite_not_tag`).
    #[arg(long = "not-tag", hide = true, value_name = "TAG")]
    not_tags: Vec<String>,
}

#[derive(Args)]
struct RunArgs {
    /// An entry ID, or words to search for.
    #[arg(required = true, num_args = 1..)]
    query: Vec<String>,
    /// Force execution of destructive commands with --yes.
    #[arg(long)]
    force: bool,
    /// Run a custom action instead of the entry's command.
    #[arg(long, value_name = "NAME")]
    action: Option<String>,
    #[command(flatten)]
    vars: VarArgs,
    /// `-tag:x` on the command line arrives here (see `rewrite_not_tag`).
    #[arg(long = "not-tag", hide = true, value_name = "TAG")]
    not_tags: Vec<String>,
}

#[derive(Args)]
struct ActionsArgs {
    /// An entry ID, or words to search for.
    #[arg(required = true, num_args = 1..)]
    query: Vec<String>,
    /// Print the actions as JSON.
    #[arg(long)]
    json: bool,
    /// `-tag:x` on the command line arrives here (see `rewrite_not_tag`).
    #[arg(long = "not-tag", hide = true, value_name = "TAG")]
    not_tags: Vec<String>,
}

#[derive(Args)]
struct UseArgs {
    /// An entry ID, or words to search for.
    #[arg(required = true, num_args = 1..)]
    query: Vec<String>,
    /// How to copy, for this run: auto, native, osc52 or stdout.
    #[arg(long, value_name = "METHOD", value_parser = ["auto", "native", "osc52", "stdout"])]
    method: Option<String>,
    /// Print what happened as JSON.
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    vars: VarArgs,
    /// `-tag:x` on the command line arrives here (see `rewrite_not_tag`).
    #[arg(long = "not-tag", hide = true, value_name = "TAG")]
    not_tags: Vec<String>,
}

#[derive(Args)]
struct VarsArgs {
    /// An entry ID, or words to search for.
    #[arg(required = true, num_args = 1..)]
    query: Vec<String>,
    /// Print the variables as JSON.
    #[arg(long)]
    json: bool,
    /// `-tag:x` on the command line arrives here (see `rewrite_not_tag`).
    #[arg(long = "not-tag", hide = true, value_name = "TAG")]
    not_tags: Vec<String>,
}

#[derive(Args)]
struct ShowArgs {
    /// An entry ID, or words to search for.
    #[arg(required = true, num_args = 1..)]
    query: Vec<String>,
    /// Print only the payload (or the URL, or `path:line`).
    #[arg(long, conflicts_with_all = ["json", "file", "checklist"])]
    raw: bool,
    /// Print the entry as JSON.
    #[arg(long, conflicts_with_all = ["raw", "file", "checklist"])]
    json: bool,
    /// Print the absolute path of the entry's file.
    #[arg(long, conflicts_with_all = ["raw", "json", "checklist"])]
    file: bool,
    /// Format runbook as a Markdown checklist.
    #[arg(long, conflicts_with_all = ["raw", "json", "file"])]
    checklist: bool,
    /// Resolve variables in runbook checklist.
    #[arg(long, requires = "checklist")]
    resolve: bool,
    /// Give a variable's value: `--var env=prod`. Repeatable.
    #[arg(long = "var", value_name = "NAME=VALUE")]
    vars: Vec<String>,
    /// Never ask: use the pre-filled value of every variable not given with --var.
    #[arg(short = 'y', long)]
    yes: bool,
    /// No colors, even on a terminal.
    #[arg(long)]
    no_color: bool,
    /// `-tag:x` on the command line arrives here (see `rewrite_not_tag`).
    #[arg(long = "not-tag", hide = true, value_name = "TAG")]
    not_tags: Vec<String>,
}

impl ShowArgs {
    fn options(&self) -> Result<ShowOptions, CliError> {
        let mut flags = Vec::new();
        for raw in &self.vars {
            let Some((name, value)) = raw.split_once('=') else {
                return Err(CliError::new(
                    "usage",
                    format!("--var {raw:?}: expected NAME=VALUE"),
                ));
            };
            if !kb_core::vars::is_valid_name(name) {
                return Err(CliError::new(
                    "usage",
                    format!(
                        "--var {raw:?}: {name:?} is not a variable name (lowercase letters, digits and `_`)"
                    ),
                ));
            }
            flags.push((name.to_string(), value.to_string()));
        }

        let mode = match (self.checklist, self.raw, self.json, self.file) {
            (true, _, _, _) => ShowMode::Checklist {
                resolve: self.resolve,
            },
            (_, true, _, _) => ShowMode::Raw,
            (_, _, true, _) => ShowMode::Json,
            (_, _, _, true) => ShowMode::File,
            _ => ShowMode::Preview,
        };
        Ok(ShowOptions {
            mode,
            no_color: self.no_color,
            flags,
            yes: self.yes,
        })
    }
}

/// The words of a query, then each excluded tag as `-tag:x`.
fn join_query(words: &[String], not_tags: &[String]) -> String {
    let mut parts: Vec<String> = words.to_vec();
    parts.extend(not_tags.iter().map(|t| format!("-tag:{t}")));
    parts.join(" ")
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Write a commented config file with every setting at its default. Never overwrites.
    Init(JsonArgs),
    /// Show where the config file is written.
    Path(ConfigPathArgs),
    /// Print the current value of one setting.
    Get(ConfigGetArgs),
    /// Change one setting in the file, keeping its comments and order.
    Set(ConfigSetArgs),
    /// Show every setting, its value and where the value comes from.
    List(JsonArgs),
}

#[derive(Args)]
struct ConfigPathArgs {
    /// List every place that is read and which one is in use.
    #[arg(long)]
    read: bool,
    /// Print JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ConfigGetArgs {
    /// The setting, for example `clipboard.method`.
    key: String,
    /// Print JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ConfigSetArgs {
    /// The setting, for example `clipboard.method`.
    key: String,
    /// The new value.
    #[arg(allow_hyphen_values = true)]
    value: String,
    /// Print JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct DoctorArgs {
    /// Copy a test string and ask you to paste it back, to prove the copy arrives.
    #[arg(long)]
    copy_test: bool,
    /// Print the checks as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct EditArgs {
    /// An entry ID, or words that find exactly one entry.
    #[arg(required = true, num_args = 1..)]
    query: Vec<String>,
    /// Print the result as JSON (no prompts).
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct MvArgs {
    /// The entry to rename.
    id: String,
    /// Its new ID, for example `cw/new-name` (folders are created).
    new_id: String,
    /// Accepted for symmetry with `rm`; renaming never asks.
    #[arg(short = 'y', long)]
    yes: bool,
    /// Print what happened as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct RmArgs {
    /// The entry to delete.
    id: String,
    /// Do not ask for confirmation.
    #[arg(short = 'y', long)]
    yes: bool,
    /// Force deletion even if referenced by runbooks.
    #[arg(short = 'f', long)]
    force: bool,
    /// Print what happened as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct AddArgs {
    /// The title (otherwise a leading comment, a link's host, or you are asked).
    #[arg(short, long)]
    title: Option<String>,
    /// The title, as a word instead of `--title`.
    #[arg(value_name = "TITLE", conflicts_with = "title")]
    title_word: Option<String>,
    /// Keep AI agents away from the new entry (adds the `private` tag).
    #[arg(long)]
    private: bool,
    /// sql, sumo, cw, cmd, prompt, link, code or note (otherwise detected).
    #[arg(long = "type", value_name = "TYPE")]
    ty: Option<String>,
    /// A tag (repeatable, or comma-separated).
    #[arg(long = "tag", value_name = "TAG")]
    tags: Vec<String>,
    /// The new entry's ID, for example `sql/active-connections` (folders are created).
    #[arg(long, value_name = "ID")]
    path: Option<String>,
    /// Take the body from the clipboard.
    #[arg(long, conflicts_with_all = ["stdin", "empty"])]
    clipboard: bool,
    /// Take the body from stdin.
    #[arg(long, conflicts_with_all = ["clipboard", "empty"])]
    stdin: bool,
    /// Start with no body.
    #[arg(long, conflicts_with_all = ["clipboard", "stdin"])]
    empty: bool,
    /// Write the entry in your editor, from a template, instead of using the form.
    #[arg(long, conflicts_with = "yes")]
    editor: bool,
    /// Never ask: save the entry or fail.
    #[arg(short = 'y', long)]
    yes: bool,
    /// Print the new entry as `kb show --json` does.
    #[arg(long)]
    json: bool,
}

impl AddArgs {
    fn into_run(self) -> kb_cli::author::AddArgs {
        use kb_cli::author::SourceFlag;
        let source = match (self.clipboard, self.stdin, self.empty) {
            (true, _, _) => SourceFlag::Clipboard,
            (_, true, _) => SourceFlag::Stdin,
            (_, _, true) => SourceFlag::Empty,
            _ => SourceFlag::Auto,
        };
        kb_cli::author::AddArgs {
            title: self.title.or(self.title_word),
            ty: self.ty,
            tags: commands::note::with_private(self.tags, self.private),
            path: self.path,
            source,
            editor: self.editor,
            yes: self.yes,
            json: self.json,
        }
    }
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
    /// List available actions for an entry.
    Actions(ActionsArgs),
    /// Copy an entry's payload to the clipboard, asking for its variables first.
    Copy(CopyArgs),
    /// Open an entry's primary link, console deep link, or code target.
    Open(OpenArgs),
    /// Run a command safely with confirmation and secret isolation.
    Run(RunArgs),
    /// Fill in an entry's variables and copy (or print) the result.
    Use(UseArgs),
    /// List an entry's variables and the values that would be pre-filled.
    Vars(VarsArgs),
    /// Print an entry: the preview as text, or only the payload, JSON or file path.
    Show(ShowArgs),
    /// Open an entry's file in $VISUAL / $EDITOR.
    Edit(EditArgs),
    /// Add an entry from the clipboard, stdin or an editor template.
    Add(AddArgs),
    /// Write a Markdown note in your editor (or from stdin with --yes).
    Note(commands::note::NoteArgs),
    /// Rename an entry, keeping its usage and remembered values.
    Mv(MvArgs),
    /// Delete an entry (asks first).
    Rm(RmArgs),
    /// Create, inspect and change the config file.
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Show the key bindings in effect, check them, or print the defaults.
    Keys(commands::keys::KeysArgs),
    /// Check the machine: root, index, config, clipboard, editor and opener, with fixes.
    Doctor(DoctorArgs),
    /// Check, re-pin or inspect code links.
    #[command(subcommand)]
    Code(CodeCommand),
    /// Pick an entry interactively or from scripts.
    Pick(PickArgs),
    /// Setup, install, or inspect shell integration.
    #[command(subcommand)]
    Shell(commands::shell::ShellCommand),
    /// Print shell completions.
    Completions(commands::shell::CompletionsArgs),
    /// Hidden dynamic completion helper for shell completion scripts.
    #[command(name = "__complete", hide = true)]
    Complete(commands::complete::CompleteArgs),
    /// Run or configure the Model Context Protocol stdio server for AI agents.
    Mcp(commands::mcp::McpArgs),
    /// Review and approve or revert agent-made changes.
    Review(commands::review::ReviewArgs),
    /// Sync the KB with its git remote in one command.
    Sync(commands::sync::SyncArgs),
    /// Clone an existing KB from a git remote and index it.
    Clone(commands::clone::CloneArgs),
    /// List the commits that changed an entry, or show the diff of one.
    History(commands::history::HistoryArgs),
    /// Put an entry back to how it was at an earlier commit.
    Restore(commands::history::RestoreArgs),
    /// Manage git hooks that keep secrets out of commits.
    #[command(subcommand)]
    Git(commands::hooks::GitCommand),
    /// Manage and create runbooks.
    #[command(subcommand)]
    Runbook(commands::runbook::RunbookCommand),
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
    /// Also put the KB under git: init, ignore `.kb/`, first commit.
    #[arg(long)]
    git: bool,
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
    /// Check the files staged in git (used by the pre-commit hook).
    #[arg(long)]
    staged: bool,
    /// Run only the secret scan, on the staged files or the given paths.
    #[arg(long)]
    secrets_only: bool,
    /// Offer to fix broken references and auto-fixable findings.
    #[arg(long)]
    fix: bool,
    /// Apply fixes without confirmation.
    #[arg(short = 'y', long)]
    yes: bool,
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
        if a == "--root" || a == "--keymap" {
            i += 2;
        } else if a.starts_with("--root=") || a.starts_with("--keymap=") {
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
        Command::Actions(a) => a.json,
        Command::Copy(a) => a.json,
        Command::Open(_) => false,
        Command::Run(_) => false,
        Command::Use(a) => a.json,
        Command::Vars(a) => a.json,
        Command::Show(a) => a.json,
        Command::Edit(a) => a.json,
        Command::Add(a) => a.json,
        Command::Note(a) => a.json,
        Command::Mv(a) => a.json,
        Command::Rm(a) => a.json,
        Command::Doctor(a) => a.json,
        Command::Keys(a) => a.json(),
        Command::Config(c) => match c {
            ConfigCommand::Init(a) | ConfigCommand::List(a) => a.json,
            ConfigCommand::Path(a) => a.json,
            ConfigCommand::Get(a) => a.json,
            ConfigCommand::Set(a) => a.json,
        },
        Command::Code(c) => match c {
            CodeCommand::Check(a) => a.json,
            CodeCommand::Repin(_) => false,
            CodeCommand::Show(a) => a.json,
        },
        Command::Pick(a) => a.widget,
        Command::Shell(_) => false,
        Command::Completions(_) => false,
        Command::Complete(_) => false,
        Command::Mcp(_) => false,
        Command::Review(a) => a.json,
        Command::Git(_) => false,
        Command::History(a) => a.json,
        Command::Restore(_) => false,
        Command::Clone(_) => false,
        Command::Sync(a) => a.json(),
        Command::Runbook(commands::runbook::RunbookCommand::New(a)) => a.json,
        Command::Runbook(commands::runbook::RunbookCommand::Runs(a)) => a.json,
        Command::Runbook(_) => false,
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
    let flags = Flags {
        root: cli.root.as_ref().map(|p| p.to_string_lossy().into_owned()),
        keymap: cli.keymap.clone(),
        clipboard_method: match &cli.command {
            Command::Copy(a) => a.method.clone(),
            Command::Use(a) => a.method.clone(),
            _ => None,
        },
    };
    let loaded = config::load(
        &platform.config_files(),
        &|name| std::env::var(name).ok(),
        &flags,
    );
    // `--root`, `KB_ROOT` and the file's `root` are already merged in the settings
    let inputs = RootInputs {
        config_root: loaded.settings.root().map(str::to_string),
        home: platform.home_dir(),
        ..RootInputs::default()
    };
    let root = resolve_root(&inputs).map_err(|e| CliError::new("config", e.to_string()))?;
    if !matches!(cli.command, Command::Tui(_) | Command::Pick(_)) {
        print_config_warnings(&loaded);
        // an interrupted sync (FR-017) is flagged by every command except the ones that end it
        if !matches!(
            cli.command,
            Command::Sync(_) | Command::Completions(_) | Command::Complete(_)
        ) && root.join(".kb/sync-state.json").exists()
        {
            eprintln!("kb: a sync is in progress; run `kb sync --continue` or `kb sync --abort`");
        }
    }
    match cli.command {
        Command::Init(args) => run_init(&root, platform.config_write_path().as_deref(), &args),
        Command::Lint(args) => run_lint(&root, &args, &loaded.settings),
        Command::Search(args) => run_search(&root, &args),
        Command::Reindex(args) => run_reindex(&root, &args),
        Command::Index(IndexCommand::Status(args)) => run_index_status(&root, &args),
        Command::Actions(args) => commands::run_actions(
            &platform,
            &root,
            &loaded,
            &join_query(&args.query, &args.not_tags),
            args.json,
        ),
        Command::Copy(args) => {
            let query = join_query(&args.query, &args.not_tags);
            if args.raw {
                commands::run_copy_raw(&platform, &root, &loaded, &query, args.json)
            } else {
                let opts = args.vars.into_opts(args.json)?;
                commands::run_use(&platform, &root, &loaded, &query, &opts)
            }
        }
        Command::Open(args) => {
            let query = join_query(&args.query, &args.not_tags);
            let opts = kb_cli::commands::OpenOpts {
                with_target: args.with,
                use_opts: args.vars.into_opts(false)?,
            };
            commands::run_open(&platform, &root, &loaded, &query, &opts)
        }
        Command::Run(args) => {
            let query = join_query(&args.query, &args.not_tags);
            let use_opts = args.vars.into_opts(false)?;
            let yes = use_opts.yes;
            let opts = kb_cli::commands::RunOpts {
                yes,
                force: args.force,
                action: args.action,
                use_opts,
            };
            commands::run_run(&platform, &root, &loaded, &query, &opts)
        }
        Command::Use(args) => {
            let query = join_query(&args.query, &args.not_tags);
            let opts = args.vars.into_opts(args.json)?;
            commands::run_use(&platform, &root, &loaded, &query, &opts)
        }
        Command::Vars(args) => commands::run_vars(
            &platform,
            &root,
            &join_query(&args.query, &args.not_tags),
            args.json,
        ),
        Command::Show(args) => commands::run_show(
            &platform,
            &root,
            &loaded,
            &join_query(&args.query, &args.not_tags),
            args.options()?,
        ),
        Command::Edit(args) => kb_cli::editfile::edit_entry(
            &platform,
            &root,
            &loaded,
            &args.query.join(" "),
            args.json,
        ),
        Command::Mv(args) => {
            kb_cli::author::run_mv(&platform, &root, &loaded, &args.id, &args.new_id, args.json)
        }
        Command::Rm(args) => kb_cli::author::run_rm(
            &platform, &root, &loaded, &args.id, args.yes, args.force, args.json,
        ),
        Command::Add(args) => kb_cli::author::run_add(&platform, &root, &loaded, args.into_run()),
        Command::Note(args) => {
            use std::io::IsTerminal;
            let add = args.into_add(std::io::stdin().is_terminal());
            kb_cli::author::run_add(&platform, &root, &loaded, add)
        }
        Command::Doctor(a) => commands::run_doctor(&platform, &root, &loaded, a.copy_test, a.json),
        Command::Config(c) => match c {
            ConfigCommand::Init(a) => commands::config_init(&platform, a.json),
            ConfigCommand::Path(a) => commands::config_path(&platform, &loaded, a.read, a.json),
            ConfigCommand::Get(a) => commands::config_get(&loaded, &a.key, a.json),
            ConfigCommand::Set(a) => {
                commands::config_set(&platform, &loaded, &a.key, &a.value, a.json)
            }
            ConfigCommand::List(a) => commands::config_list(&loaded, a.json),
        },
        Command::Keys(args) => commands::keys::run_keys(&loaded, &args),
        Command::Code(args) => commands::run_code(&platform, &root, &loaded, &args),
        Command::Pick(args) => commands::pick::run_pick(&platform, &root, &loaded, &args),
        Command::Shell(args) => commands::shell::run_shell(&platform, &loaded, &args),
        Command::Completions(args) => {
            commands::shell::run_completions(&args, &mut <Cli as clap::CommandFactory>::command())
        }
        Command::Complete(args) => commands::complete::run_complete(&platform, &root, &args),
        Command::Mcp(args) => Ok(commands::mcp::run_mcp(&root, &loaded, &args)),
        Command::Review(args) => commands::review::run_review(&platform, &root, &loaded, &args),
        Command::Git(args) => commands::hooks::run_git(&root, &args),
        Command::History(args) => commands::history::run_history(&root, &args),
        Command::Restore(args) => commands::history::run_restore(&root, &loaded, &args),
        Command::Clone(args) => commands::clone::run_clone(&platform, &loaded, &args),
        Command::Sync(args) => commands::sync::run_sync(&platform, &root, &loaded, &args),
        Command::Runbook(commands::runbook::RunbookCommand::New(args)) => {
            commands::runbook::run_runbook_new(&platform, &root, &loaded, args)
        }
        Command::Runbook(commands::runbook::RunbookCommand::Run(args)) => {
            commands::runbook::run_runbook_run(&platform, &root, &loaded, args)
        }
        Command::Runbook(commands::runbook::RunbookCommand::Runs(args)) => {
            commands::runbook::run_runbook_runs(&platform, &root, args)
        }
        Command::Runbook(commands::runbook::RunbookCommand::Abandon(args)) => {
            commands::runbook::run_runbook_abandon(&platform, &root, args)
        }
        Command::Tui(args) => run_tui(&platform, &root, &args, &loaded),
    }
}

/// Each config problem is reported once, on stderr. The UI defers this until it has left the
/// screen.
fn print_config_warnings(loaded: &Loaded) {
    for warning in &loaded.warnings {
        eprintln!("kb: {warning}");
    }
}

/// What to tell the user when `kb init` made a KB somewhere other than where the root resolves:
/// how to point kb at it. `None` when the new KB is where kb already looks.
fn init_hint(
    resolved_root: &std::path::Path,
    created: &std::path::Path,
    config_file: Option<&std::path::Path>,
) -> Option<String> {
    if resolved_root == created {
        return None;
    }
    let shown = created.display();
    let config = config_file
        .map(|c| {
            format!(
                " or run `kb config set root {shown}` (writes {})",
                c.display()
            )
        })
        .unwrap_or_default();
    Some(format!(
        "kb uses {} by default; to use this KB, set KB_ROOT={shown}{config}",
        resolved_root.display()
    ))
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
    if args.git
        && let Err(e) =
            kb_core::sync::init_repo(&report.root, &kb_cli::platform::SystemGitRunner, None)
    {
        return Err(CliError::new("io", format!("git setup failed: {e}")));
    }
    let default_root = resolved_root
        .canonicalize()
        .or_else(|_| std::path::absolute(resolved_root))
        .unwrap_or_else(|_| resolved_root.to_path_buf());
    let hint = init_hint(&default_root, &report.root, config_file);
    if args.json {
        output::print_init_json(&report, hint.as_deref());
    } else {
        output::print_init(&report, hint.as_deref());
    }
    Ok(ExitCode::SUCCESS)
}

/// The secret scan alone, over the staged files or the given paths: 1 when anything is blocked.
fn run_secrets_only(root: &std::path::Path, args: &LintArgs) -> Result<ExitCode, CliError> {
    use kb_core::git::{GitRunner, secrets::SecretGate};
    let files: Vec<PathBuf> = if args.staged {
        let out = kb_cli::platform::SystemGitRunner
            .run_git(
                root,
                &["diff", "--cached", "--name-only", "--diff-filter=ACMR"],
            )
            .map_err(|e| CliError::new("io", e.to_string()))?;
        out.lines()
            .filter(|l| l.ends_with(".md") && !l.starts_with(".kb/"))
            .map(PathBuf::from)
            .collect()
    } else {
        args.paths.clone()
    };
    let report =
        SecretGate::check_entries(root, &files).map_err(|e| CliError::new("io", e.to_string()))?;
    for f in &report.blocked {
        eprintln!("{}:{}: {} {}", f.path, f.line, f.code.slug(), f.message);
    }
    Ok(ExitCode::from(u8::from(!report.is_clean())))
}

fn run_lint(
    root: &std::path::Path,
    args: &LintArgs,
    settings: &kb_core::config::Settings,
) -> Result<ExitCode, CliError> {
    if args.secrets_only || args.staged {
        return run_secrets_only(root, args);
    }
    let cwd = std::env::current_dir().map_err(|e| CliError::new("io", e.to_string()))?;
    let scope = resolve_scope(root, &cwd, &args.paths).map_err(lint_error)?;
    let mut report = lint(root, &scope).map_err(lint_error)?;

    if args.fix {
        let modified = apply_lint_fixes(root, &report, args.yes, settings)?;
        if !modified.is_empty() {
            report = lint(root, &scope).map_err(lint_error)?;
        }
    }

    if args.json {
        println!("{}", report.to_json(args.strict));
    } else {
        output::print_lint(&report, args.quiet);
    }
    Ok(ExitCode::from(
        u8::try_from(report.exit_code(args.strict)).unwrap_or(1),
    ))
}

fn apply_lint_fixes(
    root: &std::path::Path,
    report: &kb_core::lint::LintReport,
    yes: bool,
    settings: &kb_core::config::Settings,
) -> Result<Vec<PathBuf>, CliError> {
    use std::io::Write;

    let mut modified = Vec::new();
    for finding in &report.findings {
        if finding.code == kb_core::lint::RuleCode::StepMissingEntry {
            let Some(ref fix) = finding.fix else { continue };
            let Some(old_id) = finding.message.split('`').nth(1) else {
                continue;
            };
            let Some(new_id) = fix.split('`').nth(1) else {
                continue;
            };

            if !yes {
                eprint!(
                    "Update reference in {} from `{}` to `{}`? [y/N] ",
                    finding.path, old_id, new_id
                );
                let _ = std::io::stderr().flush();
                let mut line = String::new();
                if std::io::stdin().read_line(&mut line).is_err()
                    || !line.trim().eq_ignore_ascii_case("y")
                {
                    continue;
                }
            }

            let full_path = root.join(&finding.path);
            if let Ok(content) = std::fs::read_to_string(&full_path)
                && let Some(updated) =
                    kb_core::runbook::retarget_runbook_step_references(&content, old_id, new_id)
                && std::fs::write(&full_path, updated).is_ok()
            {
                modified.push(full_path);
            }
        }
    }

    if !modified.is_empty() {
        modified.sort();
        modified.dedup();
        let git = kb_cli::platform::SystemGitRunner;
        let _ = kb_core::git::commit::auto_commit_paths_with_message(
            root,
            &git,
            settings,
            "fix: update broken runbook references",
            &modified,
        );
    }

    Ok(modified)
}

fn lint_error(e: LintError) -> CliError {
    let kind = if matches!(e, LintError::Io { .. }) {
        "io"
    } else {
        "usage"
    };
    CliError::new(kind, e.to_string())
}

fn run_tui(
    platform: &impl Platform,
    root: &std::path::Path,
    args: &TuiArgs,
    loaded: &Loaded,
) -> Result<ExitCode, CliError> {
    if !platform.has_terminal() {
        let mut help = Cli::command();
        let _ = help.write_long_help(&mut std::io::stderr());
        eprintln!("\nkb: the terminal UI needs a terminal; use `kb search` in scripts");
        return Ok(ExitCode::from(2));
    }
    let post = commands::post_pick_for(args.print, &loaded.settings);
    commands::run_ui(platform, root, loaded, &args.initial_query(), &post, true)
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
    fn the_init_hint_tells_toml_users_how_to_set_the_root() {
        use std::path::Path;
        // the new KB is where the root resolves to: nothing to say
        let same = Path::new("/home/u/kb");
        assert_eq!(
            super::init_hint(same, same, Some(Path::new("/c/config.toml"))),
            None
        );
        let hint = super::init_hint(
            Path::new("/home/u/kb"),
            Path::new("/data/notes"),
            Some(Path::new("/home/u/.config/kb/config.toml")),
        )
        .unwrap();
        assert!(hint.contains("KB_ROOT=/data/notes"), "{hint}");
        assert!(
            hint.contains("kb config set root /data/notes")
                && hint.contains("/home/u/.config/kb/config.toml"),
            "{hint}"
        );
        assert!(!hint.contains("root:"), "no YAML: {hint}");
        // without a config location only the variable is offered
        let hint =
            super::init_hint(Path::new("/home/u/kb"), Path::new("/data/notes"), None).unwrap();
        assert!(
            hint.contains("KB_ROOT=/data/notes") && !hint.contains("config"),
            "{hint}"
        );
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
