//! The full-screen terminal UI.

pub mod action_view;
pub mod add;
pub mod add_view;
pub mod app;
pub mod code_view;
pub mod conflict_view;
pub mod form;
pub mod form_view;
pub mod header;
pub mod highlight;
pub mod history_view;
pub mod inline_form;
pub mod inline_picker;
pub mod input;
pub mod keys;
pub mod layout;
pub mod markdown;
pub mod preview;
pub mod run_modal;
pub mod terminal;
pub mod theme;
pub mod view;
pub mod worker;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use kb_core::config::Settings;
use kb_core::console::{ConsoleService, OsEnv};
use kb_core::entry::{EntryType, TypeFields};
use kb_core::entry_json::PrintMode;
use kb_core::search::Kb;
use kb_core::varres::{self, Source, Value};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::terminal::size;

use crate::platform::clipboard::{self, ClipboardSettings, Place};
use crate::platform::{Opened, Platform, opener};
use app::{Action, AppState, CopyReport, Effect, Options, RefreshInfo};
use form::{FormSpec, Submission, Then};
use run_modal::RunModalState;
use terminal::{Signals, TerminalGuard, exit_code_for_signal};
use theme::{Theme, ThemeEnv};
use worker::{Reply, Request, Worker};

/// How the TUI ended.
#[derive(Debug)]
pub enum Outcome {
    /// An entry was picked; the caller copies or prints it (and records the use).
    Picked { id: String },
    /// An entry with variables was picked and its form filled in; the caller resolves it with
    /// these values, then copies or prints it. `print` is Alt-Enter.
    Resolved {
        id: String,
        values: Vec<(String, Value, Source)>,
        print: bool,
    },
    /// `kb add`'s form saved a new entry.
    Added { id: String },
    /// Cancelled or signalled; nothing to print.
    Exit(i32),
}

/// Replaces the home directory prefix with `~`.
pub fn display_root(root: &Path, home: Option<&Path>) -> String {
    // The root is canonical; the home directory may reach it through a symlink (macOS /var).
    let home: Option<PathBuf> = home.map(|h| h.canonicalize().unwrap_or_else(|_| h.to_path_buf()));
    match home.as_deref().and_then(|h| root.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => root.display().to_string(),
    }
}

fn first_error_text(findings: &[kb_core::lint::Finding]) -> Option<String> {
    findings
        .first()
        .map(|f| format!("{}:{} {} {}", f.path, f.line, f.code, f.message))
}

