//! Inline compact picker rendering in the normal terminal buffer directly below prompt.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Write};
use std::time::Duration;

use crossterm::cursor::{Hide, MoveDown, MoveToColumn, MoveUp, Show};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode, size,
};
use ratatui::Terminal;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use kb_core::entry::{Entry, EntryType};
use kb_core::query::Query;
use kb_core::search::Kb;
use kb_core::shell::ShellDialect;
use kb_core::vars::VarKind;

use super::inline_form::{InlineForm, InlineFormEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlineViewport {
    pub cols: u16,
    pub rows: u16,
    pub prompt_row: u16,
    pub prompt_col: u16,
    pub picker_height: u16,
    pub scrolled_lines: u16,
    pub use_alternate_screen: bool,
}

pub fn calculate_viewport(
    term_cols: u16,
    term_rows: u16,
    cursor_col: u16,
    cursor_row: u16,
    height_setting: &str,
) -> InlineViewport {
    if term_rows < 8 {
        return InlineViewport {
            cols: term_cols,
            rows: term_rows,
            prompt_row: cursor_row,
            prompt_col: cursor_col,
            picker_height: term_rows,
            scrolled_lines: 0,
            use_alternate_screen: true,
        };
    }

    let desired_height = if let Some(percent_str) = height_setting.strip_suffix('%') {
        let pct: u16 = percent_str.parse().unwrap_or(40);
        (term_rows as u32 * pct as u32 / 100) as u16
    } else {
        height_setting.parse().unwrap_or(10)
    };

    let clamped_height = desired_height
        .clamp(10, (term_rows - 1).max(10))
        .min(term_rows - 1);

    let available_below = term_rows.saturating_sub(cursor_row + 1);
    let scrolled_lines = clamped_height.saturating_sub(available_below);

    let prompt_row = cursor_row.saturating_sub(scrolled_lines);

    InlineViewport {
        cols: term_cols,
        rows: term_rows,
        prompt_row,
        prompt_col: cursor_col,
        picker_height: clamped_height,
        scrolled_lines,
        use_alternate_screen: false,
    }
}

pub fn teardown_sequences(viewport: &InlineViewport) -> Vec<u8> {
    if viewport.use_alternate_screen {
        let mut buf = Vec::new();
        let _ = execute!(&mut buf, Show, LeaveAlternateScreen);
        return buf;
    }

    let mut buf = Vec::new();
    for line in 1..=viewport.picker_height {
        let r = viewport.prompt_row + line + 1; // 1-indexed ANSI
        let _ = write!(&mut buf, "\x1b[{r};1H\x1b[2K");
    }
    let pr = viewport.prompt_row + 1;
    let pc = viewport.prompt_col + 1;
    let _ = write!(&mut buf, "\x1b[{pr};{pc}H\x1b[?25h");
    buf
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlinePickerResult {
    Picked {
        id: String,
        resolved_command: String,
        variables: BTreeMap<String, String>,
    },
    Cancelled,
    Refused(String),
}

#[derive(Debug, Clone)]
pub struct InlinePickerOptions {
    pub initial_query: String,
    pub default_type: Option<EntryType>,
    pub tag: Option<String>,
    pub height: String,
    pub preview: bool,
    pub dialect: Option<ShellDialect>,
}

impl Default for InlinePickerOptions {
    fn default() -> Self {
        Self {
            initial_query: String::new(),
            default_type: Some(EntryType::Cmd),
            tag: None,
            height: "40%".to_string(),
            preview: true,
            dialect: None,
        }
    }
}

fn open_tty_device() -> io::Result<File> {
    #[cfg(unix)]
    {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
    }
    #[cfg(not(unix))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "inline picker requires unix /dev/tty",
        ))
    }
}

