//! Terminal setup and restore, and signals.

use std::fs::File;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

/// The shell convention: 128 plus the signal number.
pub fn exit_code_for_signal(signal: i32) -> i32 {
    128 + signal
}

/// Writes the escape sequences that undo `enter` (everything but raw mode).
pub fn restore(out: &mut impl Write, mouse: bool) -> io::Result<()> {
    if mouse {
        execute!(out, DisableMouseCapture)?;
    }
    execute!(out, DisableBracketedPaste, Show, LeaveAlternateScreen)
}

fn enter_sequences(out: &mut impl Write, mouse: bool) -> io::Result<()> {
    execute!(out, EnterAlternateScreen, Hide, EnableBracketedPaste)?;
    if mouse {
        execute!(out, EnableMouseCapture)?;
    }
    Ok(())
}

/// The terminal to restore if the program panics: set while the UI is up.
static ACTIVE: Mutex<Option<(File, bool)>> = Mutex::new(None);

/// Whether the terminal is in the UI state (raw mode, alternate screen). Whoever undoes it first
/// (the panic hook or the guard) flips this, so the sequences are written exactly once: leaving
/// the alternate screen twice would put the cursor back at its saved position and the shell's
/// next output would overwrite what was printed in between, such as a panic message.
static UP: AtomicBool = AtomicBool::new(false);

fn undo(out: &mut impl Write, mouse: bool) {
    if UP.swap(false, Ordering::SeqCst) {
        let _ = restore(out, mouse);
        let _ = disable_raw_mode();
    }
}

fn restore_active() {
    let taken = ACTIVE.lock().ok().and_then(|mut a| a.take());
    if let Some((mut tty, mouse)) = taken {
        undo(&mut tty, mouse);
    }
}

/// Puts the terminal in raw mode on the alternate screen, and puts it back when dropped.
pub struct TerminalGuard {
    tty: File,
    mouse: bool,
}

impl TerminalGuard {
    pub fn enter(tty: File, mouse: bool) -> io::Result<TerminalGuard> {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_active();
            previous(info);
        }));
        enable_raw_mode()?;
        let mut guard = TerminalGuard { tty, mouse };
        enter_sequences(&mut guard.tty, mouse)?;
        UP.store(true, Ordering::SeqCst);
        if let Ok(copy) = guard.tty.try_clone()
            && let Ok(mut active) = ACTIVE.lock()
        {
            *active = Some((copy, mouse));
        }
        Ok(guard)
    }

    /// A handle to draw on.
    pub fn writer(&self) -> io::Result<File> {
        self.tty.try_clone()
    }

    /// Gives the terminal back (for the editor).
    pub fn suspend(&mut self) {
        undo(&mut self.tty, self.mouse);
    }

    pub fn resume(&mut self) -> io::Result<()> {
        if !UP.load(Ordering::SeqCst) {
            enable_raw_mode()?;
            enter_sequences(&mut self.tty, self.mouse)?;
            UP.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    pub fn restore(&mut self) {
        self.suspend();
        if let Ok(mut active) = ACTIVE.lock() {
            *active = None;
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Flags set by SIGINT, SIGTERM and SIGHUP; the event loop checks them every tick.
pub struct Signals {
    flags: Vec<(i32, Arc<AtomicBool>)>,
}

impl Signals {
    pub fn register() -> Signals {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        let mut flags = Vec::new();
        for sig in [SIGINT, SIGTERM, SIGHUP] {
            let flag = Arc::new(AtomicBool::new(false));
            if signal_hook::flag::register(sig, Arc::clone(&flag)).is_ok() {
                flags.push((sig, flag));
            }
        }
        Signals { flags }
    }

    /// The first signal that arrived, if any.
    pub fn received(&self) -> Option<i32> {
        self.flags
            .iter()
            .find(|(_, f)| f.load(Ordering::Relaxed))
            .map(|(s, _)| *s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_leaves_the_alternate_screen_and_shows_the_cursor_and_is_repeatable() {
        let mut out = Vec::new();
        restore(&mut out, false).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("\x1b[?1049l") && s.contains("\x1b[?25h") && s.contains("\x1b[?2004l"),
            "{s:?}"
        );
        assert!(!s.contains("\x1b[?1000l"));
        let mut out = Vec::new();
        restore(&mut out, true).unwrap();
        restore(&mut out, true).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.matches("\x1b[?1049l").count(), 2);
        assert!(s.contains("\x1b[?1000l"));
    }

    #[test]
    fn signal_exit_codes_follow_the_shell_convention() {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        assert_eq!(exit_code_for_signal(SIGINT), 130);
        assert_eq!(exit_code_for_signal(SIGTERM), 143);
        assert_eq!(exit_code_for_signal(SIGHUP), 129);
    }

    #[test]
    fn a_fresh_signal_set_has_nothing_pending() {
        assert_eq!(Signals::register().received(), None);
    }

    #[test]
    fn mouse_capture_is_only_switched_on_when_asked_for() {
        let mut off = Vec::new();
        enter_sequences(&mut off, false).unwrap();
        let off = String::from_utf8(off).unwrap();
        assert!(
            off.contains("\x1b[?1049h")
                && off.contains("\x1b[?2004h")
                && !off.contains("\x1b[?1000h"),
            "{off:?}"
        );
        let mut on = Vec::new();
        enter_sequences(&mut on, true).unwrap();
        assert!(String::from_utf8(on).unwrap().contains("\x1b[?1000h"));
    }
}