/// Runs the UI until the user picks, cancels or a signal arrives. Nothing is written to stdout.
pub fn run(
    platform: &impl Platform,
    root: &Path,
    initial_query: &str,
    options: Options,
) -> Result<Outcome, String> {
    let tty = platform
        .open_tty()
        .map_err(|e| format!("cannot open the terminal: {e}"))?;
    let (w, h) = size().map_err(|e| format!("cannot read the terminal size: {e}"))?;
    let theme = Theme::resolve(options.theme, &ThemeEnv::detect(&platform.env()));
    let mut ui_kb = Kb::open(root).map_err(|e| e.to_string())?;
    let mut options = options;
    options.highlight = theme.is_color();
    let mouse = options.mouse;
    let editor = options.editor.clone();
    let clipboard_settings = options.clipboard.clone();
    let settings = options.settings.clone();
    let mut guard = TerminalGuard::enter(tty, mouse).map_err(|e| e.to_string())?;
    let signals = Signals::register();
    let backend = CrosstermBackend::new(guard.writer().map_err(|e| e.to_string())?);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
    let home = platform.home_dir();
    let mut state = AppState::new(
        options,
        (w, h),
        initial_query,
        display_root(ui_kb.root(), home.as_deref()),
    );
    if let Some(start) = state.options.start_add.take() {
        state.open_start_add(*start, SystemTime::now());
    }
    let mut worker = Worker::spawn(root.to_path_buf(), theme.is_color());
    let git_worker = header::GitWorker::spawn(root.to_path_buf());
    let mut pending: Vec<Effect> = state.start();
    pending.push(Effect::GitStatus);
    if settings.sync_fetch_on_start() {
        pending.push(Effect::Fetch);
    }
    let mut picked: Option<String> = None;
    let mut resolved: Option<Submission> = None;
    let mut exit: Option<i32> = None;
    let mut added: Option<String> = None;
    let mut dirty = true;
    let mut first = true;

    loop {
        for effect in std::mem::take(&mut pending) {
            match effect {
                Effect::Refresh => {
                    let _ = worker.tx.send(Request::Refresh);
                }
                Effect::LoadHistory(id) => {
                    let git = crate::platform::SystemGitRunner;
                    let result = kb_core::history::HistoryEngine::new(ui_kb.root(), &git)
                        .log(&id)
                        .map(|h| Box::new(history_view::HistoryState::new(h)))
                        .map_err(|e| e.to_string());
                    pending.extend(state.update(Action::HistoryLoaded(result), Instant::now()));
                    dirty = true;
                }
                Effect::LoadHistoryDiff { id, rev } => {
                    let git = crate::platform::SystemGitRunner;
                    let result = kb_core::history::HistoryEngine::new(ui_kb.root(), &git)
                        .diff(&id, &rev)
                        .map_err(|e| e.to_string());
                    pending.extend(state.update(Action::HistoryDiff(result), Instant::now()));
                    dirty = true;
                }
                Effect::RestoreRevision { id, rev } => {
                    let signing = kb_core::git::commit::is_signing_configured(
                        ui_kb.root(),
                        &crate::platform::SystemGitRunner,
                    );
                    if signing {
                        guard.suspend();
                    }
                    let result = crate::commands::history::restore_entry(
                        ui_kb.root(),
                        &state.options.settings,
                        &id,
                        &rev,
                    )
                    .map(|(short, warning)| match warning {
                        Some(w) => format!("Restored {id} from {short}; {w}"),
                        None => format!("Restored {id} from {short}"),
                    });
                    if signing {
                        let _ = guard.resume();
                        if let Ok(size) = terminal.size() {
                            let _ = terminal.resize(ratatui::layout::Rect::new(
                                0,
                                0,
                                size.width,
                                size.height,
                            ));
                        }
                    }
                    pending.extend(state.update(Action::HistoryRestored(result), Instant::now()));
                    dirty = true;
                }
                Effect::GitStatus => {
                    let _ = git_worker.tx.send(header::GitRequest::Status);
                }
                Effect::Fetch => {
                    let _ = git_worker.tx.send(header::GitRequest::Fetch);
                }
                Effect::Search {
                    generation,
                    query,
                    limit,
                } => {
                    let _ = worker.tx.send(Request::Search {
                        generation,
                        query,
                        limit,
                    });
                }
                Effect::LoadPreview {
                    generation,
                    id,
                    modified,
                    use_count,
                } => {
                    let _ = worker.tx.send(Request::Preview {
                        generation,
                        id,
                        modified,
                        use_count,
                    });
                }
                Effect::Pick => {
                    let id = state.selected_id.clone();
                    match (&id, state.options.fill_variables) {
                        (Some(id), true) => match FormSpec::load(&ui_kb, id, &real_env) {
                            Ok(Some(spec)) => {
                                let a = Action::FormLoaded(Ok(Box::new(spec)), Then::Exit);
                                pending.extend(state.update(a, Instant::now()));
                                dirty = true;
                            }
                            Ok(None) => picked = Some(id.clone()),
                            Err(e) => {
                                let a = Action::FormLoaded(Err(e), Then::Exit);
                                pending.extend(state.update(a, Instant::now()));
                                dirty = true;
                            }
                        },
                        _ => picked = id,
                    }
                }
                Effect::Quit(code) => exit = Some(code),
                Effect::CopyPayload => {
                    let id = state.selected_id.clone();
                    let spec = id.as_ref().map(|id| FormSpec::load(&ui_kb, id, &real_env));
                    let action = match spec {
                        Some(Ok(Some(spec))) => Action::FormLoaded(Ok(Box::new(spec)), Then::Stay),
                        Some(Err(e)) => Action::FormLoaded(Err(e), Then::Stay),
                        _ => Action::CopyDone(copy_selected(
                            platform,
                            &clipboard_settings,
                            &state,
                            &ui_kb,
                        )),
                    };
                    pending.extend(state.update(action, Instant::now()));
                    dirty = true;
                }
                Effect::CopyTemplate => {
                    let mut report = copy_selected(platform, &clipboard_settings, &state, &ui_kb);
                    if report.sent {
                        report
                            .message
                            .push_str(" (template; variables not filled in)");
                    }
                    pending.extend(state.update(Action::CopyDone(report), Instant::now()));
                    dirty = true;
                }
                Effect::OpenEntry(id) => {
                    let entry = ui_kb.entry(&id);
                    if let Ok(entry) = &entry
                        && entry.entry_type() == Some(EntryType::Code)
                    {
                        match code_view::load_code_view_state(&ui_kb, &id, &settings, false) {
                            Ok(cv) => {
                                pending.extend(
                                    state
                                        .update(Action::OpenCodeView(Box::new(cv)), Instant::now()),
                                );
                            }
                            Err(e) => {
                                pending.extend(state.update(
                                    Action::CopyDone(CopyReport {
                                        message: format!("Code view error: {e}"),
                                        sent: false,
                                    }),
                                    Instant::now(),
                                ));
                            }
                        }
                        dirty = true;
                        continue;
                    }
                    let spec = FormSpec::load(&ui_kb, &id, &real_env);
                    let action = match spec {
                        Ok(Some(spec)) => Action::FormLoaded(Ok(Box::new(spec)), Then::Open),
                        Err(e) => Action::FormLoaded(Err(e), Then::Open),
                        _ => Action::CopyDone(open_selected(
                            platform,
                            &settings,
                            &clipboard_settings,
                            &ui_kb,
                            &id,
                            state.options.remember_vars,
                        )),
                    };
                    pending.extend(state.update(action, Instant::now()));
                    dirty = true;
                }
                Effect::OpenActionMenu(id) => {
                    let entry = ui_kb.entry(&id);
                    match entry {
                        Ok(entry) => {
                            let env = platform.env();
                            let is_ssh_without_display =
                                env.ssh && env.display == crate::platform::Display::None;
                            let actions = kb_core::action::ActionService::list_actions(
                                &entry, &settings, None,
                            );
                            let title = entry.title().unwrap_or(entry.id()).to_string();
                            let menu = action_view::ActionMenuState {
                                id: id.clone(),
                                title,
                                entry_type: entry.entry_type(),
                                actions,
                                has_local_clone: false,
                                is_ssh_without_display,
                            };
                            pending.extend(
                                state
                                    .update(Action::OpenActionMenu(Box::new(menu)), Instant::now()),
                            );
                        }
                        Err(e) => {
                            pending.extend(state.update(
                                Action::CopyDone(CopyReport {
                                    message: format!("Error loading entry '{id}': {e}"),
                                    sent: false,
                                }),
                                Instant::now(),
                            ));
                        }
                    }
                    dirty = true;
                }
                Effect::RunCustomAction { id, action_name } => {
                    let entry = ui_kb.entry(&id);
                    match entry {
                        Ok(entry) => {
                            let custom = settings
                                .custom_actions()
                                .iter()
                                .find(|a| a.name == action_name)
                                .cloned();
                            if let Some(custom) = custom {
                                let spec = FormSpec::load(&ui_kb, &id, &real_env);
                                let action = match spec {
                                    Ok(Some(spec)) => {
                                        Action::FormLoaded(Ok(Box::new(spec)), Then::Run)
                                    }
                                    Err(e) => Action::FormLoaded(Err(e), Then::Run),
                                    _ => {
                                        let values = kb_core::varres::Values::new();
                                        match kb_core::run::RunService::build_run_plan(
                                            &entry,
                                            &values,
                                            Some(&custom),
                                            settings.run_destructive_patterns(),
                                        ) {
                                            Ok(plan) => Action::OpenRunModal { id, plan },
                                            Err(e) => Action::CopyDone(CopyReport {
                                                message: format!("Cannot run custom action: {e}"),
                                                sent: false,
                                            }),
                                        }
                                    }
                                };
                                pending.extend(state.update(action, Instant::now()));
                            } else {
                                pending.extend(state.update(
                                    Action::CopyDone(CopyReport {
                                        message: format!("Custom action '{action_name}' not found"),
                                        sent: false,
                                    }),
                                    Instant::now(),
                                ));
                            }
                        }
                        Err(e) => {
                            pending.extend(state.update(
                                Action::CopyDone(CopyReport {
                                    message: format!("Error loading entry '{id}': {e}"),
                                    sent: false,
                                }),
                                Instant::now(),
                            ));
                        }
                    }
                    dirty = true;
                }
                Effect::OpenCodeTarget { id, target } => {
                    let now = Instant::now();
                    if let Some(cv) = &state.code_view {
                        let report = match target.as_str() {
                            "web" => {
                                if let Some(permalink) = &cv.permalink {
                                    match opener::open_url(
                                        platform,
                                        settings.opener_command(),
                                        permalink,
                                    ) {
                                        Ok(Opened::Launched) => {
                                            let _ = ui_kb.record_use(&id, SystemTime::now());
                                            CopyReport {
                                                message: "Opened in browser".into(),
                                                sent: true,
                                            }
                                        }
                                        Ok(Opened::Printed(url)) => {
                                            let outcome = clipboard::copy(
                                                platform,
                                                &clipboard_settings,
                                                &url,
                                                Place::Ui,
                                            );
                                            if outcome.sent() {
                                                let _ = ui_kb.record_use(&id, SystemTime::now());
                                            }
                                            CopyReport {
                                                message: "Copied URL (no display)".into(),
                                                sent: outcome.sent(),
                                            }
                                        }
                                        Err(e) => CopyReport {
                                            message: format!("Open failed: {e}"),
                                            sent: false,
                                        },
                                    }
                                } else {
                                    CopyReport {
                                        message: "No web permalink available".into(),
                                        sent: false,
                                    }
                                }
                            }
                            "vscode" => {
                                if platform.env().ssh {
                                    CopyReport {
                                        message: "Editor launch disabled over SSH".into(),
                                        sent: false,
                                    }
                                } else if let Some(clone_path) = &cv.clone_path {
                                    let abs_path = clone_path.join(&cv.path);
                                    let url =
                                        format!("vscode://file/{}:{}", abs_path.display(), cv.line);
                                    match opener::open_url(
                                        platform,
                                        settings.opener_command(),
                                        &url,
                                    ) {
                                        Ok(Opened::Launched) => {
                                            let _ = ui_kb.record_use(&id, SystemTime::now());
                                            CopyReport {
                                                message: "Opened in VS Code".into(),
                                                sent: true,
                                            }
                                        }
                                        Ok(_) => CopyReport {
                                            message: "Opened in VS Code".into(),
                                            sent: true,
                                        },
                                        Err(e) => CopyReport {
                                            message: format!("Open failed: {e}"),
                                            sent: false,
                                        },
                                    }
                                } else {
                                    CopyReport {
                                        message: "No local clone found".into(),
                                        sent: false,
                                    }
                                }
                            }
                            "idea" => {
                                if platform.env().ssh {
                                    CopyReport {
                                        message: "Editor launch disabled over SSH".into(),
                                        sent: false,
                                    }
                                } else if let Some(clone_path) = &cv.clone_path {
                                    let abs_path = clone_path.join(&cv.path);
                                    let url = format!(
                                        "idea://open?file={}&line={}",
                                        abs_path.display(),
                                        cv.line
                                    );
                                    match opener::open_url(
                                        platform,
                                        settings.opener_command(),
                                        &url,
                                    ) {
                                        Ok(Opened::Launched) => {
                                            let _ = ui_kb.record_use(&id, SystemTime::now());
                                            CopyReport {
                                                message: "Opened in IntelliJ".into(),
                                                sent: true,
                                            }
                                        }
                                        Ok(_) => CopyReport {
                                            message: "Opened in IntelliJ".into(),
                                            sent: true,
                                        },
                                        Err(e) => CopyReport {
                                            message: format!("Open failed: {e}"),
                                            sent: false,
                                        },
                                    }
                                } else {
                                    CopyReport {
                                        message: "No local clone found".into(),
                                        sent: false,
                                    }
                                }
                            }
                            _ => CopyReport {
                                message: format!("Unknown open target: {target}"),
                                sent: false,
                            },
                        };
                        pending.extend(state.update(Action::CopyDone(report), now));
                    }
                    dirty = true;
                }
                Effect::CopyCodeLocation(loc) => {
                    let outcome = clipboard::copy(platform, &clipboard_settings, &loc, Place::Ui);
                    let report = CopyReport {
                        message: if outcome.sent() {
                            format!("Copied location: {loc}")
                        } else {
                            format!("Copy failed: {}", outcome.message_short())
                        },
                        sent: outcome.sent(),
                    };
                    pending.extend(state.update(Action::CopyDone(report), Instant::now()));
                    dirty = true;
                }
                Effect::CopyCodePermalink(permalink) => {
                    let outcome =
                        clipboard::copy(platform, &clipboard_settings, &permalink, Place::Ui);
                    let report = CopyReport {
                        message: if outcome.sent() {
                            "Copied permalink".to_string()
                        } else {
                            format!("Copy failed: {}", outcome.message_short())
                        },
                        sent: outcome.sent(),
                    };
                    pending.extend(state.update(Action::CopyDone(report), Instant::now()));
                    dirty = true;
                }
                Effect::RepinCodeEntry { id, report } => {
                    let now = Instant::now();
                    let repin_res = (|| -> Result<(), String> {
                        let entry = ui_kb.entry(&id).map_err(|e| e.to_string())?;
                        let full_time = kb_core::author::render::rfc3339_utc(SystemTime::now());
                        let today = if full_time.len() >= 10 {
                            &full_time[..10]
                        } else {
                            "2026-10-10"
                        };
                        let file_path = std::path::Path::new(ui_kb.root()).join(entry.path());
                        let content =
                            std::fs::read_to_string(&file_path).map_err(|e| e.to_string())?;
                        let updated = kb_core::code::CodeService::repin_entry(
                            &file_path, &content, &report, today,
                        )
                        .map_err(|e| e.to_string())?;
                        std::fs::write(&file_path, updated).map_err(|e| e.to_string())?;
                        Ok(())
                    })();

                    match repin_res {
                        Ok(()) => {
                            let rel = std::path::PathBuf::from(format!("{id}.md"));
                            let is_signing = kb_core::git::commit::is_signing_configured(
                                ui_kb.root(),
                                &crate::platform::SystemGitRunner,
                            );
                            if is_signing {
                                guard.suspend();
                            }
                            let outcome = kb_core::git::commit::auto_commit_operation(
                                ui_kb.root(),
                                &crate::platform::SystemGitRunner,
                                &state.options.settings,
                                kb_core::git::commit::AutoCommitAction::Repin,
                                &id,
                                &[rel],
                            );
                            if is_signing {
                                let _ = guard.resume();
                                if let Ok(size) = terminal.size() {
                                    let _ = terminal.resize(ratatui::layout::Rect::new(
                                        0,
                                        0,
                                        size.width,
                                        size.height,
                                    ));
                                }
                            }
                            if let Ok(
                                kb_core::git::commit::AutoCommitOutcome::SkippedMissingIdentity {
                                    ..
                                },
                            ) = outcome
                            {
                                state.say(
                                    "Git identity unset: commit skipped",
                                    crate::tui::app::MessageKind::Warning,
                                    Instant::now(),
                                    true,
                                );
                            }
                            if let Ok(cv) =
                                code_view::load_code_view_state(&ui_kb, &id, &settings, false)
                            {
                                pending
                                    .extend(state.update(Action::OpenCodeView(Box::new(cv)), now));
                            }
                            pending.extend(state.update(
                                Action::CopyDone(CopyReport {
                                    message: "Re-pinned".to_string(),
                                    sent: true,
                                }),
                                now,
                            ));
                        }
                        Err(e) => {
                            pending.extend(state.update(
                                Action::CopyDone(CopyReport {
                                    message: format!("Re-pin failed: {e}"),
                                    sent: false,
                                }),
                                now,
                            ));
                        }
                    }
                    dirty = true;
                }
                Effect::FetchAndRecheckCode(id) => {
                    let now = Instant::now();
                    match code_view::load_code_view_state(&ui_kb, &id, &settings, true) {
                        Ok(cv) => {
                            pending.extend(state.update(Action::OpenCodeView(Box::new(cv)), now));
                            pending.extend(state.update(
                                Action::CopyDone(CopyReport {
                                    message: "Fetched".to_string(),
                                    sent: true,
                                }),
                                now,
                            ));
                        }
                        Err(e) => {
                            pending.extend(state.update(
                                Action::CopyDone(CopyReport {
                                    message: format!("Fetch failed: {e}"),
                                    sent: false,
                                }),
                                now,
                            ));
                        }
                    }
                    dirty = true;
                }
                Effect::RunEntry(id) => {
                    let entry = ui_kb.entry(&id);
                    match entry {
                        Ok(entry) => {
                            if entry.entry_type() != Some(EntryType::Cmd) {
                                pending.extend(state.update(
                                    Action::CopyDone(CopyReport {
                                        message: format!("cannot run entry '{id}': expected cmd"),
                                        sent: false,
                                    }),
                                    Instant::now(),
                                ));
                            } else {
                                let spec = FormSpec::load(&ui_kb, &id, &real_env);
                                let action = match spec {
                                    Ok(Some(spec)) => {
                                        Action::FormLoaded(Ok(Box::new(spec)), Then::Run)
                                    }
                                    Err(e) => Action::FormLoaded(Err(e), Then::Run),
                                    _ => {
                                        let values = kb_core::varres::Values::new();
                                        match kb_core::run::RunService::build_run_plan(
                                            &entry,
                                            &values,
                                            None,
                                            settings.run_destructive_patterns(),
                                        ) {
                                            Ok(plan) => Action::OpenRunModal { id, plan },
                                            Err(e) => Action::CopyDone(CopyReport {
                                                message: format!("cannot run: {e}"),
                                                sent: false,
                                            }),
                                        }
                                    }
                                };
                                pending.extend(state.update(action, Instant::now()));
                            }
                        }
                        Err(e) => {
                            pending.extend(state.update(
                                Action::CopyDone(CopyReport {
                                    message: format!("cannot load entry: {e}"),
                                    sent: false,
                                }),
                                Instant::now(),
                            ));
                        }
                    }
                    dirty = true;
                }
                Effect::ExecuteRun { id, plan } => {
                    guard.suspend();
                    let env_tuples: Vec<(String, String)> = plan
                        .secret_env_vars
                        .iter()
                        .map(|s| (s.env_key.clone(), s.secret_value.clone()))
                        .collect();
                    let res = platform.run_interactive_command(
                        &plan.shell,
                        &plan.resolved_command,
                        plan.cwd.as_deref(),
                        &env_tuples,
                        plan.stdin_payload.as_deref(),
                    );
                    let _ = guard.resume();
                    if let Ok(sz) = terminal.size() {
                        let _ =
                            terminal.resize(ratatui::layout::Rect::new(0, 0, sz.width, sz.height));
                    }
                    let res = res.unwrap_or(kb_core::run::RunResult {
                        exit_code: 1,
                        duration: Duration::ZERO,
                        killed_by_signal: None,
                    });
                    let _ = ui_kb.record_use(&id, SystemTime::now());
                    let now = Instant::now();
                    if res.exit_code == 0 && !settings.run_pause_after() {
                        state.run_modal = None;
                        pending.extend(state.update(
                            Action::CopyDone(CopyReport {
                                message: format!("Command completed in {:?}", res.duration),
                                sent: false,
                            }),
                            now,
                        ));
                    } else {
                        state.run_modal = Some(Box::new(RunModalState::Paused {
                            id,
                            exit_code: res.exit_code,
                            duration: res.duration,
                        }));
                    }
                    dirty = true;
                }
                Effect::SubmitForm(sub) => {
                    let now = Instant::now();
                    match recheck(&ui_kb, &sub) {
                        Recheck::Same => match sub.then {
                            // Alt-Enter prints and exits, whichever way the form was opened
                            Then::Exit => resolved = Some(*sub),
                            Then::Stay if sub.print => resolved = Some(*sub),
                            Then::Stay => {
                                let report = copy_resolved(
                                    platform,
                                    &clipboard_settings,
                                    &ui_kb,
                                    &sub,
                                    state.options.remember_vars,
                                );
                                pending.extend(state.update(Action::CopyDone(report), now));
                                pending.extend(state.update(Action::FormDone, now));
                            }
                            Then::Open if sub.print => resolved = Some(*sub),
                            Then::Open => {
                                let report = open_resolved(
                                    platform,
                                    &settings,
                                    &clipboard_settings,
                                    &ui_kb,
                                    &sub,
                                    state.options.remember_vars,
                                );
                                pending.extend(state.update(Action::CopyDone(report), now));
                                pending.extend(state.update(Action::FormDone, now));
                            }
                            Then::Run => {
                                let entry = ui_kb.entry(&sub.id).unwrap();
                                let mut map = kb_core::varres::Values::new();
                                for (name, val, _source) in &sub.values {
                                    map.insert(name.clone(), val.clone());
                                }
                                match kb_core::run::RunService::build_run_plan(
                                    &entry,
                                    &map,
                                    None,
                                    settings.run_destructive_patterns(),
                                ) {
                                    Ok(plan) => {
                                        pending.extend(state.update(Action::FormDone, now));
                                        pending.extend(state.update(
                                            Action::OpenRunModal {
                                                id: sub.id.clone(),
                                                plan,
                                            },
                                            now,
                                        ));
                                    }
                                    Err(e) => {
                                        let report = CopyReport {
                                            message: format!("cannot run: {e}"),
                                            sent: false,
                                        };
                                        pending.extend(state.update(Action::CopyDone(report), now));
                                        pending.extend(state.update(Action::FormDone, now));
                                    }
                                }
                            }
                        },
                        Recheck::Changed(spec) => {
                            pending.extend(state.update(Action::FormChanged(spec), now));
                        }
                        Recheck::Failed(message) => {
                            let report = CopyReport {
                                message,
                                sent: false,
                            };
                            pending.extend(state.update(Action::CopyDone(report), now));
                            pending.extend(state.update(Action::FormDone, now));
                        }
                    }
                    dirty = true;
                }
                Effect::ReadClipboard => {
                    let _ = worker.tx.send(Request::Tags);
                    let outcome = clipboard::read(platform);
                    let action = Action::ClipboardRead {
                        outcome,
                        now: SystemTime::now(),
                    };
                    pending.extend(state.update(action, Instant::now()));
                    dirty = true;
                }
                Effect::Sync => {
                    // leave the screen so an ssh password or passphrase prompt can be seen
                    guard.suspend();
                    let action = run_foreground_sync(
                        platform,
                        ui_kb.root(),
                        &state.options.settings,
                        &mut guard,
                        &mut terminal,
                        &theme,
                    );
                    let _ = guard.resume();
                    if let Ok(size) = terminal.size() {
                        let _ = terminal.resize(ratatui::layout::Rect::new(
                            0,
                            0,
                            size.width,
                            size.height,
                        ));
                    }
                    pending.extend(state.update(action, Instant::now()));
                    dirty = true;
                }
                Effect::LoadTags => {
                    let _ = worker.tx.send(Request::Tags);
                }
                Effect::SaveDraft(draft) => {
                    let clock = crate::author::clock();
                    let action = match crate::author::save_new(&mut ui_kb, &draft, clock.as_ref()) {
                        Ok(saved) => {
                            let rel_path = PathBuf::from(format!("{}.md", saved.id));
                            let is_signing = kb_core::git::commit::is_signing_configured(
                                ui_kb.root(),
                                &crate::platform::SystemGitRunner,
                            );
                            if is_signing {
                                guard.suspend();
                            }
                            let outcome = kb_core::git::commit::auto_commit_operation(
                                ui_kb.root(),
                                &crate::platform::SystemGitRunner,
                                &state.options.settings,
                                kb_core::git::commit::AutoCommitAction::Add,
                                &saved.id,
                                &[rel_path],
                            );
                            if is_signing {
                                let _ = guard.resume();
                                if let Ok(size) = terminal.size() {
                                    let _ = terminal.resize(ratatui::layout::Rect::new(
                                        0,
                                        0,
                                        size.width,
                                        size.height,
                                    ));
                                }
                            }
                            if let Ok(
                                kb_core::git::commit::AutoCommitOutcome::SkippedMissingIdentity {
                                    ..
                                },
                            ) = outcome
                            {
                                state.say(
                                    "Git identity unset: commit skipped",
                                    crate::tui::app::MessageKind::Warning,
                                    Instant::now(),
                                    true,
                                );
                            }
                            Action::Saved(saved.id)
                        }
                        Err(e) => Action::SaveFailed(e.message()),
                    };
                    pending.extend(state.update(action, Instant::now()));
                    dirty = true;
                }
                Effect::Added(id) => added = Some(id),
                Effect::RenameEntry { from, to } => {
                    let to = to.strip_suffix(".md").unwrap_or(&to).to_string();
                    let is_signing = kb_core::git::commit::is_signing_configured(
                        ui_kb.root(),
                        &crate::platform::SystemGitRunner,
                    );
                    if is_signing {
                        guard.suspend();
                    }
                    let action = match crate::author::move_entry(
                        platform,
                        &mut ui_kb,
                        &from,
                        &to,
                        &state.options.settings,
                    ) {
                        Ok((renamed, _)) => Action::Renamed {
                            from: renamed.from,
                            to: renamed.to,
                        },
                        Err(e) => Action::OpFailed(e.to_string()),
                    };
                    if is_signing {
                        let _ = guard.resume();
                        if let Ok(size) = terminal.size() {
                            let _ = terminal.resize(ratatui::layout::Rect::new(
                                0,
                                0,
                                size.width,
                                size.height,
                            ));
                        }
                    }
                    pending.extend(state.update(action, Instant::now()));
                    dirty = true;
                }
                Effect::RemoveEntry { id } => {
                    let is_signing = kb_core::git::commit::is_signing_configured(
                        ui_kb.root(),
                        &crate::platform::SystemGitRunner,
                    );
                    if is_signing {
                        guard.suspend();
                    }
                    let action = match crate::author::delete_entry(
                        platform,
                        &mut ui_kb,
                        &id,
                        &state.options.settings,
                    ) {
                        Ok(_) => Action::Removed(id),
                        Err(e) => Action::OpFailed(e.to_string()),
                    };
                    if is_signing {
                        let _ = guard.resume();
                        if let Ok(size) = terminal.size() {
                            let _ = terminal.resize(ratatui::layout::Rect::new(
                                0,
                                0,
                                size.width,
                                size.height,
                            ));
                        }
                    }
                    pending.extend(state.update(action, Instant::now()));
                    dirty = true;
                }
                Effect::EditBody(text) => {
                    let ty = state
                        .add
                        .as_ref()
                        .map_or(EntryType::Cmd, |a| a.draft.entry_type);
                    match edit_body(
                        platform,
                        editor.as_deref(),
                        &mut guard,
                        &mut terminal,
                        &text,
                        ty,
                        &signals,
                    ) {
                        BodyEdit::Done(result) => {
                            pending
                                .extend(state.update(Action::BodyEdited(result), Instant::now()));
                            dirty = true;
                        }
                        BodyEdit::Signalled(signal) => exit = Some(exit_code_for_signal(signal)),
                    }
                }
                Effect::EditEntry(id) => {
                    match edit_entry(
                        platform,
                        editor.as_deref(),
                        &ui_kb,
                        &mut guard,
                        &mut terminal,
                        &id,
                        &signals,
                    ) {
                        Edited::Returned(result) => {
                            pending.extend(
                                state.update(Action::EditorReturned(result), Instant::now()),
                            );
                            dirty = true;
                        }
                        Edited::Signalled(signal) => exit = Some(exit_code_for_signal(signal)),
                    }
                }
            }
        }
        if picked.is_some() || exit.is_some() || resolved.is_some() || added.is_some() {
            break;
        }
        if dirty {
            terminal
                .draw(|f| {
                    let area = f.area();
                    view::draw(&state, &theme, area, f.buffer_mut());
                })
                .map_err(|e| e.to_string())?;
            dirty = false;
            if first {
                first = false;
                #[cfg(debug_assertions)]
                if std::env::var_os("KB_TUI_PANIC_AFTER_FIRST_FRAME").is_some() {
                    panic!("KB_TUI_PANIC_AFTER_FIRST_FRAME");
                }
            }
        }
        if let Some(sig) = signals.received() {
            exit = Some(exit_code_for_signal(sig));
            break;
        }
        let mut actions: Vec<Action> = Vec::new();
        if event::poll(Duration::from_millis(50)).map_err(|e| e.to_string())? {
            loop {
                match event::read().map_err(|e| e.to_string())? {
                    Event::Key(k) => actions.push(Action::Key(k)),
                    Event::Mouse(m) => actions.push(Action::Mouse(m)),
                    Event::Paste(p) => actions.push(Action::Paste(p)),
                    Event::Resize(w, h) => actions.push(Action::Resize(w, h)),
                    _ => {}
                }
                if !event::poll(Duration::ZERO).map_err(|e| e.to_string())? {
                    break;
                }
            }
        }
        while let Ok(reply) = git_worker.rx.try_recv() {
            let action = match reply {
                header::GitReply::Status(status) => Action::GitStatus(status),
                header::GitReply::FetchFailed => Action::FetchFailed,
            };
            pending.extend(state.update(action, Instant::now()));
            dirty = true;
        }
        while let Ok(reply) = worker.rx.try_recv() {
            actions.push(match reply {
                Reply::Refreshed(info) => Action::Refreshed(RefreshInfo { ..info }),
                Reply::Results(g, page) => Action::Results(g, page),
                Reply::Preview(g, model) => Action::PreviewReady(g, model),
                Reply::Tags(tags) => Action::TagCounts(tags),
                Reply::Failed(m) => Action::WorkerFailed(m),
            });
        }
        let now = Instant::now();
        let had_input = !actions.is_empty();
        actions.push(Action::Tick);
        let before = state.message.clone();
        for action in actions {
            pending.extend(state.update(action, now));
        }
        dirty |= had_input || state.message != before;
    }

    worker.shutdown();
    drop(terminal);
    guard.restore();
    if let Some(code) = exit {
        return Ok(Outcome::Exit(code));
    }
    if let Some(id) = added {
        return Ok(Outcome::Added { id });
    }
    if let Some(sub) = resolved {
        return Ok(Outcome::Resolved {
            id: sub.id,
            values: sub.values,
            print: sub.print,
        });
    }
    match picked {
        Some(id) => Ok(Outcome::Picked { id }),
        None => Ok(Outcome::Exit(130)),
    }
}

