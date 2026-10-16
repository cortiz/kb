//! Choosing between a runbook's active runs and a new one, and the stronger confirmation before a
//! run of an unreviewed runbook starts (spec 015 FR-004).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Widget};

use super::keys::{Code, Key};
use super::layout::truncate;
use super::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub id: String,
    pub name: String,
    pub progress: String,
    pub updated: i64,
    pub locked: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct RunPicker {
    pub runbook_id: String,
    pub title: String,
    pub rows: Vec<RunRow>,
    /// `rows.len()` is "Start a new run".
    pub selected: usize,
    pub unreviewed: bool,
    pub confirming: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerEvent {
    None,
    Close,
    Resume(String),
    New,
}

impl RunPicker {
    pub fn new(runbook_id: &str, title: &str, rows: Vec<RunRow>, unreviewed: bool) -> RunPicker {
        RunPicker {
            runbook_id: runbook_id.into(),
            title: title.into(),
            selected: 0,
            rows,
            unreviewed,
            confirming: false,
        }
    }

    /// Straight to the "start a new run" confirmation of an unreviewed runbook.
    pub fn confirm_new(runbook_id: &str, title: &str) -> RunPicker {
        let mut p = RunPicker::new(runbook_id, title, Vec::new(), true);
        p.confirming = true;
        p
    }

    pub fn key(&mut self, key: Key) -> PickerEvent {
        if self.confirming {
            if key.code == Code::Char('y') && key.mods == 0 {
                return PickerEvent::New;
            }
            if self.rows.is_empty() {
                return PickerEvent::Close;
            }
            self.confirming = false;
            return PickerEvent::None;
        }
        match key.code {
            Code::Esc => PickerEvent::Close,
            Code::Down | Code::Char('j') => {
                self.selected = (self.selected + 1).min(self.rows.len());
                PickerEvent::None
            }
            Code::Up | Code::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                PickerEvent::None
            }
            Code::Enter => match self.rows.get(self.selected) {
                Some(r) => PickerEvent::Resume(r.id.clone()),
                None if self.unreviewed => {
                    self.confirming = true;
                    PickerEvent::None
                }
                None => PickerEvent::New,
            },
            _ => PickerEvent::None,
        }
    }
}

fn ago(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 86_400 {
        format!("{}d ago", s / 86_400)
    } else if s >= 3600 {
        format!("{}h ago", s / 3600)
    } else if s >= 60 {
        format!("{}m ago", s / 60)
    } else {
        "just now".into()
    }
}

pub fn draw_picker(p: &RunPicker, theme: &Theme, area: Rect, buf: &mut Buffer) {
    let w = 78.min(area.width.saturating_sub(2)).max(30);
    let h = (p.rows.len() as u16 + 6)
        .min(area.height.saturating_sub(2))
        .max(6);
    let r = Rect::new(
        area.x + (area.width.saturating_sub(w)) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w.min(area.width),
        h.min(area.height),
    );
    Clear.render(r, buf);
    let b = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " Runs of {} ",
            kb_core::terminal::display(&p.title)
        ))
        .border_style(theme.border());
    let inner = b.inner(r);
    b.render(r, buf);
    let iw = usize::from(inner.width);
    if p.confirming {
        let warn = Style::default().fg(Color::Yellow);
        buf.set_string(
            inner.x,
            inner.y,
            truncate(
                "This runbook was changed by an agent and is unreviewed.",
                iw,
            ),
            warn,
        );
        buf.set_string(
            inner.x,
            inner.y + 1,
            truncate("Start a run anyway? [y/N]", iw),
            warn,
        );
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    for (i, row) in p.rows.iter().enumerate() {
        let lock = row
            .locked
            .map_or(String::new(), |pid| format!(" [open in PID {pid}]"));
        let text = format!(
            "{} · {} · {}{}",
            kb_core::terminal::display(&row.name),
            row.progress,
            ago(now - row.updated),
            lock
        );
        let style = if i == p.selected {
            theme.selection()
        } else {
            theme.text()
        };
        buf.set_string(inner.x, inner.y + i as u16, truncate(&text, iw), style);
    }
    let y = inner.y + p.rows.len() as u16;
    if y < inner.bottom() {
        let style = if p.selected == p.rows.len() {
            theme.selection()
        } else {
            theme.text()
        };
        buf.set_string(inner.x, y, "Start a new run", style);
    }
    if inner.bottom() > inner.y + 1 {
        buf.set_string(
            inner.x,
            inner.bottom() - 1,
            truncate("Enter select · Esc back", iw),
            theme.muted(),
        );
    }
}