pub fn run_inline_picker(
    kb: &mut Kb,
    options: InlinePickerOptions,
) -> io::Result<InlinePickerResult> {
    let mut tty = match open_tty_device() {
        Ok(f) => f,
        Err(_) => {
            return Ok(InlinePickerResult::Refused(
                "Could not open controlling terminal (/dev/tty)".to_string(),
            ));
        }
    };

    let (term_cols, term_rows) = size().unwrap_or((80, 24));
    let (cursor_x, cursor_y) = (0, 0);

    let viewport = calculate_viewport(term_cols, term_rows, cursor_x, cursor_y, &options.height);

    enable_raw_mode()?;

    if viewport.use_alternate_screen {
        let _ = execute!(tty, EnterAlternateScreen, Hide);
    } else {
        // Prepare room by outputting newlines and moving cursor back up
        let mut scroll_str = String::new();
        for _ in 0..viewport.picker_height {
            scroll_str.push('\n');
        }
        let _ = tty.write_all(scroll_str.as_bytes());
        let _ = execute!(tty, MoveUp(viewport.picker_height), MoveToColumn(0));
        let _ = tty.flush();
    }

    let mut mem_terminal = Terminal::new(TestBackend::new(term_cols, viewport.picker_height))
        .map_err(|e| io::Error::other(e.to_string()))?;
    let mut alt_terminal = if viewport.use_alternate_screen {
        Some(Terminal::new(CrosstermBackend::new(tty.try_clone()?))?)
    } else {
        None
    };

    let mut query = options.initial_query.clone();
    let mut selected_type = options.default_type;
    let mut selected_idx: usize = 0;
    let mut active_form: Option<InlineForm> = None;
    let mut confirming_unreviewed: Option<(String, String, BTreeMap<String, String>)> = None;

    let dialect = options.dialect.unwrap_or(ShellDialect::Zsh);

    let result = loop {
        // Collect entries
        let q = Query::parse(&query);
        let entries: Vec<_> = match kb.search(&q, std::time::SystemTime::now()) {
            Ok(outcome) => outcome
                .results
                .into_iter()
                .filter_map(|hit| kb.entry(&hit.id).ok())
                .filter(|e| {
                    if let Some(t) = selected_type {
                        e.entry_type() == Some(t)
                    } else {
                        true
                    }
                })
                .filter(|e| {
                    if let Some(tag) = &options.tag {
                        e.tags().iter().any(|t| t == tag)
                    } else {
                        true
                    }
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        if !entries.is_empty() && selected_idx >= entries.len() {
            selected_idx = entries.len().saturating_sub(1);
        }

        // Draw UI
        let unrev_preview = confirming_unreviewed.as_ref().map(|(id, _, _)| id.as_str());
        let draw_res = if let Some(alt) = &mut alt_terminal {
            alt.draw(|f| {
                render_inline_ui(
                    f,
                    &query,
                    selected_type,
                    &entries,
                    selected_idx,
                    active_form.as_ref(),
                    unrev_preview,
                    &options,
                );
            })
            .map(|_| ())
        } else {
            let res = mem_terminal
                .draw(|f| {
                    render_inline_ui(
                        f,
                        &query,
                        selected_type,
                        &entries,
                        selected_idx,
                        active_form.as_ref(),
                        unrev_preview,
                        &options,
                    );
                })
                .map(|_| ());
            if res.is_ok() {
                let _ = draw_test_backend_to_tty(
                    mem_terminal.backend().buffer(),
                    &mut tty,
                    term_cols,
                    viewport.picker_height,
                );
            }
            res.map_err(|e| io::Error::other(e.to_string()))
        };

        if let Err(e) = draw_res {
            break InlinePickerResult::Refused(format!("Draw error: {e}"));
        }

        // Wait for event
        if event::poll(Duration::from_millis(100)).unwrap_or(false)
            && let Ok(Event::Key(key)) = event::read()
        {
            if let Some((unrev_id, unrev_cmd, unrev_vars)) = confirming_unreviewed.take() {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        break InlinePickerResult::Picked {
                            id: unrev_id,
                            resolved_command: unrev_cmd,
                            variables: unrev_vars,
                        };
                    }
                    _ => {
                        break InlinePickerResult::Cancelled;
                    }
                }
            } else if let Some(form) = active_form.as_mut() {
                match form.handle_key(key) {
                    InlineFormEvent::Cancel => {
                        active_form = None;
                    }
                    InlineFormEvent::Submit(values) => {
                        let entry_id = form.entry_id.clone();
                        let entry = match kb.entry(&entry_id) {
                            Ok(e) => e,
                            Err(err) => {
                                break InlinePickerResult::Refused(err.to_string());
                            }
                        };

                        // Resolve variables using varres
                        let plan = kb_core::varres::plan_vars(&entry);
                        let mut val_map = kb_core::varres::Values::new();
                        for (k, v) in &values {
                            val_map.insert(k.clone(), kb_core::varres::Value::plain(v));
                        }

                        match kb_core::varres::resolve_for_target(
                            &plan,
                            &val_map,
                            kb_core::varres::ResolutionTarget::ShellWidget(dialect),
                        ) {
                            Ok(resolved) => {
                                let cmd = resolved
                                    .primary()
                                    .map(|o| o.text.clone())
                                    .unwrap_or_default();
                                let is_unreviewed =
                                    kb_core::mcp::review::ReviewStateTracker::is_unreviewed(
                                        kb.root(),
                                        &entry_id,
                                    );
                                if is_unreviewed {
                                    confirming_unreviewed = Some((entry_id, cmd, values));
                                    active_form = None;
                                } else {
                                    break InlinePickerResult::Picked {
                                        id: entry_id,
                                        resolved_command: cmd,
                                        variables: values,
                                    };
                                }
                            }
                            Err(errs) => {
                                let msg = errs
                                    .into_iter()
                                    .map(|e| e.to_string())
                                    .collect::<Vec<_>>()
                                    .join("; ");
                                break InlinePickerResult::Refused(msg);
                            }
                        }
                    }
                    InlineFormEvent::None => {}
                }
            } else {
                // Search list navigation
                match key.code {
                    KeyCode::Esc => {
                        break InlinePickerResult::Cancelled;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        break InlinePickerResult::Cancelled;
                    }
                    KeyCode::Up => {
                        selected_idx = selected_idx.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        if !entries.is_empty() && selected_idx + 1 < entries.len() {
                            selected_idx += 1;
                        }
                    }
                    KeyCode::Tab => {
                        selected_type = match selected_type {
                            Some(EntryType::Cmd) => None,
                            None => Some(EntryType::Cmd),
                            Some(_) => Some(EntryType::Cmd),
                        };
                        selected_idx = 0;
                    }
                    KeyCode::Enter => {
                        if let Some(entry) = entries.get(selected_idx) {
                            let vars = entry.vars();
                            if vars.is_empty() {
                                let resolved_command = entry
                                    .payload()
                                    .map(|p| p.text.clone())
                                    .unwrap_or_else(|| entry.notes().to_string());
                                let id = entry.id().to_string();
                                let is_unreviewed =
                                    kb_core::mcp::review::ReviewStateTracker::is_unreviewed(
                                        kb.root(),
                                        &id,
                                    );
                                if is_unreviewed {
                                    confirming_unreviewed =
                                        Some((id, resolved_command, BTreeMap::new()));
                                } else {
                                    break InlinePickerResult::Picked {
                                        id,
                                        resolved_command,
                                        variables: BTreeMap::new(),
                                    };
                                }
                            } else {
                                // Open inline form
                                let (mem, _) = kb.remembered(entry.id());
                                active_form = Some(InlineForm::new(entry, dialect, &mem));
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        query.pop();
                        selected_idx = 0;
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        query.push(c);
                        selected_idx = 0;
                    }
                    _ => {}
                }
            }
        }
    };

    // Teardown
    let _ = disable_raw_mode();
    if viewport.use_alternate_screen {
        let _ = execute!(tty, Show, LeaveAlternateScreen);
    } else {
        for i in 0..viewport.picker_height {
            let _ = execute!(tty, MoveToColumn(0), Clear(ClearType::CurrentLine));
            if i + 1 < viewport.picker_height {
                let _ = execute!(tty, MoveDown(1));
            }
        }
        let _ = execute!(tty, MoveUp(viewport.picker_height), Show);
    }
    let _ = tty.flush();

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn render_inline_ui(
    f: &mut ratatui::Frame,
    query: &str,
    selected_type: Option<EntryType>,
    entries: &[Entry],
    selected_idx: usize,
    active_form: Option<&InlineForm>,
    confirming_unreviewed: Option<&str>,
    options: &InlinePickerOptions,
) {
    let area = f.area();

    if let Some(unrev_id) = confirming_unreviewed {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Unreviewed Agent Change: {unrev_id} "))
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let lines = vec![
            Line::from(Span::styled(
                "WARNING: This entry was changed by an agent and has not been reviewed.",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Insert this command? [y/N]"),
        ];
        f.render_widget(Paragraph::new(lines), inner);
        return;
    }

    if let Some(form) = active_form {
        // Render Inline Form
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Variables for {} ", form.entry_id))
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(form.fields.len() as u16),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);

        let mut field_lines = Vec::new();
        for (i, field) in form.fields.iter().enumerate() {
            let is_active = i == form.focus;
            let cursor_mark = if is_active { "> " } else { "  " };
            let name_style = if is_active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let val_str = if field.secret {
                if let Some(env) = &field.env_var {
                    format!("from ${env}")
                } else {
                    "(no env source - use kb run)".to_string()
                }
            } else if field.kind == VarKind::Enum {
                format!("[{}] (←/→)", field.value)
            } else {
                format!("[{}]", field.value)
            };

            field_lines.push(Line::from(vec![
                Span::styled(cursor_mark, Style::default().fg(Color::Cyan)),
                Span::styled(format!("{}: ", field.name), name_style),
                Span::raw(val_str),
            ]));
        }
        f.render_widget(Paragraph::new(field_lines), chunks[0]);

        let sep = Paragraph::new("─".repeat(area.width as usize))
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(sep, chunks[1]);

        let preview_p = Paragraph::new(format!("Preview: {}", form.preview()))
            .style(Style::default().fg(Color::Green));
        f.render_widget(preview_p, chunks[2]);

        let help = Paragraph::new("[Tab] Next  [Enter] Pick  [Esc] Back")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(help, chunks[3]);
    } else {
        // Render Search List
        let show_preview = options.preview && area.width >= 120;
        let (list_area, preview_area) = if show_preview {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            (chunks[0], Some(chunks[1]))
        } else {
            (area, None)
        };

        let type_label = match selected_type {
            Some(EntryType::Cmd) => "cmd",
            Some(EntryType::Sql) => "sql",
            Some(EntryType::Sumo) => "sumo",
            Some(EntryType::Cw) => "cw",
            Some(EntryType::Prompt) => "prompt",
            Some(EntryType::Link) => "link",
            Some(EntryType::Code) => "code",
            Some(EntryType::Note) => "note",
            None => "all",
        };

        let title_str = format!(" Search: {} [{type_label}] ", query);
        let list_block = Block::default()
            .borders(Borders::ALL)
            .title(title_str)
            .border_style(Style::default().fg(Color::Blue));
        let list_inner = list_block.inner(list_area);
        f.render_widget(list_block, list_area);

        let list_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(list_inner);

        let items: Vec<ListItem> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let is_selected = i == selected_idx;
                let prefix = if is_selected { "> " } else { "  " };
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let line = Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::Cyan)),
                    Span::styled(entry.id(), style),
                    Span::raw("  "),
                    Span::raw(entry.title().unwrap_or("")),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list_widget = List::new(items);
        f.render_widget(list_widget, list_chunks[0]);

        let footer = Paragraph::new("[↑/↓] Navigate  [Tab] Type  [Enter] Pick  [Esc] Cancel")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(footer, list_chunks[1]);

        if let Some(p_area) = preview_area {
            let prev_block = Block::default()
                .borders(Borders::ALL)
                .title(" Preview ")
                .border_style(Style::default().fg(Color::DarkGray));
            let prev_inner = prev_block.inner(p_area);
            f.render_widget(prev_block, p_area);

            if let Some(entry) = entries.get(selected_idx) {
                let body = entry
                    .payload()
                    .map(|p| p.text.as_str())
                    .unwrap_or_else(|| entry.notes());
                f.render_widget(Paragraph::new(body), prev_inner);
            }
        }
    }
}

fn draw_test_backend_to_tty(
    buffer: &ratatui::buffer::Buffer,
    tty: &mut File,
    cols: u16,
    height: u16,
) -> io::Result<()> {
    for y in 0..height {
        let _ = execute!(tty, MoveToColumn(0), Clear(ClearType::CurrentLine));
        let mut row_str = String::new();
        let mut last_style = Style::default();

        for x in 0..cols {
            let cell = &buffer[(x, y)];
            let style = cell.style();
            if style != last_style {
                row_str.push_str("\x1b[0m");
                row_str.push_str(&ansi_for_style(style));
                last_style = style;
            }
            row_str.push_str(cell.symbol());
        }
        row_str.push_str("\x1b[0m");
        let _ = tty.write_all(row_str.as_bytes());
        if y + 1 < height {
            let _ = execute!(tty, MoveDown(1));
        }
    }
    if height > 1 {
        let _ = execute!(tty, MoveUp(height - 1), MoveToColumn(0));
    }
    tty.flush()?;
    Ok(())
}

fn ansi_for_style(style: Style) -> String {
    let mut s = String::new();
    if style.add_modifier.contains(Modifier::BOLD) {
        s.push_str("\x1b[1m");
    }
    match style.fg {
        Some(Color::Black) => s.push_str("\x1b[30m"),
        Some(Color::Red) => s.push_str("\x1b[31m"),
        Some(Color::Green) => s.push_str("\x1b[32m"),
        Some(Color::Yellow) => s.push_str("\x1b[33m"),
        Some(Color::Blue) => s.push_str("\x1b[34m"),
        Some(Color::Magenta) => s.push_str("\x1b[35m"),
        Some(Color::Cyan) => s.push_str("\x1b[36m"),
        Some(Color::Gray) => s.push_str("\x1b[37m"),
        Some(Color::DarkGray) => s.push_str("\x1b[90m"),
        Some(Color::LightRed) => s.push_str("\x1b[91m"),
        Some(Color::LightGreen) => s.push_str("\x1b[92m"),
        Some(Color::LightYellow) => s.push_str("\x1b[93m"),
        Some(Color::LightBlue) => s.push_str("\x1b[94m"),
        Some(Color::LightMagenta) => s.push_str("\x1b[95m"),
        Some(Color::LightCyan) => s.push_str("\x1b[96m"),
        Some(Color::White) => s.push_str("\x1b[97m"),
        Some(Color::Rgb(r, g, b)) => s.push_str(&format!("\x1b[38;2;{r};{g};{b}m")),
        Some(Color::Indexed(n)) => s.push_str(&format!("\x1b[38;5;{n}m")),
        _ => {}
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_height_clamping() {
        // Standard terminal 50 rows: 40% = 20 lines
        let vp = calculate_viewport(100, 50, 10, 5, "40%");
        assert_eq!(vp.picker_height, 20);
        assert!(!vp.use_alternate_screen);

        // Small terminal 20 rows: 40% = 8 lines -> clamped to min 10 lines
        let vp = calculate_viewport(100, 20, 10, 5, "40%");
        assert_eq!(vp.picker_height, 10);
        assert!(!vp.use_alternate_screen);

        // Terminal with 12 rows: min 10, max 11 (12 - 1)
        let vp = calculate_viewport(100, 12, 10, 0, "40%");
        assert_eq!(vp.picker_height, 10);
        assert!(!vp.use_alternate_screen);

        // Explicit line count "15"
        let vp = calculate_viewport(100, 50, 10, 5, "15");
        assert_eq!(vp.picker_height, 15);

        // Tiny terminal < 8 rows falls back to alternate screen
        let vp = calculate_viewport(80, 7, 5, 2, "40%");
        assert!(vp.use_alternate_screen);
        assert_eq!(vp.picker_height, 7);
    }

    #[test]
    fn test_viewport_scrolling_calculation() {
        // Prompt near bottom: 50 rows, cursor at row 45 (0-indexed).
        // Rows available below: 50 - 1 - 45 = 4 rows.
        // Desired picker height: 20 rows.
        // Need to scroll 20 - 4 = 16 rows.
        let vp = calculate_viewport(100, 50, 15, 45, "40%");
        assert_eq!(vp.picker_height, 20);
        assert_eq!(vp.scrolled_lines, 16);
        assert_eq!(vp.prompt_row, 29); // 45 - 16
        assert_eq!(vp.prompt_col, 15);
    }

    #[test]
    fn test_teardown_clears_lines_and_restores_cursor() {
        let vp = InlineViewport {
            cols: 80,
            rows: 24,
            prompt_row: 5,
            prompt_col: 12,
            picker_height: 10,
            scrolled_lines: 0,
            use_alternate_screen: false,
        };

        let seq = teardown_sequences(&vp);
        let seq_str = String::from_utf8_lossy(&seq);

        // Check line clearing sequences (CSI 2K)
        assert!(seq_str.contains("\x1b[2K"));
        // Check cursor restoration to (5, 12) (1-indexed ANSI: row 6, col 13)
        assert!(seq_str.contains("\x1b[6;13H"));
    }
}
