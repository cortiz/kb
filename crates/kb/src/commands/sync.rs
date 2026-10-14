//! `kb sync`: the one-command sync, and the prompt that resolves conflicts entry by entry.

use std::io::{BufRead, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Subcommand};
use kb_core::config::Loaded;
use kb_core::git::status::SyncStatus;
use kb_core::sync::conflict::validate_entry_text;
use kb_core::sync::{
    Conflict, ConflictError, ConflictHandler, ConflictKind, ResolutionChoice, SyncConfig,
    SyncEngine, SyncError, SyncOptions, SyncSummary,
};

use crate::output::CliError;
use crate::platform::{Platform, SystemGitRunner};

#[derive(Subcommand, Debug, Clone)]
pub enum SyncSub {
    /// Show where the KB stands against its remote, without using the network.
    Status(StatusArgs),
    /// Put the KB under git with a remote: init, ignore `.kb/`, first commit, push.
    Setup(SetupArgs),
}

#[derive(Args, Debug, Clone)]
pub struct StatusArgs {
    /// Fetch from the remote first.
    #[arg(long)]
    pub fetch: bool,
    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct SetupArgs {
    /// The remote's URL (ssh or https).
    pub remote_url: String,
    /// Branch to push and track (default: the current branch, or main).
    #[arg(short, long, value_name = "NAME")]
    pub branch: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
#[command(args_conflicts_with_subcommands = true)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub command: Option<SyncSub>,

    /// Fetch and report what would be committed, pulled and pushed; change nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Resume after conflicts were left unresolved.
    #[arg(long = "continue")]
    pub resume: bool,
    /// Abort an in-progress sync and restore the KB to where it was before it started.
    #[arg(long)]
    pub abort: bool,
    /// Integrate remote changes but do not push.
    #[arg(long)]
    pub no_push: bool,
    /// Message for the commit of hand-made edits (default: "kb: sync N entries").
    #[arg(short, long, value_name = "MSG")]
    pub message: Option<String>,
    /// Print the summary as JSON.
    #[arg(long)]
    pub json: bool,
}

/// Reads a one-letter answer per conflict from `input`; end of input leaves the conflict for
/// `kb sync --continue`. `edit` opens the merged text in an editor (`None`: no editor available).
pub struct PromptHandler<R, W, E> {
    pub input: R,
    pub out: W,
    pub edit: E,
    pub host: String,
}

impl<R: BufRead, W: Write, E: FnMut(&str) -> Option<String>> PromptHandler<R, W, E> {
    fn ask(&mut self, prompt: &str) -> Result<String, ConflictError> {
        let _ = write!(self.out, "{prompt}");
        let _ = self.out.flush();
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) | Err(_) => Err(ConflictError::Deferred),
            Ok(_) => Ok(line.trim().to_string()),
        }
    }
}

/// The text offered for "edit merged": both versions between conflict markers.
pub fn merged_template(c: &Conflict) -> String {
    format!(
        "<<<<<<< mine\n{}=======\n{}>>>>>>> theirs\n",
        c.mine.as_deref().unwrap_or(""),
        c.theirs.as_deref().unwrap_or("")
    )
}

fn describe(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::BothModified => "edited on both sides",
        ConflictKind::ModifiedVsDeleted => "edited here, deleted there",
        ConflictKind::DeletedVsModified => "deleted here, edited there",
        ConflictKind::BothAdded => "added on both sides",
        ConflictKind::RenamedDifferently => "renamed differently on both sides",
    }
}