fn real_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// What the entry looks like now compared with the form that was filled in.
enum Recheck {
    Same,
    Changed(Box<FormSpec>),
    Failed(String),
}

/// Reads the entry again before it is used (the file may have been edited since the form opened).
fn recheck(kb: &Kb, sub: &Submission) -> Recheck {
    match FormSpec::load(kb, &sub.id, &real_env) {
        Ok(Some(spec)) if spec.plan.signature == sub.signature => Recheck::Same,
        Ok(Some(spec)) => Recheck::Changed(Box::new(spec)),
        Ok(None) => Recheck::Failed(format!(
            "{} no longer has variables; nothing copied",
            sub.id
        )),
        Err(e) => Recheck::Failed(format!("Copy failed: {e}")),
    }
}

/// Ctrl-Y with a form: resolves the values, copies through the clipboard chain and, when the text
/// was sent, records the use and remembers the values.
fn copy_resolved(
    platform: &impl Platform,
    settings: &ClipboardSettings,
    kb: &Kb,
    sub: &Submission,
    remember: bool,
) -> CopyReport {
    let failed = |m: String| CopyReport {
        message: m,
        sent: false,
    };
    let entry = match kb.entry(&sub.id) {
        Ok(e) => e,
        Err(e) => return failed(format!("Copy failed: {e}")),
    };
    let plan = varres::plan_vars(&entry);
    let values: varres::Values = sub
        .values
        .iter()
        .map(|(n, v, _)| (n.clone(), v.clone()))
        .collect();
    let resolved = match varres::resolve(&plan, &values) {
        Ok(r) => r,
        Err(errors) => {
            let first = errors.first().map(ToString::to_string).unwrap_or_default();
            return failed(format!("Copy failed: {first}"));
        }
    };
    let Some(text) = resolved.primary().map(|o| o.text.clone()) else {
        return failed("Copy failed: nothing to copy".into());
    };
    let outcome = clipboard::copy(platform, settings, &text, Place::Ui);
    if outcome.sent() {
        let pairs: Vec<(String, String)> = if remember {
            sub.values
                .iter()
                .filter(|(_, v, _)| !v.is_secret())
                .map(|(n, v, _)| (n.clone(), v.reveal().to_string()))
                .collect()
        } else {
            Vec::new()
        };
        let _ = kb.record_resolved_use(&sub.id, SystemTime::now(), &pairs);
    }
    let mut message = outcome.message_short();
    if outcome.sent() && resolved.has_secret {
        message.push_str(" · the clipboard now holds a secret");
    }
    CopyReport {
        message,
        sent: outcome.sent(),
    }
}

