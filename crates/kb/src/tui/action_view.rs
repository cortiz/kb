//! Action Menu popup overlay modal (Alt-A / F2).

use kb_core::action::{Action, ActionKind};
use kb_core::entry::EntryType;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::{Block, Borders, Clear, Widget};

use super::app::AppState;
use super::layout::truncate;
use super::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionMenuState {
    pub id: String,
    pub title: String,
    pub entry_type: Option<EntryType>,
    pub actions: Vec<Action>,
    pub has_local_clone: bool,
    pub is_ssh_without_display: bool,
}

/// One entry a note links to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkChoice {
    pub id: String,
    /// The target's type and title; `None` when the entry does not exist.
    pub target: Option<(EntryType, String)>,
}

/// The list opened by "Follow entry link" when a note links to more than one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPickerState {
    pub choices: Vec<LinkChoice>,
    pub selected: usize,
}

pub fn draw_link_picker(picker: &LinkPickerState, theme: &Theme, area: Rect, buf: &mut Buffer) {
    if area.width < 20 || area.height < 6 {
        return;
    }
    let popup_w = 72.min(area.width.saturating_sub(4)).max(36);
    let popup_h = ((picker.choices.len() as u16) + 4)
        .min(area.height.saturating_sub(4))
        .max(7);
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);
    Clear.render(popup, buf);
    Block::default()
        .borders(Borders::ALL)
        .title(" Follow entry link ")
        .border_style(theme.text().add_modifier(Modifier::BOLD))
        .render(popup, buf);
    let max_w = usize::from(popup.width.saturating_sub(4));
    let rows = usize::from(popup.height.saturating_sub(4));
    // keep the selected row in view
    let first = (picker.selected + 1).saturating_sub(rows);
    for (i, choice) in picker.choices.iter().enumerate().skip(first).take(rows) {
        let selected = i == picker.selected;
        let marker = if selected { "▸ " } else { "  " };
        let mut spans = vec![ratatui::text::Span::styled(
            marker,
            theme.text().add_modifier(Modifier::BOLD),
        )];
        match &choice.target {
            Some((ty, title)) => {
                spans.push(ratatui::text::Span::styled(
                    format!("[{}]", ty.name()),
                    theme.badge(*ty),
                ));
                spans.push(ratatui::text::Span::raw(" "));
                spans.push(ratatui::text::Span::styled(
                    truncate(title, max_w.saturating_sub(24)),
                    theme.text(),
                ));
                spans.push(ratatui::text::Span::styled(
                    format!("  {}", choice.id),
                    theme.muted(),
                ));
            }
            None => {
                spans.push(ratatui::text::Span::styled(
                    format!("[?] {}", choice.id),
                    theme.warning(),
                ));
                spans.push(ratatui::text::Span::styled(
                    "  (no such entry)",
                    theme.muted(),
                ));
            }
        }
        buf.set_line(
            popup.x + 2,
            popup.y + 1 + (i - first) as u16,
            &ratatui::text::Line::from(spans),
            max_w as u16,
        );
    }
    buf.set_string(
        popup.x + 2,
        popup.y + popup.height - 2,
        truncate("  ↑↓ choose • Enter go • Esc close", max_w),
        theme.muted(),
    );
}

pub fn draw_action_menu(
    _state: &AppState,
    menu: &ActionMenuState,
    theme: &Theme,
    area: Rect,
    buf: &mut Buffer,
) {
    if area.width < 20 || area.height < 6 {
        return;
    }

    let ty_str = menu.entry_type.map(|t| t.name()).unwrap_or("entry");
    let title = format!(" Actions: {} ({}) ", truncate(&menu.title, 36), ty_str);

    let popup_w = 64.min(area.width.saturating_sub(4)).max(36);
    let popup_h = ((menu.actions.len() as u16) + 4)
        .min(area.height.saturating_sub(4))
        .max(7);
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    Clear.render(popup, buf);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(theme.text().add_modifier(Modifier::BOLD));
    block.render(popup, buf);

    let inner_x = popup.x + 2;
    let mut inner_y = popup.y + 1;
    let max_w = usize::from(popup.width.saturating_sub(4));

    for a in &menu.actions {
        let available = a.is_available(menu.has_local_clone, menu.is_ssh_without_display);
        let suffix = if !available {
            " (disabled)"
        } else if a.is_primary {
            " (primary)"
        } else if a.kind == ActionKind::CustomCommand {
            " (custom)"
        } else {
            ""
        };

        let line = format!("  [{}] {}{}", a.key, a.name, suffix);
        let style = if available {
            theme.text()
        } else {
            theme.muted()
        };
        buf.set_string(inner_x, inner_y, truncate(&line, max_w), style);
        inner_y += 1;
        if inner_y >= popup.y + popup.height - 2 {
            break;
        }
    }

    let footer = "  Press letter to run • Esc to close";
    buf.set_string(
        inner_x,
        popup.y + popup.height - 2,
        truncate(footer, max_w),
        theme.muted(),
    );
}
