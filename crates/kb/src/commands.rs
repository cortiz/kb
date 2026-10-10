//! Commands that do what a TUI action does, so every TUI action is scriptable.

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::SystemTime;

use kb_core::config::{
    self, ConfigEditError, EnterAction, Kind, Loaded, Resolved, SETTINGS, Settings, Source,
};
use kb_core::entry_json::PrintMode;
use kb_core::resolve::{Candidate, Resolution};
use kb_core::search::Kb;
use serde_json::{Value, json};

use crate::doctor::{self, Check, Status};
use crate::output::CliError;
use crate::platform::Platform;
use crate::platform::clipboard::{self, ClipboardSettings, CopyOutcome, Method, Place};
use crate::platform::{Opened, opener};
use crate::show;
use crate::tui::app::Options;
use crate::tui::theme::{Theme, ThemeEnv};
use crate::tui::{self, Outcome};
use crate::varcli::{self, Ask, Collected, TtyAsk, UseOpts};
use kb_core::entry::{EntryType, TypeFields};
use kb_core::varres;

fn io_error(e: impl std::fmt::Display) -> CliError {
    CliError::new("io", e.to_string())
}

/// How `kb show` prints an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShowMode {
    /// The preview as text (colored on a terminal).
    #[default]
    Preview,
    /// Only the payload.
    Raw,
    /// The entry as JSON.
    Json,
    /// The absolute path of the entry's file.
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShowOptions {
    pub mode: ShowMode,
    pub no_color: bool,
}

/// What happens to the entry the UI returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostPick {
    /// Put the payload on the clipboard (the default for Enter, and what `kb copy` does).
    Copy,
    /// Print this view of the entry on stdout (`--print`, or `tui.enter = "print"`).
    Print(PrintMode),
    /// Print the entry the way `kb show` does (when the UI was opened by `kb show`).
    Show(ShowOptions),
    /// Resolve the entry's variables and copy or print it (`kb use` and `kb copy` that opened the UI).
    Use(UseOpts),
    /// Copy the payload as it is in the file, placeholders included (`kb copy --raw`).
    Raw,
    /// Open the entry's primary link or console deep link (`kb open`).
    Open(OpenOpts),
    /// Run a cmd entry or custom action (`kb run`).
    Run(RunOpts),
}

/// Options for `kb open`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenOpts {
    pub with_target: Option<String>,
    pub use_opts: UseOpts,
}

/// Options for `kb run`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOpts {
    pub yes: bool,
    pub force: bool,
    pub action: Option<String>,
    pub use_opts: UseOpts,
}

/// A copy that did not fail, reported: warnings and the confirmation on stderr, the text on stdout
/// for the stdout method, and the exit code (0 delivered, 2 refused).
fn report_copy(outcome: &CopyOutcome, id: &str, json: bool, extra: Option<Value>) -> ExitCode {
    for warning in outcome.warnings() {
        eprintln!("{warning}");
    }
    if json {
        let mut doc = copy_json(outcome, id);
        if let (Some(Value::Object(more)), Some(map)) = (extra, doc.as_object_mut()) {
            map.extend(more);
        }
        println!("{doc}");
    } else if outcome.used == Some(Method::Stdout) {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(outcome.text.as_bytes());
        let _ = stdout.flush();
    }
    if outcome.sent() {
        eprintln!("{}", outcome.message(id));
        ExitCode::SUCCESS
    } else {
        eprintln!("kb: {}", outcome.message(id));
        ExitCode::from(2)
    }
}

/// The machine-readable form of a copy (`kb copy --json`).
fn copy_json(o: &CopyOutcome, id: &str) -> Value {
    let method = o.used.as_ref().map(|m| match m {
        Method::Native(t) => t.name.to_string(),
        Method::Custom(_) => "custom".to_string(),
        Method::Osc52 => "osc52".to_string(),
        Method::Stdout => "stdout".to_string(),
    });
    let mut doc = json!({
        "id": id,
        "sent": o.sent(),
        "method": method,
        "confirmed": o.confirmed,
        "bytes": o.bytes,
        "lines": o.lines,
        "attempts": o.attempts.iter().map(|a| json!({
            "method": a.method.label(),
            "reason": a.reason,
            "hint": a.hint,
        })).collect::<Vec<_>>(),
        "notes": o.notes,
    });
    if o.used == Some(Method::Stdout) {
        doc["text"] = json!(o.text);
    }
    if let Some(t) = o.refused {
        doc["refused"] = json!({ "bytes": t.bytes, "limit": t.limit });
    }
    doc
}

/// Puts the payload of `id` on the clipboard and records the use when it was sent.
fn copy_entry(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
) -> Result<CopyOutcome, CliError> {
    let text = kb.pick(id, PrintMode::Payload).map_err(io_error)?;
    let outcome = clipboard::copy(
        platform,
        &ClipboardSettings::from_settings(settings),
        &text,
        Place::Cli,
    );
    if outcome.sent() {
        record_use(kb, id);
    }
    Ok(outcome)
}

/// A use that cannot be recorded is a warning, not a failure: the text was already delivered.
fn record_use(kb: &Kb, id: &str) {
    if let Err(e) = kb.record_use(id, SystemTime::now()) {
        eprintln!("kb: warning: could not record the use of {id}: {e}");
    }
}

/// Counts the use and, when asked and allowed, remembers the values that were used.
fn finish_use(kb: &Kb, settings: &Settings, id: &str, used: &[varcli::Used], remember: bool) {
    if used.is_empty() || !remember || !settings.vars_remember() {
        return record_use(kb, id);
    }
    let pairs: Vec<(String, String)> = used
        .iter()
        .filter_map(|u| u.text.clone().map(|t| (u.name.clone(), t)))
        .collect();
    match kb.record_resolved_use(id, SystemTime::now(), &pairs) {
        Ok(skipped) => {
            for name in skipped {
                eprintln!("kb: {name} looks like a credential, so it was not remembered");
            }
        }
        Err(e) => eprintln!("kb: warning: could not record the use of {id}: {e}"),
    }
}

const SECRET_NOTICE: &str =
    "kb: the clipboard now holds a secret; clipboard managers may keep a history";