/// Ctrl-Y: copies the selected entry's payload through the platform's clipboard chain. Printing
/// is not an option here (the screen belongs to the UI), so a copy that has nowhere to go is
/// reported in the key bar. A use is recorded when the text was sent.
fn copy_selected(
    platform: &impl Platform,
    settings: &ClipboardSettings,
    state: &AppState,
    kb: &Kb,
) -> CopyReport {
    // ponytail: the copy runs on this thread; a hung clipboard tool freezes the UI for up to its
    // 2 s timeout. Move it to the worker if that ever bites.
    let Some(id) = &state.selected_id else {
        return CopyReport {
            message: "Copy failed: nothing selected".into(),
            sent: false,
        };
    };
    let payload = match kb.pick(id, PrintMode::Payload) {
        Ok(p) => p,
        Err(e) => {
            return CopyReport {
                message: format!("Copy failed: {e}"),
                sent: false,
            };
        }
    };
    let outcome = clipboard::copy(platform, settings, &payload, Place::Ui);
    if outcome.sent() {
        let _ = kb.record_use(id, SystemTime::now());
    }
    CopyReport {
        message: outcome.message_short(),
        sent: outcome.sent(),
    }
}

fn open_selected(
    platform: &impl Platform,
    settings: &Settings,
    clipboard_settings: &ClipboardSettings,
    kb: &Kb,
    id: &str,
    _remember: bool,
) -> CopyReport {
    open_in_tui(platform, settings, clipboard_settings, kb, id, None, &[])
}