impl<R: BufRead, W: Write, E: FnMut(&str) -> Option<String>> ConflictHandler
    for PromptHandler<R, W, E>
{
    fn resolve(&mut self, c: &Conflict) -> Result<ResolutionChoice, ConflictError> {
        let _ = writeln!(self.out, "\nconflict: {} ({})", c.id, describe(c.kind));
        loop {
            let a = self.ask("[m]ine  [t]heirs  [e]dit merged  [b]oth  [d]elete  [q]uit > ")?;
            match a.as_str() {
                "m" => return Ok(ResolutionChoice::Mine),
                "t" => return Ok(ResolutionChoice::Theirs),
                "d" => return Ok(ResolutionChoice::Delete),
                "q" => return Err(ConflictError::UserAborted),
                "b" => {
                    return Ok(ResolutionChoice::KeepBoth {
                        new_id: format!("{}-conflict-{}", c.id, self.host),
                    });
                }
                "e" => {
                    let mut text = merged_template(c);
                    loop {
                        let Some(edited) = (self.edit)(&text) else {
                            let _ = writeln!(self.out, "no editor available here");
                            break;
                        };
                        match validate_entry_text(&c.id, &edited) {
                            None => return Ok(ResolutionChoice::EditMerged(edited)),
                            Some(problems) => {
                                let _ = writeln!(
                                    self.out,
                                    "the merged entry has problems:\n{problems}"
                                );
                                text = edited;
                                if self.ask("[r]e-edit  [d]iscard > ")? != "r" {
                                    break;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn new_id(&mut self, taken: &str) -> Result<String, ConflictError> {
        loop {
            let id = self.ask(&format!("{taken} already exists; save theirs as > "))?;
            if !id.is_empty() {
                return Ok(id);
            }
        }
    }
}

pub fn host() -> String {
    ["HOSTNAME", "HOST"]
        .iter()
        .find_map(|v| std::env::var(v).ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().split('.').next().unwrap_or("").to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "remote".into())
}

fn merged_in_editor(platform: &impl Platform, loaded: &Loaded, text: &str) -> Option<String> {
    let tty = platform.open_tty().ok()?;
    let temp = platform.private_temp_file("md", text.as_bytes()).ok()?;
    let editor = platform.resolve_editor(loaded.settings.editor());
    platform
        .run_editor(&editor, temp.path(), &tty, &|| false)
        .ok()??;
    temp.read_to_string().ok()
}

/// Exit code and message for a sync failure (cli.md: 1 blocked, 2 usage or I/O).
fn failure(e: &SyncError, json: bool) -> ExitCode {
    let code = match e {
        SyncError::ConflictAborted => 130,
        SyncError::Git(_) | SyncError::Io(_) => 2,
        _ => 1,
    };
    eprintln!("kb: {e}");
    if json {
        println!(
            "{}",
            serde_json::json!({"status": "error", "error": e.to_string()})
        );
    }
    ExitCode::from(code)
}

fn print_summary(s: &SyncSummary, json: bool) {
    if json {
        let mut v = serde_json::to_value(s).unwrap_or_default();
        v["status"] = "success".into();
        println!("{v}");
        return;
    }
    println!("kb: {}", s.new_status);
    println!(
        "  local  +{} ~{} -{}   remote  +{} ~{} -{}   conflicts resolved: {}",
        s.local_added,
        s.local_modified,
        s.local_deleted,
        s.remote_added,
        s.remote_modified,
        s.remote_deleted,
        s.conflicts_resolved
    );
    for a in &s.allowed_secrets {
        println!("  allowed secret (kb:allow-secret): {a}");
    }
    for p in &s.unmanaged_files {
        println!("  not managed by kb: {}", p.display());
    }
    for w in &s.warnings {
        println!("  warning: {w}");
    }
}

impl SyncArgs {
    pub fn json(&self) -> bool {
        match &self.command {
            Some(SyncSub::Status(a)) => a.json,
            Some(SyncSub::Setup(_)) => false,
            None => self.json,
        }
    }
}

fn ago(secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    match now.saturating_sub(secs) {
        s if s < 90 => "just now".into(),
        s if s < 5400 => format!("{}m ago", s / 60),
        s if s < 172_800 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

fn run_status(root: &Path, loaded: &Loaded, args: &StatusArgs) -> Result<ExitCode, CliError> {
    let git = SystemGitRunner;
    let config = SyncConfig {
        strategy: loaded.settings.sync_strategy(),
        timeout: Duration::from_secs(loaded.settings.sync_timeout()),
    };
    if args.fetch
        && let Err(e) = SyncEngine::new(root, &git, config).fetch()
    {
        eprintln!("kb: fetch failed: {e}");
    }
    let st = SyncStatus::query(&git, root).map_err(|e| CliError::new("io", e.to_string()))?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&st).unwrap_or_default());
        return Ok(ExitCode::SUCCESS);
    }
    let g = &st.git;
    if !g.is_git {
        println!("kb: not a git repository (try `kb sync setup <url>`)");
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "branch {} -> {}",
        g.branch.as_deref().unwrap_or("(detached)"),
        g.upstream.as_deref().unwrap_or("(no upstream)")
    );
    println!("ahead {}, behind {}", g.ahead, g.behind);
    println!("{} uncommitted entries", g.uncommitted.len());
    for u in &g.uncommitted {
        println!("  {:?} {}", u.status, u.id);
    }
    for p in &g.unmanaged_changes {
        println!("  not managed by kb: {}", p.display());
    }
    println!("last fetch: {}", st.last_fetch.map_or("never".into(), ago));
    if st.in_progress.is_some() {
        println!("a sync is in progress: run `kb sync --continue` or `kb sync --abort`");
    }
    Ok(ExitCode::SUCCESS)
}

fn run_setup(root: &Path, loaded: &Loaded, args: &SetupArgs) -> Result<ExitCode, CliError> {
    let timeout = Duration::from_secs(loaded.settings.sync_timeout());
    Ok(
        match kb_core::sync::setup(
            root,
            &SystemGitRunner,
            &args.remote_url,
            args.branch.as_deref(),
            timeout,
        ) {
            Ok(r) => {
                println!(
                    "kb: pushed {} to {} and set it as upstream",
                    r.branch, args.remote_url
                );
                ExitCode::SUCCESS
            }
            Err(e) => failure(&e, false),
        },
    )
}

pub fn run_sync(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    args: &SyncArgs,
) -> Result<ExitCode, CliError> {
    match &args.command {
        Some(SyncSub::Status(a)) => return run_status(root, loaded, a),
        Some(SyncSub::Setup(a)) => return run_setup(root, loaded, a),
        None => {}
    }
    let git = SystemGitRunner;
    let config = SyncConfig {
        strategy: loaded.settings.sync_strategy(),
        timeout: Duration::from_secs(loaded.settings.sync_timeout()),
    };
    let engine = SyncEngine::new(root, &git, config);
    let opts = SyncOptions {
        no_push: args.no_push,
        message: args.message.clone(),
    };

    if args.abort {
        return Ok(match engine.abort() {
            Ok(()) => {
                println!("kb: sync aborted; the KB is back to where it was before the sync");
                ExitCode::SUCCESS
            }
            Err(e) => failure(&e, args.json),
        });
    }
    if args.dry_run {
        let planned = engine.fetch().and_then(|()| engine.plan());
        return Ok(match planned {
            Ok(p) => {
                if args.json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "status": "dry-run",
                            "hand_edits": p.hand_edits,
                            "commits_to_push": p.commits_to_push.len(),
                            "commits_to_integrate": p.commits_to_integrate.len(),
                        })
                    );
                } else {
                    println!("kb: would commit {} edited entries", p.hand_edits.len());
                    println!("kb: would pull {} commits", p.commits_to_integrate.len());
                    println!("kb: would push {} commits", p.commits_to_push.len());
                }
                ExitCode::SUCCESS
            }
            Err(e) => failure(&e, args.json),
        });
    }

    let mut handler = PromptHandler {
        input: std::io::stdin().lock(),
        out: std::io::stderr(),
        edit: |t: &str| merged_in_editor(platform, loaded, t),
        host: host(),
    };
    let result = if args.resume {
        engine.continue_sync(&opts, &mut handler)
    } else {
        engine.execute(&opts, &mut handler)
    };
    Ok(match result {
        Ok(s) => {
            print_summary(&s, args.json);
            ExitCode::SUCCESS
        }
        Err(e) => failure(&e, args.json),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict() -> Conflict {
        Conflict {
            id: "a".into(),
            rel_path: "a.md".into(),
            kind: ConflictKind::BothModified,
            mine: Some("m\n".into()),
            theirs: Some("t\n".into()),
            base: None,
        }
    }

    fn handler<'a>(
        input: &'a str,
        edits: Vec<&'static str>,
    ) -> PromptHandler<&'a [u8], Vec<u8>, impl FnMut(&str) -> Option<String>> {
        let mut edits = edits.into_iter();
        PromptHandler {
            input: input.as_bytes(),
            out: Vec::new(),
            edit: move |_: &str| edits.next().map(str::to_string),
            host: "mac".into(),
        }
    }

    #[test]
    fn letters_map_to_choices() {
        let mut h = handler("x\nm\nt\nb\nd\nq\n", vec![]);
        let c = conflict();
        assert_eq!(h.resolve(&c).unwrap(), ResolutionChoice::Mine);
        assert_eq!(h.resolve(&c).unwrap(), ResolutionChoice::Theirs);
        assert_eq!(
            h.resolve(&c).unwrap(),
            ResolutionChoice::KeepBoth {
                new_id: "a-conflict-mac".into()
            }
        );
        assert_eq!(h.resolve(&c).unwrap(), ResolutionChoice::Delete);
        assert!(matches!(h.resolve(&c), Err(ConflictError::UserAborted)));
    }

    #[test]
    fn end_of_input_defers() {
        assert!(matches!(
            handler("", vec![]).resolve(&conflict()),
            Err(ConflictError::Deferred)
        ));
    }

    #[test]
    fn invalid_merge_loops_until_it_lints() {
        let good = "---\nkb_format: 1\ntype: cmd\ntitle: A\n---\n\n```\nls\n```\n";
        let mut h = handler("e\nr\n", vec!["<<<<<<< junk\n", good]);
        assert_eq!(
            h.resolve(&conflict()).unwrap(),
            ResolutionChoice::EditMerged(good.into())
        );
        assert!(String::from_utf8_lossy(&h.out).contains("problems"));
    }
}
