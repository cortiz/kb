//! The history overlay (Alt-H): the commits that changed the selected entry, a diff on Enter,
//! and restore on `r`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use kb_core::history::EntryHistory;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Clear, Widget};

use super::layout::truncate;
use super::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryState {
    pub history: EntryHistory,
    pub selected: usize,
    /// The diff being shown, and how far it is scrolled.
    pub diff: Option<(String, usize)>,
    pub confirm_restore: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryOutcome {
    Nothing,
    Close,
    Diff(String),
    Restore(String),
}

impl HistoryState {
    pub fn new(history: EntryHistory) -> Self {
        Self {
            history,
            selected: 0,
            diff: None,
            confirm_restore: false,
            message: None,
        }
    }

    fn hash(&self) -> Option<String> {
        self.history
            .commits
            .get(self.selected)
            .map(|c| c.hash.clone())
    }

    /// The restore question takes its answer as it is, whatever the keymap says.
    pub fn takes_raw_keys(&self) -> bool {
        self.confirm_restore
    }

    pub fn key(&mut self, e: KeyEvent) -> HistoryOutcome {
        self.message = None;
        if e.modifiers.contains(KeyModifiers::CONTROL) {
            return if e.code == KeyCode::Char('c') {
                HistoryOutcome::Close
            } else {
                HistoryOutcome::Nothing
            };
        }
        if self.confirm_restore {
            self.confirm_restore = false;
            return match (e.code, self.hash()) {
                (KeyCode::Char('y'), Some(h)) => HistoryOutcome::Restore(h),
                _ => HistoryOutcome::Nothing,
            };
        }
        let last = self.history.commits.len().saturating_sub(1);
        if let Some((_, scroll)) = &mut self.diff {
            match e.code {
                KeyCode::Esc | KeyCode::Char('q') => self.diff = None,
                KeyCode::Down | KeyCode::Char('j') => *scroll += 1,
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageDown => *scroll += 10,
                KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                _ => {}
            }
            return HistoryOutcome::Nothing;
        }
        match e.code {
            KeyCode::Esc | KeyCode::Char('q') => return HistoryOutcome::Close,
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Enter => {
                return self
                    .hash()
                    .map_or(HistoryOutcome::Nothing, HistoryOutcome::Diff);
            }
            KeyCode::Char('r') if self.hash().is_some() => self.confirm_restore = true,
            _ => {}
        }
        HistoryOutcome::Nothing
    }
}

pub fn draw_history(state: &HistoryState, theme: &Theme, area: Rect, buf: &mut Buffer) {
    if area.width < 40 || area.height < 8 {
        return;
    }
    let w = area.width.saturating_sub(4).min(110);
    let h = area.height.saturating_sub(4);
    let popup = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    Clear.render(popup, buf);
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" History: {} ", truncate(&state.history.id, 60)))
        .border_style(theme.border())
        .render(popup, buf);
    let inner_w = usize::from(popup.width - 4);
    let rows = usize::from(popup.height.saturating_sub(4));
    let x = popup.x + 2;

    if let Some((diff, scroll)) = &state.diff {
        for (i, line) in diff.lines().skip(*scroll).take(rows).enumerate() {
            let style = match line.chars().next() {
                Some('+') => theme.matched(),
                Some('-') => theme.warning(),
                _ => theme.text(),
            };
            buf.set_string(x, popup.y + 1 + i as u16, truncate(line, inner_w), style);
        }
        buf.set_string(
            x,
            popup.y + popup.height - 2,
            "[↑/↓] scroll   [Esc] back to the list",
            theme.muted(),
        );
        return;
    }

    let skip = state.selected.saturating_sub(rows.saturating_sub(1));
    for (i, c) in state
        .history
        .commits
        .iter()
        .enumerate()
        .skip(skip)
        .take(rows)
    {
        let cats: Vec<String> = c
            .change_categories
            .iter()
            .map(|k| format!("{k:?}").to_lowercase())
            .collect();
        let author = c.author.split(" <").next().unwrap_or(&c.author);
        let line = format!(
            "{}  {:<14}  {}  {:<22}  {}",
            c.date.get(..10).unwrap_or(&c.date),
            truncate(author, 14),
            truncate(&c.short_hash, 7),
            truncate(&cats.join(", "), 22),
            c.message
        );
        let style = if i == state.selected {
            theme.selection()
        } else {
            theme.text()
        };
        buf.set_string(
            x,
            popup.y + 1 + (i - skip) as u16,
            truncate(&line, inner_w),
            style,
        );
    }
    let footer = if state.confirm_restore {
        "Restore the entry from this revision? (y/n)".to_string()
    } else if let Some(m) = &state.message {
        m.lines().next().unwrap_or("").to_string()
    } else {
        "[Enter] view diff   [r] restore revision   [Esc] close".to_string()
    };
    buf.set_string(
        x,
        popup.y + popup.height - 2,
        truncate(&footer, inner_w),
        theme.warning(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use kb_core::history::{ChangeCategory, EntryCommit};

    fn state() -> HistoryState {
        let commit = |h: &str, m: &str| EntryCommit {
            hash: format!("{h}full"),
            short_hash: h.into(),
            author: "Ann <a@x>".into(),
            date: "2026-10-10T00:00:00Z".into(),
            message: m.into(),
            change_categories: vec![ChangeCategory::Payload],
        };
        HistoryState::new(EntryHistory {
            id: "a/b".into(),
            commits: vec![commit("c2", "kb: edit a/b"), commit("c1", "kb: add a/b")],
        })
    }

    fn press(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn keys_select_diff_and_restore_after_confirming() {
        let mut s = state();
        s.key(press(KeyCode::Char('j')));
        assert_eq!(s.selected, 1);
        assert_eq!(
            s.key(press(KeyCode::Enter)),
            HistoryOutcome::Diff("c1full".into())
        );
        assert_eq!(s.key(press(KeyCode::Char('r'))), HistoryOutcome::Nothing);
        assert_eq!(
            s.key(press(KeyCode::Char('n'))),
            HistoryOutcome::Nothing,
            "n cancels"
        );
        s.key(press(KeyCode::Char('r')));
        assert_eq!(
            s.key(press(KeyCode::Char('y'))),
            HistoryOutcome::Restore("c1full".into())
        );
        assert_eq!(s.key(press(KeyCode::Esc)), HistoryOutcome::Close);
    }

    #[test]
    fn esc_leaves_the_diff_before_closing() {
        let mut s = state();
        s.diff = Some(("+x".into(), 0));
        assert_eq!(s.key(press(KeyCode::Esc)), HistoryOutcome::Nothing);
        assert!(s.diff.is_none());
        assert_eq!(s.key(press(KeyCode::Esc)), HistoryOutcome::Close);
    }

    #[test]
    fn draws_the_commits() {
        let area = Rect::new(0, 0, 100, 20);
        let mut buf = Buffer::empty(area);
        draw_history(&state(), &Theme::no_color(), area, &mut buf);
        let text: String = (0..20)
            .map(|y| {
                (0..100)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    + "\n"
            })
            .collect();
        assert!(
            text.contains("History: a/b")
                && text.contains("kb: edit a/b")
                && text.contains("payload")
        );
    }
}