fn open_resolved(
    platform: &impl Platform,
    settings: &Settings,
    clipboard_settings: &ClipboardSettings,
    kb: &Kb,
    sub: &Submission,
    remember: bool,
) -> CopyReport {
    let failed = |m: String| CopyReport {
        message: m,
        sent: false,
    };
    let entry = match kb.entry(&sub.id) {
        Ok(e) => e,
        Err(e) => return failed(format!("Open failed: {e}")),
    };
    let plan = varres::plan_vars(&entry);
    let values: varres::Values = sub
        .values
        .iter()
        .map(|(n, v, _)| (n.clone(), v.clone()))
        .collect();
    let resolved = match varres::resolve(&plan, &values) {
        Ok(r) => r,
        Err(errors) => {
            let first = errors.first().map(ToString::to_string).unwrap_or_default();
            return failed(format!("Open failed: {first}"));
        }
    };
    let pairs: Vec<(String, String)> = if remember {
        sub.values
            .iter()
            .filter(|(_, v, _)| !v.is_secret())
            .map(|(n, v, _)| (n.clone(), v.reveal().to_string()))
            .collect()
    } else {
        Vec::new()
    };
    open_in_tui(
        platform,
        settings,
        clipboard_settings,
        kb,
        &sub.id,
        Some(resolved),
        &pairs,
    )
}