/// Resolves the variables of `id`, then copies or prints the result and records the use (`kb use`,
/// `kb copy`, and Enter in a UI that returned an entry with variables).
fn deliver(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    opts: &UseOpts,
) -> Result<ExitCode, CliError> {
    let entry = kb.entry(id).map_err(io_error)?;
    let plan = varres::plan_vars(&entry);
    varcli::check_flag_names(&plan, &opts.flags)?;
    let mut secret = false;
    let mut used = Vec::new();
    let mut fields = Vec::new();
    let text = if plan.is_empty() {
        kb.pick(id, PrintMode::Payload).map_err(io_error)?
    } else {
        let (memory, _) = kb.remembered(id);
        let mut tty = if opts.yes || !platform.has_terminal() {
            None
        } else {
            TtyAsk::open(platform)
        };
        let ask = tty.as_mut().map(|t| t as &mut dyn Ask);
        let env = |name: &str| std::env::var(name).ok();
        match varcli::collect(&plan, &opts.flags, &env, Some(&memory), ask)? {
            Collected::Stop(code) => return Ok(code),
            Collected::Ready(values, report) => {
                let resolved = varres::resolve(&plan, &values).map_err(|errors| {
                    CliError::new(
                        "usage",
                        errors
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("; "),
                    )
                })?;
                secret = resolved.has_secret;
                used = report;
                fields = other_fields(&resolved);
                resolved
                    .primary()
                    .map(|o| o.text.clone())
                    .ok_or_else(|| CliError::new("io", format!("{id} has nothing to copy")))?
            }
        }
    };
    emit(
        platform,
        kb,
        settings,
        id,
        Prepared {
            text,
            used,
            secret,
            has_vars: !plan.is_empty(),
            fields,
        },
        opts,
    )
}

/// What `deliver` ends up with: the text to hand over and how the variables were filled.
struct Prepared {
    text: String,
    used: Vec<varcli::Used>,
    secret: bool,
    has_vars: bool,
    /// The other resolved strings (`cwd`, `range`, `log_groups`), secrets masked, for `--json`.
    fields: Vec<(String, String)>,
}

/// `(label, text)` of every resolved string that is not the primary one; secrets are masked.
fn other_fields(resolved: &varres::Resolved) -> Vec<(String, String)> {
    let primary = resolved.primary().map(|o| o.target.clone());
    resolved
        .outputs
        .iter()
        .filter(|o| Some(&o.target) != primary.as_ref())
        .map(|o| {
            let label = match &o.target {
                varres::Target::Payload => "payload".to_string(),
                varres::Target::Url => "url".to_string(),
                varres::Target::Cwd => "cwd".to_string(),
                varres::Target::Range => "range".to_string(),
                varres::Target::LogGroup(i) => format!("log_groups[{i}]"),
            };
            (label, o.redacted())
        })
        .collect()
}

/// Copies or prints the text, records the use and reports.
fn emit(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    prepared: Prepared,
    opts: &UseOpts,
) -> Result<ExitCode, CliError> {
    let Prepared {
        text,
        used,
        secret,
        has_vars,
        fields,
    } = prepared;
    let extra = has_vars.then(|| {
        json!({
            "variables": varcli::used_json(&used),
            "secret": secret,
            "fields": fields.iter().cloned().collect::<std::collections::BTreeMap<_, _>>(),
        })
    });
    if opts.print {
        if opts.json {
            let mut doc = json!({ "id": id, "sent": true, "method": "print", "text": text });
            if let (Some(Value::Object(more)), Some(map)) = (extra, doc.as_object_mut()) {
                map.extend(more);
            }
            println!("{doc}");
        } else {
            write_stdout(&text);
        }
        finish_use(kb, settings, id, &used, opts.remember);
        return Ok(ExitCode::SUCCESS);
    }
    let outcome = clipboard::copy(
        platform,
        &ClipboardSettings::from_settings(settings),
        &text,
        Place::Cli,
    );
    if outcome.sent() {
        finish_use(kb, settings, id, &used, opts.remember);
    }
    let code = report_copy(&outcome, id, opts.json, extra);
    if outcome.sent() && secret && outcome.used != Some(Method::Stdout) {
        eprintln!("{SECRET_NOTICE}");
    }
    Ok(code)
}

/// The values the UI collected: checks them against the entry as it is now, then hands the text
/// over like `kb use` does.
fn deliver_values(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    values: Vec<(String, kb_core::varres::Value, kb_core::varres::Source)>,
    opts: &UseOpts,
) -> Result<ExitCode, CliError> {
    let entry = kb.entry(id).map_err(io_error)?;
    let plan = varres::plan_vars(&entry);
    let mut map = varres::Values::new();
    let mut used = Vec::new();
    for (name, value, source) in values {
        if let Some(p) = plan.var(&name) {
            used.push(varcli::Used {
                name: name.clone(),
                kind: p.var.kind,
                source,
                text: (!value.is_secret()).then(|| value.reveal().to_string()),
            });
            map.insert(name, value);
        }
    }
    match varres::resolve(&plan, &map) {
        Ok(resolved) => {
            let text = resolved
                .primary()
                .map(|o| o.text.clone())
                .ok_or_else(|| CliError::new("io", format!("{id} has nothing to copy")))?;
            let prepared = Prepared {
                text,
                used,
                secret: resolved.has_secret,
                has_vars: true,
                fields: other_fields(&resolved),
            };
            emit(platform, kb, settings, id, prepared, opts)
        }
        Err(errors) => {
            eprintln!("kb: {id} changed while you filled it in; nothing was copied");
            for e in errors {
                eprintln!("  {e}");
            }
            Ok(ExitCode::from(1))
        }
    }
}

