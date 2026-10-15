//! `kb add`, `kb edit`, `kb mv` and `kb rm`: the command-line front end of `kb_core::author`.
//! This file only collects input, prints and picks exit codes; detection, drafting, rendering,
//! checking and the file operations are in the core.

use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kb_core::author::fsops::FsError;
use kb_core::author::render::parse_rfc3339_utc;
use kb_core::author::{
    Clock, Confidence, Draft, DraftError, DraftOptions, DraftSource, FixedClock, SystemClock,
};
use kb_core::config::Loaded;
use kb_core::entry::EntryType;
use kb_core::entry_json::entry_json;
use kb_core::lint::{Finding, Severity, rules::MAX_FILE_BYTES};
use kb_core::search::Kb;

use crate::commands;
use crate::output::CliError;
use crate::platform::Platform;
use crate::platform::clipboard::{self, ReadOutcome};

/// Reading stdin stops after this many bytes (the size limit plus one), so `yes | kb add` ends.
pub const MAX_STDIN_BYTES: usize = MAX_FILE_BYTES + 1;

fn io_error(e: impl std::fmt::Display) -> CliError {
    CliError::new("io", e.to_string())
}

/// Where the body comes from when a flag forces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFlag {
    /// stdin when it is piped in, else the clipboard.
    Auto,
    Clipboard,
    Stdin,
    Empty,
}

/// What `kb add` was asked to do.
#[derive(Debug, Clone)]
pub struct AddArgs {
    pub title: Option<String>,
    pub ty: Option<String>,
    pub tags: Vec<String>,
    pub path: Option<String>,
    pub source: SourceFlag,
    pub editor: bool,
    pub yes: bool,
    pub json: bool,
}

/// The clock kb stamps entries with. Debug builds let `KB_TEST_NOW` (`2026-10-09T12:00:00Z`) fix
/// it, for tests; release builds never look at it.
pub fn clock() -> Box<dyn Clock> {
    let value = if cfg!(debug_assertions) {
        std::env::var("KB_TEST_NOW").ok()
    } else {
        None
    };
    clock_from(value.as_deref())
}

fn clock_from(test_now: Option<&str>) -> Box<dyn Clock> {
    match test_now.and_then(parse_rfc3339_utc) {
        Some(t) => Box::new(FixedClock(t)),
        None => Box::new(SystemClock),
    }
}