fn open_in_tui(
    platform: &impl Platform,
    settings: &Settings,
    clipboard_settings: &ClipboardSettings,
    kb: &Kb,
    id: &str,
    resolved_result: Option<varres::Resolved>,
    pairs: &[(String, String)],
) -> CopyReport {
    let entry = match kb.entry(id) {
        Ok(e) => e,
        Err(e) => {
            return CopyReport {
                message: format!("Open failed: {e}"),
                sent: false,
            };
        }
    };
    let ty = match entry.entry_type() {
        Some(t) => t,
        None => {
            return CopyReport {
                message: format!("{id}: missing or unknown type"),
                sent: false,
            };
        }
    };

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
            };
            let Some(url) = url else {
                return CopyReport {
                    message: format!("{id}: link has no URL"),
                    sent: false,
                };
            };
            match opener::open_url(platform, settings.opener_command(), &url) {
                Ok(Opened::Launched) => {
                    let _ = kb.record_resolved_use(id, SystemTime::now(), pairs);
                    CopyReport {
                        message: "Opened in browser".into(),
                        sent: true,
                    }
                }
                Ok(Opened::Printed(url)) => {
                    let outcome = clipboard::copy(platform, clipboard_settings, &url, Place::Ui);
                    if outcome.sent() {
                        let _ = kb.record_resolved_use(id, SystemTime::now(), pairs);
                    }
                    CopyReport {
                        message: "Copied URL (no display)".into(),
                        sent: outcome.sent(),
                    }
                }
                Err(e) => CopyReport {
                    message: format!("Open failed: {e}"),
                    sent: false,
                },
            }
        }
        EntryType::Sumo => {
            let query = if let Some(resolved) = &resolved_result {
                resolved
                    .primary()
                    .map(|o| o.text.clone())
                    .unwrap_or_default()
            } else {
                match kb.pick(id, PrintMode::Payload) {
                    Ok(p) => p,
                    Err(e) => {
                        return CopyReport {
                            message: format!("Open failed: {e}"),
                            sent: false,
                        };
                    }
                }
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
                let outcome = clipboard::copy(platform, clipboard_settings, &query, Place::Ui);
                if outcome.sent() {
                    let _ = kb.record_resolved_use(id, SystemTime::now(), pairs);
                }
                return CopyReport {
                    message: "Sumo URL not configured; copied query to clipboard".into(),
                    sent: outcome.sent(),
                };
            };

            let link = match ConsoleService::build_sumo_link(base_url, &query, range.as_deref()) {
                Ok(l) => l,
                Err(e) => {
                    return CopyReport {
                        message: format!("Open failed: {e}"),
                        sent: false,
                    };
                }
            };

            if !link.query_included {
                let _ = clipboard::copy(platform, clipboard_settings, &query, Place::Ui);
            }

            match opener::open_url(platform, settings.opener_command(), &link.url) {
                Ok(Opened::Launched) => {
                    let _ = kb.record_resolved_use(id, SystemTime::now(), pairs);
                    let msg = if !link.query_included {
                        "URL exceeds limit; copied query & opened console".into()
                    } else {
                        "Opened Sumo Logic search console".into()
                    };
                    CopyReport {
                        message: msg,
                        sent: true,
                    }
                }
                Ok(Opened::Printed(url)) => {
                    let outcome = clipboard::copy(platform, clipboard_settings, &url, Place::Ui);
                    if outcome.sent() {
                        let _ = kb.record_resolved_use(id, SystemTime::now(), pairs);
                    }
                    CopyReport {
                        message: "Copied URL (no display)".into(),
                        sent: outcome.sent(),
                    }
                }
                Err(e) => CopyReport {
                    message: format!("Open failed: {e}"),
                    sent: false,
                },
            }
        }
        EntryType::Cw => {
            let region = match ConsoleService::resolve_aws_region(&entry, settings, &OsEnv) {
                Ok(r) => r,
                Err(e) => {
                    return CopyReport {
                        message: format!("Open failed: {e}"),
                        sent: false,
                    };
                }
            };

            let query = if let Some(resolved) = &resolved_result {
                resolved
                    .primary()
                    .map(|o| o.text.clone())
                    .unwrap_or_default()
            } else {
                match kb.pick(id, PrintMode::Payload) {
                    Ok(p) => p,
                    Err(e) => {
                        return CopyReport {
                            message: format!("Open failed: {e}"),
                            sent: false,
                        };
                    }
                }
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

            let link = match ConsoleService::build_cw_link(
                &region,
                &resolved_log_groups,
                &query,
                range.as_deref(),
            ) {
                Ok(l) => l,
                Err(e) => {
                    return CopyReport {
                        message: format!("Open failed: {e}"),
                        sent: false,
                    };
                }
            };

            if !link.query_included {
                let _ = clipboard::copy(platform, clipboard_settings, &query, Place::Ui);
            }

            match opener::open_url(platform, settings.opener_command(), &link.url) {
                Ok(Opened::Launched) => {
                    let _ = kb.record_resolved_use(id, SystemTime::now(), pairs);
                    let msg = if !link.query_included {
                        "URL exceeds limit; copied query & opened console".into()
                    } else {
                        "Opened CloudWatch console".into()
                    };
                    CopyReport {
                        message: msg,
                        sent: true,
                    }
                }
                Ok(Opened::Printed(url)) => {
                    let outcome = clipboard::copy(platform, clipboard_settings, &url, Place::Ui);
                    if outcome.sent() {
                        let _ = kb.record_resolved_use(id, SystemTime::now(), pairs);
                    }
                    CopyReport {
                        message: "Copied URL (no display)".into(),
                        sent: outcome.sent(),
                    }
                }
                Err(e) => CopyReport {
                    message: format!("Open failed: {e}"),
                    sent: false,
                },
            }
        }
        EntryType::Code => CopyReport {
            message: "Code open is not implemented in this phase".into(),
            sent: false,
        },
        _ => CopyReport {
            message: format!("{} entries do not have an open action", ty.name()),
            sent: false,
        },
    }
}