/// Copies, prints or shows the entry the UI returned, and returns the exit code.
pub fn run_pick(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    id: &str,
    post: &PostPick,
) -> Result<ExitCode, CliError> {
    let kb = Kb::open(root).map_err(io_error)?;
    match post {
        PostPick::Copy => deliver(
            platform,
            &kb,
            &loaded.settings,
            id,
            &UseOpts {
                remember: true,
                ..UseOpts::default()
            },
        ),
        PostPick::Use(opts) => deliver(platform, &kb, &loaded.settings, id, opts),
        PostPick::Open(opts) => deliver_open(platform, &kb, &loaded.settings, id, opts),
        PostPick::Run(opts) => deliver_run(platform, &kb, &loaded.settings, id, opts),
        PostPick::Raw => {
            let outcome = copy_entry(platform, &kb, &loaded.settings, id)?;
            Ok(report_copy(&outcome, id, false, None))
        }
        PostPick::Print(PrintMode::Payload) => deliver(
            platform,
            &kb,
            &loaded.settings,
            id,
            &UseOpts {
                print: true,
                remember: true,
                ..UseOpts::default()
            },
        ),
        PostPick::Print(mode) => {
            let out = kb.pick(id, *mode).map_err(io_error)?;
            write_stdout(&out);
            record_use(&kb, id);
            Ok(ExitCode::SUCCESS)
        }
        PostPick::Show(options) => {
            write_stdout(&show_entry(platform, &kb, &loaded.settings, id, *options)?);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn write_stdout(text: &str) {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

/// What `kb show` prints for `id`. A use is not recorded.
fn show_entry(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    options: ShowOptions,
) -> Result<String, CliError> {
    match options.mode {
        ShowMode::Raw => kb.pick(id, PrintMode::Payload).map_err(io_error),
        ShowMode::Json => kb.pick(id, PrintMode::Json).map_err(io_error),
        ShowMode::File => kb.pick(id, PrintMode::Path).map_err(io_error),
        ShowMode::Preview => {
            let terminal = std::io::stdout().is_terminal();
            let env = ThemeEnv::detect(&platform.env());
            let color = terminal && !options.no_color && !env.no_color;
            let theme = Theme::resolve(
                settings.theme(),
                &ThemeEnv {
                    no_color: !color,
                    ..env
                },
            );
            let width = if terminal {
                ratatui::crossterm::terminal::size().map_or(show::DEFAULT_WIDTH, |(w, _)| w)
            } else {
                show::DEFAULT_WIDTH
            };
            show::preview_text(kb, id, &theme, color, width).map_err(io_error)
        }
    }
}

// ---- resolving what the user typed ----

/// What `kb copy` and `kb show` do next with the argument.
enum Next {
    Entry(String),
    /// Several close matches and a terminal: let the user choose in the UI.
    Choose,
    /// Already reported; this is the exit code.
    Done(ExitCode),
}

fn candidates_json(c: &[Candidate]) -> Value {
    Value::Array(
        c.iter()
            .map(|c| json!({ "id": c.id, "title": c.title }))
            .collect(),
    )
}

fn resolve_or_report(
    platform: &impl Platform,
    kb: &mut Kb,
    text: &str,
    json: bool,
) -> Result<Next, CliError> {
    Ok(match kb.resolve(text).map_err(io_error)? {
        Resolution::Resolved(id) => Next::Entry(id),
        Resolution::NotFound => {
            let message = format!("no entry matches {text:?}");
            eprintln!("kb: {message}");
            if json {
                println!(
                    "{}",
                    json!({ "error": { "kind": "not_found", "message": message } })
                );
            }
            Next::Done(ExitCode::from(1))
        }
        Resolution::Ambiguous(c) => {
            if !json && platform.has_terminal() {
                return Ok(Next::Choose);
            }
            let message = format!("{} close matches for {text:?}; use an exact ID", c.len());
            eprintln!("kb: {message}");
            for cand in &c {
                eprintln!("{}\t{}", cand.id, cand.title);
            }
            if json {
                println!(
                    "{}",
                    json!({ "error": {
                        "kind": "ambiguous",
                        "message": message,
                        "candidates": candidates_json(&c),
                    } })
                );
            }
            Next::Done(ExitCode::from(1))
        }
    })
}

/// `kb use <id|query>`, and `kb copy` without `--raw`.
pub fn run_use(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    text: &str,
    opts: &UseOpts,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(io_error)?;
    match resolve_or_report(platform, &mut kb, text, opts.json)? {
        Next::Entry(id) => deliver(platform, &kb, &loaded.settings, &id, opts),
        Next::Choose => {
            if !opts.flags.is_empty() || opts.yes {
                eprintln!("kb: --var and --yes are ignored when you choose in the UI");
            }
            let post = PostPick::Use(UseOpts {
                flags: Vec::new(),
                yes: false,
                ..opts.clone()
            });
            run_ui(platform, root, loaded, text, &post, false)
        }
        Next::Done(code) => Ok(code),
    }
}

/// `kb open <id|query>`.
pub fn run_open(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    text: &str,
    opts: &OpenOpts,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(io_error)?;
    match resolve_or_report(platform, &mut kb, text, opts.use_opts.json)? {
        Next::Entry(id) => deliver_open(platform, &kb, &loaded.settings, &id, opts),
        Next::Choose => {
            if !opts.use_opts.flags.is_empty() || opts.use_opts.yes {
                eprintln!("kb: --var and --yes are ignored when you choose in the UI");
            }
            let post = PostPick::Open(OpenOpts {
                with_target: opts.with_target.clone(),
                use_opts: UseOpts {
                    flags: Vec::new(),
                    yes: false,
                    ..opts.use_opts.clone()
                },
            });
            run_ui(platform, root, loaded, text, &post, false)
        }
        Next::Done(code) => Ok(code),
    }
}

pub fn deliver_open(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    opts: &OpenOpts,
) -> Result<ExitCode, CliError> {
    let entry = kb.entry(id).map_err(io_error)?;
    let plan = varres::plan_vars(&entry);
    varcli::check_flag_names(&plan, &opts.use_opts.flags)?;

    let mut used = Vec::new();
    let resolved_result = if plan.is_empty() {
        None
    } else {
        let (memory, _) = kb.remembered(id);
        let mut tty = if opts.use_opts.yes || !platform.has_terminal() {
            None
        } else {
            TtyAsk::open(platform)
        };
        let ask = tty.as_mut().map(|t| t as &mut dyn Ask);
        let env = |name: &str| std::env::var(name).ok();
        match varcli::collect(&plan, &opts.use_opts.flags, &env, Some(&memory), ask)? {
            Collected::Stop(code) => return Ok(code),
            Collected::Ready(values, report) => {
                let resolved = varres::resolve(&plan, &values).map_err(|errors| {
                    CliError::new(
                        "usage",
                        errors
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("; "),
                    )
                })?;
                used = report;
                Some(resolved)
            }
        }
    };

    deliver_open_resolved(
        platform,
        kb,
        settings,
        id,
        opts.with_target.as_deref(),
        resolved_result,
        &used,
        opts.use_opts.remember,
    )
}

fn deliver_open_values(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    values: Vec<(String, kb_core::varres::Value, kb_core::varres::Source)>,
    opts: &OpenOpts,
) -> Result<ExitCode, CliError> {
    let entry = kb.entry(id).map_err(io_error)?;
    let plan = varres::plan_vars(&entry);
    let mut map = varres::Values::new();
    let mut used = Vec::new();
    for (name, value, source) in values {
        if let Some(p) = plan.var(&name) {
            used.push(varcli::Used {
                name: name.clone(),
                kind: p.var.kind,
                source,
                text: (!value.is_secret()).then(|| value.reveal().to_string()),
            });
            map.insert(name, value);
        }
    }
    let resolved = varres::resolve(&plan, &map).map_err(|errors| {
        CliError::new(
            "usage",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    deliver_open_resolved(
        platform,
        kb,
        settings,
        id,
        opts.with_target.as_deref(),
        Some(resolved),
        &used,
        opts.use_opts.remember,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn deliver_open_resolved(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    with_target: Option<&str>,
    resolved_result: Option<varres::Resolved>,
    used: &[varcli::Used],
    remember: bool,
) -> Result<ExitCode, CliError> {
    let entry = kb.entry(id).map_err(io_error)?;
    let ty = entry.entry_type().ok_or_else(|| {
        CliError::new("usage", format!("{id}: entry has missing or unknown type"))
    })?;

    if let Some(target) = with_target {
        match ty {
            EntryType::Sumo | EntryType::Cw => {
                if target != "console" {
                    return Err(CliError::new(
                        "usage",
                        format!(
                            "--with {target}: valid target for {} entries is 'console'",
                            ty.name()
                        ),
                    ));
                }
            }
            EntryType::Code => {
                if target != "web" && target != "vscode" && target != "idea" {
                    return Err(CliError::new(
                        "usage",
                        format!(
                            "--with {target}: valid targets for code entries are 'web', 'vscode', 'idea'"
                        ),
                    ));
                }
            }
            _ => {
                return Err(CliError::new(
                    "usage",
                    format!("--with is not supported for {} entries", ty.name()),
                ));
            }
        }
    }

    match ty {
        EntryType::Link => {
            let url = if let Some(resolved) = &resolved_result {
                resolved
                    .get(&varres::Target::Url)
                    .map(|o| o.text.clone())
                    .or_else(|| {
                        if let TypeFields::Link { url: Some(u) } = entry.fields() {
                            Some(u.clone())
                        } else {
                            None
                        }
                    })
            } else if let TypeFields::Link { url: Some(u) } = entry.fields() {
                Some(u.clone())
            } else {
                None
            }
            .ok_or_else(|| CliError::new("usage", format!("{id}: link has no URL")))?;

            execute_open_url(platform, kb, settings, id, &url, used, remember)
        }
        EntryType::Sumo => {
            let query = if let Some(resolved) = &resolved_result {
                resolved
                    .primary()
                    .map(|o| o.text.clone())
                    .unwrap_or_default()
            } else {
                kb.pick(id, PrintMode::Payload).map_err(io_error)?
            };
            let range = if let Some(resolved) = &resolved_result {
                resolved
                    .get(&varres::Target::Range)
                    .map(|o| o.text.clone())
                    .or_else(|| {
                        if let TypeFields::Sumo { range } = entry.fields() {
                            range.clone()
                        } else {
                            None
                        }
                    })
            } else if let TypeFields::Sumo { range } = entry.fields() {
                range.clone()
            } else {
                None
            };

            let Some(base_url) = settings.sumo_base_url().filter(|u| !u.trim().is_empty()) else {
                eprintln!(
                    "kb: Sumo deployment URL is not configured. Set `sumo.base_url` in config or via `kb config set sumo.base_url <url>`. Copying query to clipboard instead."
                );
                let outcome = clipboard::copy(
                    platform,
                    &ClipboardSettings::from_settings(settings),
                    &query,
                    Place::Cli,
                );
                if outcome.sent() {
                    finish_use(kb, settings, id, used, remember);
                }
                return Ok(ExitCode::SUCCESS);
            };

            let link = kb_core::console::ConsoleService::build_sumo_link(
                base_url,
                &query,
                range.as_deref(),
            )
            .map_err(|e| CliError::new("usage", e.to_string()))?;

            if !link.query_included {
                eprintln!(
                    "kb: warning: URL exceeds 8,000 characters; copying query to clipboard and opening search console without query"
                );
                let _ = clipboard::copy(
                    platform,
                    &ClipboardSettings::from_settings(settings),
                    &query,
                    Place::Cli,
                );
            }

            execute_open_url(platform, kb, settings, id, &link.url, used, remember)
        }
        EntryType::Cw => {
            let region = kb_core::console::ConsoleService::resolve_aws_region(
                &entry,
                settings,
                &kb_core::console::OsEnv,
            )
            .map_err(|e| CliError::new("usage", e.to_string()))?;

            let query = if let Some(resolved) = &resolved_result {
                resolved
                    .primary()
                    .map(|o| o.text.clone())
                    .unwrap_or_default()
            } else {
                kb.pick(id, PrintMode::Payload).map_err(io_error)?
            };
            let range = if let Some(resolved) = &resolved_result {
                resolved
                    .get(&varres::Target::Range)
                    .map(|o| o.text.clone())
                    .or_else(|| {
                        if let TypeFields::Cw { range, .. } = entry.fields() {
                            range.clone()
                        } else {
                            None
                        }
                    })
            } else if let TypeFields::Cw { range, .. } = entry.fields() {
                range.clone()
            } else {
                None
            };

            let mut resolved_log_groups = Vec::new();
            if let TypeFields::Cw { log_groups, .. } = entry.fields() {
                for (i, lg) in log_groups.iter().enumerate() {
                    if let Some(r) = resolved_result
                        .as_ref()
                        .and_then(|res| res.get(&varres::Target::LogGroup(i)))
                    {
                        resolved_log_groups.push(r.text.clone());
                    } else {
                        resolved_log_groups.push(lg.clone());
                    }
                }
            }

            let link = kb_core::console::ConsoleService::build_cw_link(
                &region,
                &resolved_log_groups,
                &query,
                range.as_deref(),
            )
            .map_err(|e| CliError::new("usage", e.to_string()))?;

            if !link.query_included {
                eprintln!(
                    "kb: warning: URL exceeds 8,000 characters; copying query to clipboard and opening CloudWatch console without query"
                );
                let _ = clipboard::copy(
                    platform,
                    &ClipboardSettings::from_settings(settings),
                    &query,
                    Place::Cli,
                );
            }

            execute_open_url(platform, kb, settings, id, &link.url, used, remember)
        }
        EntryType::Code => {
            eprintln!("kb: code open is not implemented in this phase");
            Ok(ExitCode::from(1))
        }
        _ => {
            eprintln!("kb: {} entries do not have an open action", ty.name());
            Ok(ExitCode::from(1))
        }
    }
}

fn execute_open_url(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    url: &str,
    used: &[varcli::Used],
    remember: bool,
) -> Result<ExitCode, CliError> {
    match opener::open_url(platform, settings.opener_command(), url) {
        Ok(Opened::Launched) => {
            finish_use(kb, settings, id, used, remember);
            Ok(ExitCode::SUCCESS)
        }
        Ok(Opened::Printed(printed_url)) => {
            println!("{printed_url}");
            let _ = clipboard::copy(
                platform,
                &ClipboardSettings::from_settings(settings),
                &printed_url,
                Place::Cli,
            );
            finish_use(kb, settings, id, used, remember);
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("kb: error opening URL: {e}");
            Ok(ExitCode::from(1))
        }
    }
}

/// `kb run <id|query>`.
pub fn run_run(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    text: &str,
    opts: &RunOpts,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(io_error)?;
    match resolve_or_report(platform, &mut kb, text, opts.use_opts.json)? {
        Next::Entry(id) => deliver_run(platform, &kb, &loaded.settings, &id, opts),
        Next::Choose => {
            if !opts.use_opts.flags.is_empty() || opts.yes {
                eprintln!("kb: --var and --yes are ignored when you choose in the UI");
            }
            let post = PostPick::Run(RunOpts {
                action: opts.action.clone(),
                yes: false,
                force: opts.force,
                use_opts: UseOpts {
                    flags: Vec::new(),
                    yes: false,
                    ..opts.use_opts.clone()
                },
            });
            run_ui(platform, root, loaded, text, &post, false)
        }
        Next::Done(code) => Ok(code),
    }
}

pub fn deliver_run(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    opts: &RunOpts,
) -> Result<ExitCode, CliError> {
    let entry = kb.entry(id).map_err(io_error)?;
    let plan = varres::plan_vars(&entry);
    varcli::check_flag_names(&plan, &opts.use_opts.flags)?;

    let (values, used) = if plan.is_empty() {
        (varres::Values::new(), Vec::new())
    } else {
        let (memory, _) = kb.remembered(id);
        let mut tty = if opts.yes || !platform.has_terminal() {
            None
        } else {
            TtyAsk::open(platform)
        };
        let ask = tty.as_mut().map(|t| t as &mut dyn Ask);
        let env = |name: &str| std::env::var(name).ok();
        match varcli::collect(&plan, &opts.use_opts.flags, &env, Some(&memory), ask)? {
            Collected::Stop(code) => return Ok(code),
            Collected::Ready(values, report) => (values, report),
        }
    };

    deliver_run_values_map(platform, kb, settings, id, &values, &used, opts)
}

fn deliver_run_values(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    values: Vec<(String, kb_core::varres::Value, kb_core::varres::Source)>,
    opts: &RunOpts,
) -> Result<ExitCode, CliError> {
    let entry = kb.entry(id).map_err(io_error)?;
    let plan = varres::plan_vars(&entry);
    let mut map = varres::Values::new();
    let mut used = Vec::new();
    for (name, value, source) in values {
        if let Some(p) = plan.var(&name) {
            used.push(varcli::Used {
                name: name.clone(),
                kind: p.var.kind,
                source,
                text: (!value.is_secret()).then(|| value.reveal().to_string()),
            });
            map.insert(name, value);
        }
    }
    deliver_run_values_map(platform, kb, settings, id, &map, &used, opts)
}

fn deliver_run_values_map(
    platform: &impl Platform,
    kb: &Kb,
    settings: &Settings,
    id: &str,
    values: &varres::Values,
    used: &[varcli::Used],
    opts: &RunOpts,
) -> Result<ExitCode, CliError> {
    let entry = kb.entry(id).map_err(io_error)?;

    let custom_action = if let Some(ref action_name) = opts.action {
        let ty = entry.entry_type().map(|t| t.name()).unwrap_or("");
        let action = settings
            .custom_actions()
            .iter()
            .find(|a| a.entry_type == ty && a.name == *action_name)
            .ok_or_else(|| {
                CliError::new(
                    "usage",
                    format!("action '{action_name}' not found for type '{ty}'"),
                )
            })?;
        Some(action)
    } else {
        if entry.entry_type() != Some(EntryType::Cmd) {
            let ty_name = entry.entry_type().map(|t| t.name()).unwrap_or("unknown");
            return Err(CliError::new(
                "usage",
                format!("cannot run entry '{id}' of type '{ty_name}' (expected cmd)"),
            ));
        }
        None
    };

    let run_plan = kb_core::run::RunService::build_run_plan(
        &entry,
        values,
        custom_action,
        settings.run_destructive_patterns(),
    )
    .map_err(|e| match e {
        kb_core::run::RunPlanError::MissingCwd(path) => CliError::new(
            "usage",
            format!("working directory does not exist: {}", path.display()),
        ),
        kb_core::run::RunPlanError::MissingPlaceholderField(f) => {
            CliError::new("usage", format!("missing field in template: {{{{{f}}}}}"))
        }
        kb_core::run::RunPlanError::VarResolution(msg) => CliError::new("usage", msg),
    })?;

    if opts.yes {
        if run_plan.requires_typed_run && !opts.force {
            return Err(CliError::new(
                "usage",
                "command matches destructive patterns; use --yes --force to execute non-interactively",
            ));
        }
        eprintln!("kb: running: {}", run_plan.resolved_command);
    } else {
        if !platform.has_terminal() {
            return Err(CliError::new(
                "usage",
                "cannot prompt for confirmation without a terminal; use --yes to run non-interactively",
            ));
        }

        eprintln!("Command: {}", run_plan.resolved_command);
        eprintln!("Shell:   {}", run_plan.shell);
        if let Some(ref cwd) = run_plan.cwd {
            eprintln!("Cwd:     {}", cwd.display());
        }
        if !run_plan.secret_env_vars.is_empty() {
            let secret_names: Vec<_> = run_plan
                .secret_env_vars
                .iter()
                .map(|s| s.env_key.as_str())
                .collect();
            eprintln!("Secrets: {}", secret_names.join(", "));
        }
        for warn in &run_plan.destructive_warnings {
            eprintln!("WARNING: Destructive pattern matched: {}", warn.description);
        }

        let mut tty =
            TtyAsk::open(platform).ok_or_else(|| CliError::new("io", "cannot open terminal"))?;

        if run_plan.requires_typed_run {
            let answer = tty
                .line("This command matched destructive patterns. Type 'run' to execute: ")
                .map_err(|e| CliError::new("io", e.to_string()))?;
            match answer {
                Some(ans) if ans.trim() == "run" => {}
                _ => {
                    eprintln!("kb: run cancelled");
                    return Ok(ExitCode::from(130));
                }
            }
        } else {
            let answer = tty
                .line("Run this command? [y/N]: ")
                .map_err(|e| CliError::new("io", e.to_string()))?;
            match answer {
                Some(ans)
                    if ans.trim().eq_ignore_ascii_case("y")
                        || ans.trim().eq_ignore_ascii_case("yes") => {}
                _ => {
                    eprintln!("kb: run cancelled");
                    return Ok(ExitCode::from(130));
                }
            }
        }
    }

    let env_tuples: Vec<(String, String)> = run_plan
        .secret_env_vars
        .into_iter()
        .map(|s| (s.env_key, s.secret_value))
        .collect();

    let res = platform
        .run_interactive_command(
            &run_plan.shell,
            &run_plan.resolved_command,
            run_plan.cwd.as_deref(),
            &env_tuples,
        )
        .map_err(|e| CliError::new("io", format!("failed to run command: {e}")))?;

    finish_use(kb, settings, id, used, opts.use_opts.remember);

    let code = u8::try_from(res.exit_code).unwrap_or(1);
    Ok(ExitCode::from(code))
}

/// `kb copy --raw <id|query>`: the payload as it is in the file, placeholders included.
pub fn run_copy_raw(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    text: &str,
    json: bool,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(io_error)?;
    match resolve_or_report(platform, &mut kb, text, json)? {
        Next::Entry(id) => {
            let outcome = copy_entry(platform, &kb, &loaded.settings, &id)?;
            Ok(report_copy(&outcome, &id, json, None))
        }
        Next::Choose => run_ui(platform, root, loaded, text, &PostPick::Raw, false),
        Next::Done(code) => Ok(code),
    }
}

/// `kb vars <id|query>`.
pub fn run_vars(
    platform: &impl Platform,
    root: &Path,
    text: &str,
    json: bool,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(io_error)?;
    match resolve_or_report(platform, &mut kb, text, json)? {
        Next::Entry(id) => {
            let entry = kb.entry(&id).map_err(io_error)?;
            let plan = varres::plan_vars(&entry);
            let (memory, _) = kb.remembered(&id);
            let env = |name: &str| std::env::var(name).ok();
            write_stdout(&varcli::vars_report(&plan, &id, &env, Some(&memory), json));
            Ok(ExitCode::SUCCESS)
        }
        Next::Choose => {
            eprintln!("kb: several close matches; use an exact ID");
            Ok(ExitCode::from(1))
        }
        Next::Done(code) => Ok(code),
    }
}

/// `kb show <id|query>`.
pub fn run_show(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    text: &str,
    options: ShowOptions,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(io_error)?;
    let json = options.mode == ShowMode::Json;
    match resolve_or_report(platform, &mut kb, text, json)? {
        Next::Entry(id) => {
            write_stdout(&show_entry(platform, &kb, &loaded.settings, &id, options)?);
            Ok(ExitCode::SUCCESS)
        }
        Next::Choose => run_ui(
            platform,
            root,
            loaded,
            text,
            &PostPick::Show(options),
            false,
        ),
        Next::Done(code) => Ok(code),
    }
}

/// What Enter does in a UI opened by plain `kb`: `--print` wins, then `tui.enter`.
pub fn post_pick_for(print: Option<PrintMode>, settings: &Settings) -> PostPick {
    match (print, settings.tui_enter()) {
        (Some(mode), _) => PostPick::Print(mode),
        (None, EnterAction::Print) => PostPick::Print(PrintMode::Payload),
        (None, EnterAction::Copy | EnterAction::Primary) => PostPick::Copy,
    }
}

/// Opens the UI with `query` in the search line, then does `post` with the entry picked in it.
/// `report_config` prints the config warnings after the UI has left the screen (the commands that
/// open the UI themselves printed them before).
pub fn run_ui(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    query: &str,
    post: &PostPick,
    report_config: bool,
) -> Result<ExitCode, CliError> {
    let options = Options {
        mouse: loaded.settings.tui_mouse(),
        theme: loaded.settings.theme(),
        clipboard: ClipboardSettings::from_settings(&loaded.settings),
        editor: loaded.settings.editor().map(str::to_string),
        settings: loaded.settings.clone(),
        config_problems: loaded.warnings.len(),
        fill_variables: matches!(
            post,
            PostPick::Copy
                | PostPick::Print(PrintMode::Payload)
                | PostPick::Use(_)
                | PostPick::Open(_)
                | PostPick::Run(_)
        ),
        remember_vars: loaded.settings.vars_remember()
            && !matches!(post, PostPick::Use(o) if !o.remember),
        // Timing tests set this so the background refresh does not add noise.
        refresh_timer: std::env::var_os("KB_TUI_NO_REFRESH_TIMER").is_none(),
        path_template: loaded.settings.path_template(),
        highlight: true,
        start_add: None,
    };
    let outcome = tui::run(platform, root, query, options).map_err(|e| CliError::new("io", e));
    if report_config {
        for warning in &loaded.warnings {
            eprintln!("kb: {warning}");
        }
    }
    match outcome? {
        Outcome::Picked { id } => run_pick(platform, root, loaded, &id, post),
        Outcome::Resolved { id, values, print } => {
            if let PostPick::Open(open_opts) = post {
                let kb = Kb::open(root).map_err(io_error)?;
                return deliver_open_values(
                    platform,
                    &kb,
                    &loaded.settings,
                    &id,
                    values,
                    open_opts,
                );
            }
            if let PostPick::Run(run_opts) = post {
                let kb = Kb::open(root).map_err(io_error)?;
                return deliver_run_values(platform, &kb, &loaded.settings, &id, values, run_opts);
            }
            let mut opts = match post {
                PostPick::Use(o) => o.clone(),
                _ => UseOpts {
                    remember: true,
                    ..UseOpts::default()
                },
            };
            opts.print |= print || matches!(post, PostPick::Print(PrintMode::Payload));
            let kb = Kb::open(root).map_err(io_error)?;
            deliver_values(platform, &kb, &loaded.settings, &id, values, &opts)
        }
        Outcome::Added { id } => {
            println!("{id}");
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Exit(code) => Ok(ExitCode::from(u8::try_from(code).unwrap_or(2))),
    }
}

/// `kb add` without `--yes`: the UI opens on the add form for `draft`. The new ID is printed after
/// the terminal is restored; cancelling prints nothing and exits 130.
pub fn run_add_ui(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    draft: kb_core::author::Draft,
    unavailable: Option<String>,
) -> Result<ExitCode, CliError> {
    let options = Options {
        mouse: loaded.settings.tui_mouse(),
        theme: loaded.settings.theme(),
        clipboard: ClipboardSettings::from_settings(&loaded.settings),
        editor: loaded.settings.editor().map(str::to_string),
        settings: loaded.settings.clone(),
        config_problems: loaded.warnings.len(),
        fill_variables: false,
        remember_vars: false,
        refresh_timer: false,
        path_template: loaded.settings.path_template(),
        highlight: true,
        start_add: Some(Box::new(crate::tui::app::StartAdd { draft, unavailable })),
    };
    match tui::run(platform, root, "", options).map_err(|e| CliError::new("io", e))? {
        Outcome::Added { id } => {
            println!("{id}");
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Exit(code) => Ok(ExitCode::from(u8::try_from(code).unwrap_or(2))),
        Outcome::Picked { .. } | Outcome::Resolved { .. } => Ok(ExitCode::from(130)),
    }
}

// ---- kb config ----

fn describe_source(source: &Source) -> String {
    match source {
        Source::Default => "default".to_string(),
        Source::File(_) => "file".to_string(),
        Source::Env(name) => format!("env {name}"),
        Source::Flag(flag) => format!("flag {flag}"),
    }
}

/// A setting's value as JSON: booleans and integers keep their type.
fn typed_value(kind: Kind, value: &str) -> Value {
    match kind {
        Kind::Bool => json!(value == "true"),
        Kind::Int => value
            .parse::<u64>()
            .map_or_else(|_| json!(value), |n| json!(n)),
        _ => json!(value),
    }
}

fn setting_json(key: &str, kind: Kind, r: &Resolved<String>) -> Value {
    let file = match &r.source {
        Source::File(p) => Some(p.display().to_string()),
        _ => None,
    };
    json!({
        "key": key,
        "value": typed_value(kind, &r.value),
        "source": r.source_name(),
        "file": file,
        "line": r.line,
    })
}

fn config_write_path(platform: &impl Platform) -> Result<std::path::PathBuf, CliError> {
    platform.config_write_path().ok_or_else(|| {
        CliError::new(
            "config",
            "cannot find a home directory to keep the config file in",
        )
    })
}

/// `kb config init`: write the commented template. Never overwrites.
pub fn config_init(platform: &impl Platform, json: bool) -> Result<ExitCode, CliError> {
    let path = config_write_path(platform)?;
    if path.exists() {
        eprintln!("kb: config file already exists: {}", path.display());
        return Ok(ExitCode::from(1));
    }
    config::write_atomic(&path, &config::render_template())
        .map_err(|e| CliError::new("io", format!("cannot write {}: {e}", path.display())))?;
    if json {
        println!(
            "{}",
            json!({ "path": path.display().to_string(), "created": true })
        );
    } else {
        println!("{}", path.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// `kb config path [--read]`: where the file is written, or every place that is read.
pub fn config_path(
    platform: &impl Platform,
    loaded: &Loaded,
    read: bool,
    json: bool,
) -> Result<ExitCode, CliError> {
    let write = config_write_path(platform)?;
    let candidates = platform.config_files();
    let in_use = |p: &std::path::PathBuf| loaded.file.as_ref() == Some(p);
    if json {
        let rows: Vec<Value> = candidates
            .iter()
            .map(|p| json!({ "path": p.display().to_string(), "exists": p.is_file(), "in_use": in_use(p) }))
            .collect();
        println!(
            "{}",
            json!({ "path": write.display().to_string(), "candidates": rows })
        );
    } else if read {
        for p in &candidates {
            let mark = if in_use(p) {
                "  (in use)"
            } else if p.is_file() {
                "  (exists, not used)"
            } else {
                ""
            };
            println!("{}{mark}", p.display());
        }
    } else {
        println!("{}", write.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// `kb config get <key>`.
pub fn config_get(loaded: &Loaded, key: &str, json: bool) -> Result<ExitCode, CliError> {
    let (Some(r), Some(s)) = (
        loaded.settings.get(key),
        SETTINGS.iter().find(|s| s.key == key),
    ) else {
        return Err(CliError::new(
            "usage",
            ConfigEditError::UnknownKey(key.to_string()).to_string(),
        ));
    };
    if json {
        println!("{}", setting_json(key, s.kind, r));
    } else {
        println!("{}", r.value);
    }
    Ok(ExitCode::SUCCESS)
}

/// `kb config list`.
pub fn config_list(loaded: &Loaded, json: bool) -> Result<ExitCode, CliError> {
    if json {
        let rows: Vec<Value> = SETTINGS
            .iter()
            .filter_map(|s| {
                loaded
                    .settings
                    .get(s.key)
                    .map(|r| setting_json(s.key, s.kind, r))
            })
            .collect();
        println!("{}", Value::Array(rows));
    } else {
        for (key, r) in loaded.settings.iter() {
            let shown = if r.value.is_empty() {
                "\"\"".to_string()
            } else {
                r.value.clone()
            };
            println!("{key} = {shown}  ({})", describe_source(&r.source));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The settings that came from the legacy `config.yml`, as the text of a `config.toml`.
fn legacy_as_toml(loaded: &Loaded) -> String {
    let mut text = String::new();
    for (key, r) in loaded.settings.iter() {
        if matches!(r.source, Source::File(_))
            && let Ok(next) = config::set_value(&text, key, &r.value)
        {
            text = next;
        }
    }
    text
}

/// `kb config set <key> <value>`: edits the file in place, keeping comments and order.
pub fn config_set(
    platform: &impl Platform,
    loaded: &Loaded,
    key: &str,
    value: &str,
    json: bool,
) -> Result<ExitCode, CliError> {
    let path = config_write_path(platform)?;
    let read = |p: &std::path::Path| match std::fs::read_to_string(p) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CliError::new(
            "io",
            format!("cannot read {}: {e}", p.display()),
        )),
    };
    // Writing a new file where another one was in use would hide that file's settings, so the
    // new file starts as a copy of it.
    let (base, moved_from) = match read(&path)? {
        Some(t) => (t, None),
        // the pre-M4 YAML file: its settings move into the new file, because once config.toml
        // exists kb stops reading config.yml
        None if loaded.legacy => (legacy_as_toml(loaded), loaded.file.clone()),
        None => match loaded.file.as_ref().filter(|f| **f != path) {
            Some(other) => (read(other)?.unwrap_or_default(), Some(other.clone())),
            None => (String::new(), None),
        },
    };
    let edited = config::set_value(&base, key, value).map_err(|e| match e {
        ConfigEditError::NotToml(_) => CliError::new("config", format!("{}: {e}", path.display())),
        _ => CliError::new("usage", e.to_string()),
    })?;
    config::write_atomic(&path, &edited)
        .map_err(|e| CliError::new("io", format!("cannot write {}: {e}", path.display())))?;
    if let Some(other) = moved_from {
        eprintln!(
            "kb: copied the settings of {} to {}, which kb reads first",
            other.display(),
            path.display()
        );
    }
    match loaded.settings.get(key).map(|r| &r.source) {
        Some(Source::Env(name)) => eprintln!("kb: {name} is set and overrides this value"),
        Some(Source::Flag(flag)) => eprintln!("kb: {flag} is given and overrides this value"),
        _ => {}
    }
    if json {
        println!(
            "{}",
            json!({ "key": key, "value": value, "path": path.display().to_string() })
        );
    }
    Ok(ExitCode::SUCCESS)
}

// ---- kb doctor ----

/// A string that is unlikely to be on the clipboard already.
fn test_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "kb doctor test {:08x}",
        (nanos ^ u128::from(std::process::id()) << 32) as u32
    )
}

/// Copies a test string with the method kb would use and asks the user to paste it back.
fn copy_test(platform: &impl Platform, loaded: &Loaded) -> Result<Check, CliError> {
    use std::io::BufRead;
    if !std::io::stdin().is_terminal() {
        return Err(CliError::new(
            "usage",
            "--copy-test needs a terminal on stdin: you paste the test text back there",
        ));
    }
    let token = test_token();
    let outcome = clipboard::copy(
        platform,
        &ClipboardSettings::from_settings(&loaded.settings),
        &token,
        Place::Ui,
    );
    for warning in outcome.warnings() {
        eprintln!("{warning}");
    }
    if !outcome.sent() {
        let mut check = Check {
            id: "copy-test",
            status: Status::Fail,
            summary: format!(
                "nothing could carry the test text: {}",
                outcome.message_short()
            ),
            hint: Some("see the clipboard check above".into()),
            details: Vec::new(),
        };
        check.details.extend(
            outcome
                .attempts
                .iter()
                .map(|a| format!("{} skipped: {}", a.method.label(), a.reason)),
        );
        return Ok(check);
    }
    eprintln!("{}", outcome.message_short());
    eprintln!("Paste it here and press Enter:");
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    let pasted = line.trim();
    Ok(if pasted == token {
        Check {
            id: "copy-test",
            status: Status::Pass,
            summary: "the pasted text arrived".into(),
            hint: None,
            details: Vec::new(),
        }
    } else {
        Check {
            id: "copy-test",
            status: Status::Fail,
            summary: format!("the pasted text did not match what was copied (got {pasted:?})"),
            hint: Some(
                "the clipboard may belong to another session; see the clipboard check above".into(),
            ),
            details: Vec::new(),
        }
    })
}

/// `kb doctor`.
pub fn run_doctor(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    with_copy_test: bool,
    json: bool,
) -> Result<ExitCode, CliError> {
    let mut report = doctor::run(platform, root, loaded);
    if with_copy_test {
        report.checks.push(copy_test(platform, loaded)?);
    }
    if json {
        println!("{}", report.to_json());
    } else {
        let fancy = crate::color::enabled();
        print!("{}", report.render(fancy));
    }
    Ok(if report.ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::fake::{FakePlatform, FakeTool, mac_env};

    #[test]
    fn copy_json_has_the_documented_shape() {
        let p = FakePlatform::new(mac_env(&[])).with_tool("pbcopy", FakeTool::Ok);
        let o = clipboard::copy(&p, &ClipboardSettings::default(), "a\nb\n", Place::Cli);
        let doc = copy_json(&o, "x/y");
        assert_eq!(doc["id"], "x/y");
        assert_eq!(doc["method"], "pbcopy");
        assert_eq!(doc["confirmed"], true);
        assert_eq!(
            (doc["bytes"].as_u64(), doc["lines"].as_u64()),
            (Some(3), Some(2))
        );
        assert_eq!(doc["sent"], true);
        assert!(doc.get("text").is_none() && doc.get("refused").is_none());
        let s = ClipboardSettings {
            choice: kb_core::config::ClipboardChoice::Stdout,
            ..ClipboardSettings::default()
        };
        let o = clipboard::copy(&p, &s, "text", Place::Cli);
        let doc = copy_json(&o, "x/y");
        assert_eq!(
            (doc["method"].as_str(), doc["text"].as_str()),
            (Some("stdout"), Some("text"))
        );
    }

    #[test]
    fn copy_json_names_a_refusal() {
        let p = FakePlatform::new(mac_env(&[("SSH_TTY", "t")]));
        let s = ClipboardSettings {
            osc52_max_bytes: 2,
            ..ClipboardSettings::default()
        };
        let o = clipboard::copy(&p, &s, "abcdef", Place::Cli);
        let doc = copy_json(&o, "x/y");
        assert_eq!(doc["sent"], false);
        assert!(doc["method"].is_null());
        assert_eq!(doc["refused"]["limit"], 2);
        assert_eq!(doc["attempts"][0]["method"], "terminal (OSC 52)");
    }

    #[test]
    fn enter_follows_the_flag_then_the_setting() {
        let default =
            kb_core::config::load(&[], &|_| None, &kb_core::config::Flags::default()).settings;
        assert_eq!(post_pick_for(None, &default), PostPick::Copy);
        assert_eq!(
            post_pick_for(Some(PrintMode::Id), &default),
            PostPick::Print(PrintMode::Id)
        );
    }
}