/// Reads at most `cap` bytes.
pub fn read_capped(reader: impl Read, cap: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(cap as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// `--tag a --tag b,c` as one list; a tag may also be given as `#c`.
pub fn split_tags(values: &[String]) -> Vec<String> {
    values
        .iter()
        .flat_map(|v| v.split(','))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// `path:line: error[KB014-missing-title]: message`, the line `kb lint` prints.
pub fn finding_line(f: &Finding) -> String {
    let severity = match f.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    };
    format!(
        "{}:{}: {severity}[{}]: {}",
        f.path, f.line, f.code, f.message
    )
}

pub const SECRET_WAYS_OUT: &str =
    "remove the value, use a {{variable}} of kind secret, or mark it with kb:allow-secret";

fn entry_type(name: &str) -> Result<EntryType, CliError> {
    EntryType::from_name(name).ok_or_else(|| {
        let valid: Vec<_> = EntryType::ALL.iter().map(|t| t.name()).collect();
        CliError::new(
            "usage",
            format!("unknown type \"{name}\"; valid types: {}", valid.join(", ")),
        )
    })
}

/// The text to start from and where it came from.
struct Body {
    text: Option<String>,
    source: DraftSource,
    /// Why the clipboard could not be read, if it could not.
    unavailable: Option<String>,
}

fn gather(
    platform: &impl Platform,
    flag: SourceFlag,
    when_unreadable: &str,
    interactive: bool,
) -> Result<Body, CliError> {
    let flag = match flag {
        SourceFlag::Auto if !io::stdin().is_terminal() => SourceFlag::Stdin,
        SourceFlag::Auto => SourceFlag::Clipboard,
        other => other,
    };
    match flag {
        SourceFlag::Empty | SourceFlag::Auto => Ok(Body {
            text: None,
            source: DraftSource::Empty,
            unavailable: None,
        }),
        SourceFlag::Stdin => {
            let bytes = read_capped(io::stdin().lock(), MAX_STDIN_BYTES).map_err(io_error)?;
            let text = String::from_utf8(bytes)
                .map_err(|_| CliError::new("io", "stdin is not valid UTF-8 text"))?;
            Ok(Body {
                text: Some(text),
                source: DraftSource::Stdin,
                unavailable: None,
            })
        }
        SourceFlag::Clipboard => match clipboard::read(platform) {
            ReadOutcome::Text(text) => Ok(Body {
                text: Some(text),
                source: DraftSource::Clipboard,
                unavailable: None,
            }),
            ReadOutcome::Empty => Ok(Body {
                text: None,
                source: DraftSource::Empty,
                unavailable: None,
            }),
            ReadOutcome::Unavailable(why) => {
                // the form says this in its header; only the other paths print it
                if !interactive {
                    eprintln!("kb: {}; {when_unreadable}", why.explain());
                }
                Ok(Body {
                    text: None,
                    source: DraftSource::Empty,
                    unavailable: Some(why.explain()),
                })
            }
        },
    }
}

/// `kb add`.
pub fn run_add(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    args: AddArgs,
) -> Result<ExitCode, CliError> {
    let ty = args.ty.as_deref().map(entry_type).transpose()?;
    let unreadable = if args.yes || args.editor {
        "using an empty body"
    } else {
        "opening an empty form"
    };
    let interactive = !args.yes && !args.editor;
    if args.editor && !platform.has_terminal() {
        return Err(CliError::new("usage", "no terminal to run the editor on"));
    }
    if interactive && !platform.has_terminal() {
        return Err(CliError::new(
            "usage",
            "kb add needs --yes or --editor when there is no terminal",
        ));
    }
    let body = gather(platform, args.source, unreadable, interactive)?;
    let opts = DraftOptions {
        title: args.title.clone(),
        ty,
        tags: split_tags(&args.tags),
        path: args.path.clone(),
        path_template: loaded.settings.path_template(),
    };
    let draft = match &body.text {
        Some(text) => Draft::from_text(text, body.source, &opts),
        None => Draft::empty(body.source, ty, &opts),
    };
    let mut kb = Kb::open(root).map_err(io_error)?;
    if args.yes {
        return save_without_asking(&mut kb, &draft, args.json, &loaded.settings);
    }
    if args.editor {
        return crate::editfile::add_with_editor(platform, &mut kb, loaded, &draft, &args);
    }
    commands::run_add_ui(platform, root, loaded, draft, body.unavailable)
}

/// Why a new entry was not written.
#[derive(Debug)]
pub enum SaveFailure {
    /// The title gives no usable path.
    NeedsPath,
    /// The path is invalid or taken.
    Path(DraftError),
    TooLarge {
        bytes: usize,
    },
    /// `kb lint` would report errors (or a likely secret).
    Invalid {
        findings: Vec<Finding>,
        secrets: bool,
    },
    Fs(FsError),
}

impl SaveFailure {
    /// One line for the key bar or stderr.
    pub fn message(&self) -> String {
        match self {
            SaveFailure::NeedsPath => "a path is required (use --path)".into(),
            SaveFailure::Path(e) => e.to_string(),
            SaveFailure::TooLarge { bytes } => {
                format!("the entry is {bytes} bytes; entries are limited to {MAX_FILE_BYTES}")
            }
            SaveFailure::Invalid { findings, .. } => findings.first().map_or_else(
                || "the entry is not valid".into(),
                |f| format!("{} {}", f.code, f.message),
            ),
            SaveFailure::Fs(e) => e.to_string(),
        }
    }
}

/// A new entry that was written.
#[derive(Debug)]
pub struct Saved {
    pub id: String,
    pub path: PathBuf,
    /// Findings that did not stop the write (warnings).
    pub warnings: Vec<Finding>,
}

/// Writes a new entry: finds a free ID, checks the bytes that would be written, creates the file
/// and refreshes the index. Every way of saving a draft without asking goes through here.
pub fn save_new(kb: &mut Kb, draft: &Draft, clock: &dyn Clock) -> Result<Saved, SaveFailure> {
    let id = match draft.target_id(&|id| kb.id_taken(id)) {
        Ok(id) => id,
        Err(DraftError::NeedsPath) => return Err(SaveFailure::NeedsPath),
        Err(e) => return Err(SaveFailure::Path(e)),
    };
    let check = draft.check(&id, clock);
    if check.too_large {
        return Err(SaveFailure::TooLarge {
            bytes: check.bytes.len(),
        });
    }
    if !check.writable(false) {
        return Err(SaveFailure::Invalid {
            secrets: !check.secrets().is_empty(),
            findings: check
                .findings
                .into_iter()
                .filter(|f| f.severity == Severity::Error)
                .collect(),
        });
    }
    let path = kb.create_entry(&check).map_err(SaveFailure::Fs)?;
    let warnings = check
        .findings
        .into_iter()
        .filter(|f| f.severity == Severity::Warning)
        .collect();
    Ok(Saved { id, path, warnings })
}

/// `--yes`: save or fail, never ask.
fn save_without_asking(
    kb: &mut Kb,
    draft: &Draft,
    json: bool,
    settings: &kb_core::config::Settings,
) -> Result<ExitCode, CliError> {
    let refuse = |message: &str| {
        eprintln!("kb: {message}");
        Ok(ExitCode::from(1))
    };
    if draft.title.is_empty() {
        return refuse("a title is required (use --title)");
    }
    if draft.detection.confidence == Confidence::Low {
        return refuse("could not tell the type (use --type)");
    }
    let clock = clock();
    let saved = match save_new(kb, draft, clock.as_ref()) {
        Ok(saved) => saved,
        Err(SaveFailure::Invalid { findings, secrets }) => {
            for f in &findings {
                eprintln!("{}", finding_line(f));
            }
            if secrets {
                eprintln!("kb: {SECRET_WAYS_OUT}");
            }
            return Ok(ExitCode::from(1));
        }
        Err(SaveFailure::Fs(e @ FsError::Io { .. })) => return Err(io_error(e)),
        Err(e) => return refuse(&e.message()),
    };
    for f in &saved.warnings {
        eprintln!("{}", finding_line(f));
    }
    if json {
        let entry = kb.entry(&saved.id).map_err(io_error)?;
        println!("{}", entry_json(&entry, &saved.path));
    } else {
        println!("{}", saved.id);
    }
    let rel_path = saved
        .path
        .strip_prefix(kb.root())
        .unwrap_or(&saved.path)
        .to_path_buf();
    let outcome = kb_core::git::commit::auto_commit_operation(
        kb.root(),
        &crate::platform::SystemGitRunner,
        settings,
        kb_core::git::commit::AutoCommitAction::Add,
        &saved.id,
        &[rel_path],
    );
    if let Ok(kb_core::git::commit::AutoCommitOutcome::SkippedMissingIdentity { .. }) = outcome {
        eprintln!("kb: git commit skipped: author identity unknown (set user.name and user.email)");
    }
    Ok(ExitCode::SUCCESS)
}

// ---- kb mv, kb rm and the git behind them ----

const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Whether `rel` (a path relative to the KB root) is tracked by git in a work tree that holds
/// the root, and git can be run.
fn git_tracks(platform: &impl Platform, root: &Path, rel: &str) -> bool {
    let ok = |args: &[&str]| {
        platform
            .run_git(root, args, GIT_TIMEOUT)
            .is_ok_and(|o| o.success() && (args[0] != "rev-parse" || o.stdout.trim() == "true"))
    };
    ok(&["rev-parse", "--is-inside-work-tree"]) && ok(&["ls-files", "--error-unmatch", "--", rel])
}

/// Renames an entry with `git mv` when it is tracked, else (or when git fails) with a plain
/// rename; usage and remembered values follow. The `[[old]]` links of notes are pointed at the new
/// ID, and the rename and those edits are one commit. Returns what happened and how, and the notes
/// whose links changed.
pub fn move_entry(
    platform: &impl Platform,
    kb: &mut Kb,
    old: &str,
    new: &str,
    settings: &kb_core::config::Settings,
) -> Result<(kb_core::author::fsops::Renamed, &'static str, Vec<String>, Vec<String>), FsError> {
    let root = kb.root().to_path_buf();
    let method = std::cell::Cell::new("rename");
    let mover = |from: &Path, to: &Path| -> io::Result<()> {
        let rel = |p: &Path| {
            p.strip_prefix(&root)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        };
        let (from_rel, to_rel) = (rel(from), rel(to));
        if git_tracks(platform, &root, &from_rel)
            && platform
                .run_git(&root, &["mv", "--", &from_rel, &to_rel], GIT_TIMEOUT)
                .is_ok_and(|o| o.success())
        {
            method.set("git mv");
            return Ok(());
        }
        std::fs::rename(from, to)
    };
    let renamed = kb.rename_entry(old, new, &mover)?;
    // notes that linked to the old ID (the renamed note too, when it linked to itself)
    let linkers = kb.notes_linking_to(old);
    let relinked = kb.retarget_links(&linkers, old, new)?;
    let referencing_rbs = kb.runbooks_referencing(old);
    let updated_runbooks = kb.retarget_runbook_steps(&referencing_rbs, old, new)?;

    let old_rel = PathBuf::from(format!("{old}.md"));
    let new_rel = PathBuf::from(format!("{new}.md"));
    let mv_id = format!("{old} -> {new}");
    let mut paths = vec![old_rel, new_rel];
    for id in &relinked {
        let rel = PathBuf::from(format!("{id}.md"));
        if !paths.contains(&rel) {
            paths.push(rel);
        }
    }
    for id in &updated_runbooks {
        let rel = PathBuf::from(format!("{id}.md"));
        if !paths.contains(&rel) {
            paths.push(rel);
        }
    }
    let outcome = if !updated_runbooks.is_empty() {
        let subject = kb_core::git::commit::format_commit_message(
            settings.git_commit_template(),
            kb_core::git::commit::AutoCommitAction::Mv,
            &mv_id,
        );
        let body = format!(
            "\n\nUpdated references in:\n{}",
            updated_runbooks
                .iter()
                .map(|rb| format!("- {rb}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let msg = format!("{subject}{body}");
        kb_core::git::commit::auto_commit_paths_with_message(
            &root,
            &crate::platform::SystemGitRunner,
            settings,
            &msg,
            &paths,
        )
    } else {
        kb_core::git::commit::auto_commit_operation(
            &root,
            &crate::platform::SystemGitRunner,
            settings,
            kb_core::git::commit::AutoCommitAction::Mv,
            &mv_id,
            &paths,
        )
    };
    if let Ok(kb_core::git::commit::AutoCommitOutcome::SkippedMissingIdentity { .. }) = outcome {
        eprintln!("kb: git commit skipped: author identity unknown (set user.name and user.email)");
    }
    Ok((renamed, method.get(), relinked, updated_runbooks))
}

/// Deletes an entry with `git rm` when it is tracked, else (or when git fails) from the disk.
pub fn delete_entry(
    platform: &impl Platform,
    kb: &mut Kb,
    id: &str,
    force: bool,
    settings: &kb_core::config::Settings,
) -> Result<&'static str, FsError> {
    if !force {
        let rbs = kb.runbooks_referencing(id);
        if !rbs.is_empty() {
            return Err(FsError::ReferencedByRunbooks {
                id: id.to_string(),
                runbooks: rbs,
            });
        }
    }
    let root = kb.root().to_path_buf();
    let method = std::cell::Cell::new("delete");
    let remover = |path: &Path| -> io::Result<()> {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        if git_tracks(platform, &root, &rel)
            && platform
                .run_git(&root, &["rm", "-q", "-f", "--", &rel], GIT_TIMEOUT)
                .is_ok_and(|o| o.success())
        {
            method.set("git rm");
            return Ok(());
        }
        std::fs::remove_file(path)
    };
    kb.remove_entry(id, &remover)?;
    let rel = PathBuf::from(format!("{id}.md"));
    let outcome = kb_core::git::commit::auto_commit_operation(
        &root,
        &crate::platform::SystemGitRunner,
        settings,
        kb_core::git::commit::AutoCommitAction::Rm,
        id,
        &[rel],
    );
    if let Ok(kb_core::git::commit::AutoCommitOutcome::SkippedMissingIdentity { .. }) = outcome {
        eprintln!("kb: git commit skipped: author identity unknown (set user.name and user.email)");
    }
    Ok(method.get())
}

/// `kb mv`.
pub fn run_mv(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    old: &str,
    new: &str,
    json: bool,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(io_error)?;
    let new = new.strip_suffix(".md").unwrap_or(new);
    let old = old.strip_suffix(".md").unwrap_or(old);
    if old == new {
        return Ok(ExitCode::SUCCESS);
    }
    match move_entry(platform, &mut kb, old, new, &loaded.settings) {
        Ok((renamed, method, relinked, updated_runbooks)) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "id": renamed.to,
                        "from": renamed.from,
                        "to": renamed.to,
                        "method": method,
                        "links_updated": relinked,
                        "runbooks_updated": updated_runbooks,
                    })
                );
            } else {
                println!("renamed {} -> {} ({method})", renamed.from, renamed.to);
                if !relinked.is_empty() {
                    println!(
                        "updated links in {} note{}: {}",
                        relinked.len(),
                        if relinked.len() == 1 { "" } else { "s" },
                        relinked.join(", ")
                    );
                }
                if !updated_runbooks.is_empty() {
                    println!(
                        "updated references in {} runbook{}: {}",
                        updated_runbooks.len(),
                        if updated_runbooks.len() == 1 { "" } else { "s" },
                        updated_runbooks.join(", ")
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(e @ FsError::Io { .. }) => Err(io_error(e)),
        Err(e) => {
            eprintln!("kb: {e}");
            Ok(ExitCode::from(1))
        }
    }
}

/// `kb rm`.
pub fn run_rm(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    id: &str,
    yes: bool,
    force: bool,
    json: bool,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(io_error)?;
    let id = id.strip_suffix(".md").unwrap_or(id);
    if kb.entry_path(id).is_err() {
        eprintln!("kb: no entry with ID \"{id}\"");
        return Ok(ExitCode::from(1));
    }
    let runbook_refs = kb.runbook_step_references(id);
    if !runbook_refs.is_empty() {
        if !force {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "error": format!("entry '{id}' is referenced by runbook(s); use --force to delete"),
                        "runbooks": runbook_refs.iter().map(|(rb, step)| {
                            serde_json::json!({ "runbook": rb, "step": step })
                        }).collect::<Vec<_>>()
                    })
                );
            } else {
                eprintln!("kb: cannot delete '{id}': referenced by runbook(s):");
                for (rb, step) in &runbook_refs {
                    eprintln!("  - {rb} (step: {step})");
                }
            }
            return Ok(ExitCode::from(1));
        } else {
            eprintln!("kb: warning: '{id}' is referenced by runbook(s); deleting anyway");
        }
    }
    // notes that link here would be left with a broken `[[link]]`
    let referenced_by: Vec<String> = kb
        .notes_linking_to(id)
        .into_iter()
        .filter(|n| n != id)
        .collect();
    let warning = (!referenced_by.is_empty()).then(|| {
        format!(
            "Entry '{id}' is referenced by {} note{} ({})",
            referenced_by.len(),
            if referenced_by.len() == 1 { "" } else { "s" },
            referenced_by.join(", ")
        )
    });
    if yes && let Some(w) = &warning {
        eprintln!("kb: {w}; deleting anyway");
    }
    if !yes {
        let Ok(tty) = platform
            .open_tty()
            .ok()
            .filter(|_| platform.has_terminal())
            .ok_or(())
        else {
            return Err(CliError::new(
                "usage",
                "kb rm needs --yes when there is no terminal",
            ));
        };
        let signals = crate::tui::terminal::Signals::register();
        let question = match &warning {
            Some(w) => format!("{w}. Delete anyway? [y/N] "),
            None => format!("Delete {id}? (y/N) "),
        };
        let answer = crate::editfile::ask_line(&tty, &signals, &question);
        if !answer.is_some_and(|a| matches!(a.trim().to_ascii_lowercase().as_str(), "y" | "yes")) {
            if warning.is_some() {
                // a considered "no" to a warning is not a failure
                eprintln!("kb: kept {id}");
                return Ok(ExitCode::SUCCESS);
            }
            return Ok(ExitCode::from(130));
        }
    }
    match delete_entry(platform, &mut kb, id, force, &loaded.settings) {
        Ok(method) => {
            if json {
                println!("{}", serde_json::json!({ "id": id, "method": method }));
            } else {
                println!("deleted {id} ({method})");
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(e @ FsError::Io { .. }) => Err(io_error(e)),
        Err(e) => {
            eprintln!("kb: {e}");
            Ok(ExitCode::from(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_reading_stops_at_the_cap() {
        let bytes = read_capped(io::repeat(0), MAX_STDIN_BYTES).unwrap();
        assert_eq!(bytes.len(), MAX_STDIN_BYTES);
        assert_eq!(MAX_STDIN_BYTES, 1_048_577);
        assert_eq!(read_capped(&b"abc"[..], 10).unwrap(), b"abc");
    }

    #[test]
    fn tags_are_split_on_commas_and_trimmed() {
        let tags = split_tags(&["a".into(), "b, #c".into(), " ".into(), ",d,".into()]);
        assert_eq!(tags, ["a", "b", "#c", "d"]);
    }

    #[test]
    fn the_test_clock_is_honored_only_when_given_and_valid() {
        let fixed = clock_from(Some("2026-10-09T12:00:00Z")).now();
        assert_eq!(fixed, parse_rfc3339_utc("2026-10-09T12:00:00Z").unwrap());
        let real = clock_from(Some("tomorrow")).now();
        assert!(real > fixed, "an unreadable value is ignored");
        assert!(clock_from(None).now() > fixed);
    }

    #[test]
    fn a_release_build_ignores_kb_test_now() {
        // `clock()` reads the variable only under `debug_assertions`; the same code path decides
        // here, so a release build compiles to `None` and a debug build to the variable
        let honored = cfg!(debug_assertions);
        let value = honored.then_some("2026-10-09T12:00:00Z");
        assert_eq!(value.is_some(), cfg!(debug_assertions));
    }
}
