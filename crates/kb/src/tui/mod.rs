//! The full-screen terminal UI.

pub mod add;
pub mod add_view;
pub mod app;
pub mod form;
pub mod form_view;
pub mod highlight;
pub mod input;
pub mod keys;
pub mod layout;
pub mod markdown;
pub mod preview;
pub mod terminal;
pub mod theme;
pub mod view;
pub mod worker;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use kb_core::entry::EntryType;
use kb_core::entry_json::PrintMode;
use kb_core::search::Kb;
use kb_core::varres::{self, Source, Value};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::terminal::size;

use crate::platform::Platform;
use crate::platform::clipboard::{self, ClipboardSettings, Place};
use app::{Action, AppState, CopyReport, Effect, Options, RefreshInfo};
use form::{FormSpec, Submission, Then};
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
    let mut pending: Vec<Effect> = state.start();
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
                Effect::LoadTags => {
                    let _ = worker.tx.send(Request::Tags);
                }
                Effect::SaveDraft(draft) => {
                    let clock = crate::author::clock();
                    let action = match crate::author::save_new(&mut ui_kb, &draft, clock.as_ref()) {
                        Ok(saved) => Action::Saved(saved.id),
                        Err(e) => Action::SaveFailed(e.message()),
                    };
                    pending.extend(state.update(action, Instant::now()));
                    dirty = true;
                }
                Effect::Added(id) => added = Some(id),
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
