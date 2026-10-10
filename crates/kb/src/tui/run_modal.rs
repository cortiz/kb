//! Command execution confirmation modal and destructive warning prompt.

use kb_core::run::RunPlan;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Widget};
use std::time::Duration;

use super::app::AppState;
use super::layout::truncate;
use super::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunModalState {
    Confirming {
        id: String,
        plan: RunPlan,
        input: String,
        error: Option<String>,
    },
    Paused {
        id: String,
        exit_code: i32,
        duration: Duration,
    },
}

pub fn draw_run_modal(
    _state: &AppState,
    modal: &RunModalState,
    theme: &Theme,
    area: Rect,
    buf: &mut Buffer,
) {
    if area.width < 20 || area.height < 6 {
        return;
    }

    let popup_w = 76.min(area.width.saturating_sub(4)).max(40);
    let popup_h = 16.min(area.height.saturating_sub(4)).max(8);
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    Clear.render(popup, buf);

    match modal {
        RunModalState::Confirming {
            id,
            plan,
            input,
            error,
        } => {
            let title = format!(" Run Command ({id}) ");
            let block = Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(if plan.requires_typed_run {
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD)
                } else {
                    theme.text().add_modifier(Modifier::BOLD)
                });
            block.render(popup, buf);

            let inner_x = popup.x + 2;
            let mut inner_y = popup.y + 1;
            let max_w = usize::from(popup.width.saturating_sub(4));

            // Command
            let cmd_line = format!("Command: {}", plan.resolved_command);
            buf.set_string(
                inner_x,
                inner_y,
                truncate(&cmd_line, max_w),
                theme.text().add_modifier(Modifier::BOLD),
            );
            inner_y += 1;

            // Shell
            let shell_line = format!("Shell:   {}", plan.shell);
            buf.set_string(inner_x, inner_y, truncate(&shell_line, max_w), theme.text());
            inner_y += 1;

            // Cwd
            if let Some(ref cwd) = plan.cwd {
                let cwd_line = format!("Cwd:     {}", cwd.display());
                buf.set_string(inner_x, inner_y, truncate(&cwd_line, max_w), theme.text());
                inner_y += 1;
            }

            // Secrets
            if !plan.secret_env_vars.is_empty() {
                let sec_names: Vec<_> = plan
                    .secret_env_vars
                    .iter()
                    .map(|s| s.env_key.as_str())
                    .collect();
                let sec_line = format!("Secrets: {}", sec_names.join(", "));
                buf.set_string(
                    inner_x,
                    inner_y,
                    truncate(&sec_line, max_w),
                    Style::default().fg(Color::Yellow),
                );
                inner_y += 1;
            }

            // Destructive warnings
            if !plan.destructive_warnings.is_empty() {
                inner_y += 1;
                buf.set_string(
                    inner_x,
                    inner_y,
                    truncate("WARNING: Destructive operations matched:", max_w),
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                );
                inner_y += 1;
                for warn in &plan.destructive_warnings {
                    if inner_y < popup.bottom() - 3 {
                        let warn_line = format!("  - {}", warn.description);
                        buf.set_string(
                            inner_x,
                            inner_y,
                            truncate(&warn_line, max_w),
                            Style::default().fg(Color::LightRed),
                        );
                        inner_y += 1;
                    }
                }
            }

            // Input / Prompt at bottom
            let prompt_y = popup.bottom() - 3;
            if plan.requires_typed_run {
                let prompt_line = format!("Type 'run' to execute: {input}▌");
                buf.set_string(
                    inner_x,
                    prompt_y,
                    truncate(&prompt_line, max_w),
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                );
                if let Some(err) = error {
                    buf.set_string(
                        inner_x,
                        prompt_y + 1,
                        truncate(err, max_w),
                        Style::default().fg(Color::LightRed),
                    );
                } else {
                    buf.set_string(
                        inner_x,
                        prompt_y + 1,
                        truncate("Esc cancel · Enter run", max_w),
                        theme.text(),
                    );
                }
            } else {
                buf.set_string(
                    inner_x,
                    prompt_y,
                    truncate("Press Enter or 'y' to run, Esc to cancel", max_w),
                    theme.text().add_modifier(Modifier::BOLD),
                );
            }
        }
        RunModalState::Paused {
            id,
            exit_code,
            duration,
        } => {
            let title = format!(" Run Finished ({id}) ");
            let block = Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(if *exit_code == 0 {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::LightRed)
                });
            block.render(popup, buf);

            let inner_x = popup.x + 2;
            let mut inner_y = popup.y + 2;
            let max_w = usize::from(popup.width.saturating_sub(4));

            let status_line = format!(
                "Command exited with status {} (exit code {}) in {:.2}s",
                exit_code,
                exit_code,
                duration.as_secs_f64()
            );
            let style = if *exit_code == 0 {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD)
            };
            buf.set_string(inner_x, inner_y, truncate(&status_line, max_w), style);
            inner_y += 2;

            buf.set_string(
                inner_x,
                inner_y,
                truncate("Press any key to continue...", max_w),
                theme.text().add_modifier(Modifier::BOLD),
            );
        }
    }
}
