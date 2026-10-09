//! The full-screen terminal UI.

pub mod app;
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

use kb_core::entry_json::PrintMode;
use kb_core::search::Kb;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::terminal::size;

use crate::platform::Platform;
use crate::platform::clipboard::{self, ClipboardSettings, Place};
use app::{Action, AppState, CopyReport, Effect, Options, RefreshInfo};
use terminal::{Signals, TerminalGuard, exit_code_for_signal};
use theme::{Theme, ThemeEnv};
use worker::{Reply, Request, Worker};

/// How the TUI ended.
#[derive(Debug)]
pub enum Outcome {
    /// An entry was picked; the caller copies or prints it (and records the use).
    Picked { id: String },
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
    let ui_kb = Kb::open(root).map_err(|e| e.to_string())?;
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
    let mut worker = Worker::spawn(root.to_path_buf(), theme.is_color());
    let mut pending: Vec<Effect> = state.start();
    let mut picked: Option<String> = None;
    let mut exit: Option<i32> = None;
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
                Effect::Pick => picked = state.selected_id.clone(),
                Effect::Quit(code) => exit = Some(code),
                Effect::CopyPayload => {
                    let report = copy_selected(platform, &clipboard_settings, &state, &ui_kb);
                    pending.extend(state.update(Action::CopyDone(report), Instant::now()));
                    dirty = true;
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
        if picked.is_some() || exit.is_some() {
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
    match picked {
        Some(id) => Ok(Outcome::Picked { id }),
        None => Ok(Outcome::Exit(130)),
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