/// How the editor session on a draft's body ended.
enum BodyEdit {
    /// Back in the UI: the new text, `None` when it did not change, or why it could not be used.
    Done(Result<Option<String>, String>),
    /// A termination signal arrived while the editor ran; the editor was killed.
    Signalled(i32),
}

/// Ctrl-E in the add form: the body goes to a private temp file (an extension that gets syntax
/// highlighting), the editor runs on it, and what it holds afterwards is the new body. The file
/// is deleted whichever way this ends.
fn edit_body(
    platform: &impl Platform,
    editor: Option<&str>,
    guard: &mut TerminalGuard,
    terminal: &mut Terminal<CrosstermBackend<std::fs::File>>,
    text: &str,
    ty: EntryType,
    signals: &Signals,
) -> BodyEdit {
    let ext = match ty {
        EntryType::Sql => "sql",
        EntryType::Cmd => "sh",
        EntryType::Prompt => "md",
        _ => "txt",
    };
    let temp = match platform.private_temp_file(ext, text.as_bytes()) {
        Ok(t) => t,
        Err(e) => return BodyEdit::Done(Err(format!("could not make a temporary file: {e}"))),
    };
    let editor = platform.resolve_editor(editor);
    guard.suspend();
    let result = guard.writer().and_then(|tty| {
        platform.run_editor(&editor, temp.path(), &tty, &|| signals.received().is_some())
    });
    if matches!(result, Ok(None)) {
        return BodyEdit::Signalled(signals.received().unwrap_or(15));
    }
    let resumed = guard.resume();
    if let Ok(size) = terminal.size() {
        let _ = terminal.resize(ratatui::layout::Rect::new(0, 0, size.width, size.height));
    }
    let status = match result {
        Ok(Some(status)) => status,
        Ok(None) => return BodyEdit::Signalled(15),
        Err(e) => return BodyEdit::Done(Err(format!("could not start the editor: {e}"))),
    };
    if let Err(e) = resumed {
        return BodyEdit::Done(Err(format!("could not return to the UI: {e}")));
    }
    if !status.success() {
        return BodyEdit::Done(Err(
            "the editor exited with an error; the body is unchanged".into(),
        ));
    }
    let bytes = match temp.read_bytes() {
        Ok(b) => b,
        Err(e) => return BodyEdit::Done(Err(format!("could not read the edited body: {e}"))),
    };
    let Ok(new) = String::from_utf8(bytes) else {
        return BodyEdit::Done(Err(
            "the edited body is not valid UTF-8 text; unchanged".into()
        ));
    };
    let unchanged = new.replace("\r\n", "\n").trim_end() == text.trim_end();
    BodyEdit::Done(Ok((!unchanged).then_some(new)))
}

/// How an editor session ended.
enum Edited {
    /// Back in the UI; the text is the first lint error or a failure to report.
    Returned(Option<String>),
    /// A termination signal arrived while the editor ran; the editor was killed.
    Signalled(i32),
}

