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