fn edit_entry(
    platform: &impl Platform,
    editor: Option<&str>,
    kb: &Kb,
    guard: &mut TerminalGuard,
    terminal: &mut Terminal<CrosstermBackend<std::fs::File>>,
    id: &str,
    signals: &Signals,
) -> Edited {
    let path: PathBuf = match kb.entry_path(id) {
        Ok(p) => p,
        Err(e) => return Edited::Returned(Some(e.to_string())),
    };
    let editor = platform.resolve_editor(editor);
    guard.suspend();
    let result = guard.writer().and_then(|tty| {
        platform.run_editor(&editor, &path, &tty, &|| signals.received().is_some())
    });
    if matches!(result, Ok(None)) {
        // leave the terminal as the shell expects it
        return Edited::Signalled(signals.received().unwrap_or(15));
    }
    let resumed = guard.resume();
    // `Terminal::clear` asks the terminal for the cursor position (and waits up to two seconds
    // for the answer); `resize` redraws everything without asking.
    if let Ok(size) = terminal.size() {
        let _ = terminal.resize(ratatui::layout::Rect::new(0, 0, size.width, size.height));
    }
    if let Err(e) = result {
        return Edited::Returned(Some(format!("could not start the editor: {e}")));
    }
    if let Err(e) = resumed {
        return Edited::Returned(Some(format!("could not return to the UI: {e}")));
    }
    Edited::Returned(kb.lint_entry(id).ok().and_then(|f| first_error_text(&f)))
}

/// Alt-S: the same pipeline as `kb sync`, on the suspended terminal so a password or passphrase
/// prompt can be answered. Each conflict brings the UI back as the conflict screen.
fn run_foreground_sync(
    platform: &impl Platform,
    root: &Path,
    settings: &Settings,
    guard: &mut TerminalGuard,
    terminal: &mut Terminal<CrosstermBackend<std::fs::File>>,
    theme: &Theme,
) -> Action {
    use conflict_view::{ConflictOutcome, ConflictScreen, draw_conflict};
    use kb_core::sync::conflict::validate_entry_text;
    use kb_core::sync::{
        Conflict, ConflictError, ConflictHandler, ResolutionChoice, SyncConfig, SyncEngine,
        SyncOptions,
    };

    struct Screen<'a, P: Platform> {
        platform: &'a P,
        editor: Option<String>,
        guard: &'a mut TerminalGuard,
        terminal: &'a mut Terminal<CrosstermBackend<std::fs::File>>,
        theme: &'a Theme,
        host: String,
        number: usize,
    }

    impl<P: Platform> Screen<'_, P> {
        /// Shows `screen` until a key decides it.
        fn decide(
            &mut self,
            screen: &mut ConflictScreen,
        ) -> Result<ConflictOutcome, ConflictError> {
            let _ = self.guard.resume();
            let result = loop {
                let theme = self.theme;
                if self
                    .terminal
                    .draw(|f| {
                        let area = f.area();
                        draw_conflict(screen, theme, area, f.buffer_mut());
                    })
                    .is_err()
                {
                    break Err(ConflictError::Io(
                        "could not draw the conflict screen".into(),
                    ));
                }
                match event::poll(Duration::from_millis(100)) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(e) => break Err(ConflictError::Io(e.to_string())),
                }
                match event::read() {
                    Ok(Event::Key(k)) if k.kind != crossterm::event::KeyEventKind::Release => {
                        if let Some(outcome) = screen.key(k) {
                            break Ok(outcome);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => break Err(ConflictError::Io(e.to_string())),
                }
            };
            // git may prompt for credentials next: give the terminal back
            self.guard.suspend();
            result
        }

        fn edit_merged(&mut self, conflict: &Conflict) -> Option<String> {
            let tty = self.guard.writer().ok()?;
            let temp = self
                .platform
                .private_temp_file(
                    "md",
                    crate::commands::sync::merged_template(conflict).as_bytes(),
                )
                .ok()?;
            let editor = self.platform.resolve_editor(self.editor.as_deref());
            self.guard.suspend();
            let ran = self
                .platform
                .run_editor(&editor, temp.path(), &tty, &|| false);
            ran.ok()??;
            temp.read_to_string().ok()
        }
    }

    impl<P: Platform> ConflictHandler for Screen<'_, P> {
        fn resolve(&mut self, c: &Conflict) -> Result<ResolutionChoice, ConflictError> {
            self.number += 1;
            let mut screen = ConflictScreen::new(c.clone(), self.number);
            loop {
                match self.decide(&mut screen)? {
                    ConflictOutcome::Mine => return Ok(ResolutionChoice::Mine),
                    ConflictOutcome::Theirs => return Ok(ResolutionChoice::Theirs),
                    ConflictOutcome::Delete => return Ok(ResolutionChoice::Delete),
                    ConflictOutcome::Abort | ConflictOutcome::NewId(_) => {
                        return Err(ConflictError::UserAborted);
                    }
                    ConflictOutcome::Both => {
                        return Ok(ResolutionChoice::KeepBoth {
                            new_id: format!("{}-conflict-{}", c.id, self.host),
                        });
                    }
                    ConflictOutcome::Edit => match self.edit_merged(c) {
                        None => screen.message = Some("could not run the editor".into()),
                        Some(text) => match validate_entry_text(&c.id, &text) {
                            None => return Ok(ResolutionChoice::EditMerged(text)),
                            Some(problem) => screen.message = Some(problem),
                        },
                    },
                }
            }
        }

        fn new_id(&mut self, taken: &str) -> Result<String, ConflictError> {
            let mut screen = ConflictScreen::new(
                Conflict {
                    id: taken.to_string(),
                    rel_path: format!("{taken}.md").into(),
                    kind: kb_core::sync::ConflictKind::BothAdded,
                    mine: None,
                    theirs: None,
                    base: None,
                },
                self.number,
            );
            screen.new_id = Some((String::new(), taken.to_string()));
            match self.decide(&mut screen)? {
                ConflictOutcome::NewId(id) => Ok(id),
                _ => Err(ConflictError::UserAborted),
            }
        }
    }

    let git = crate::platform::SystemGitRunner;
    let config = SyncConfig {
        strategy: settings.sync_strategy(),
        timeout: Duration::from_secs(settings.sync_timeout()),
    };
    let mut handler = Screen {
        platform,
        editor: settings.editor().map(str::to_string),
        guard,
        terminal,
        theme,
        host: crate::commands::sync::host(),
        number: 0,
    };
    match SyncEngine::new(root, &git, config).execute(&SyncOptions::default(), &mut handler) {
        Ok(s) => Action::SyncDone(format!("Sync: {}", s.new_status), true),
        Err(e) => Action::SyncDone(format!("Sync failed: {e}"), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_is_shown_with_a_tilde() {
        let home = Path::new("/home/carlos");
        assert_eq!(
            display_root(Path::new("/home/carlos/kb"), Some(home)),
            "~/kb"
        );
        assert_eq!(display_root(Path::new("/home/carlos"), Some(home)), "~");
        assert_eq!(display_root(Path::new("/data/kb"), Some(home)), "/data/kb");
        assert_eq!(
            display_root(Path::new("/home/carlos/kb"), None),
            "/home/carlos/kb"
        );
    }
}
