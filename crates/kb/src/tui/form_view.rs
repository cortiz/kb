//! Drawing the variable form (screen 2): a header, one block per variable, the live preview of the
//! resolved text, and a key bar. A pure function of the state, like `view::draw`.

use kb_core::varres::{MASK, Source, Target};
use kb_core::vars::{Escape, VarKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};

use super::app::{AppState, MessageKind};
use super::form::{Field, FormState, KEY_BAR};
use super::layout::{WIDE, truncate, width};
use super::theme::Theme;

/// Lines of preview under the fields on a narrow terminal.
const NARROW_PREVIEW_LINES: u16 = 8;

pub fn draw_form(state: &AppState, form: &FormState, theme: &Theme, area: Rect, buf: &mut Buffer) {
    draw_header(form, theme, Rect::new(area.x, area.y, area.width, 1), buf);
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(2),
    );
    let (fields, preview) = if area.width >= WIDE {
        let left = (area.width * 45 / 100).max(40);
        (
            Rect::new(body.x, body.y, left, body.height),
            Rect::new(body.x + left, body.y, area.width - left, body.height),
        )
    } else {
        let ph = (NARROW_PREVIEW_LINES + 2).min(body.height / 2);
        (
            Rect::new(body.x, body.y, body.width, body.height - ph),
            Rect::new(body.x, body.y + body.height - ph, body.width, ph),
        )
    };
    draw_fields(form, theme, fields, buf);
    draw_preview(form, theme, preview, buf);
    draw_keybar(
        state,
        form,
        theme,
        Rect::new(area.x, area.y + area.height - 1, area.width, 1),
        buf,
    );
}

fn draw_header(form: &FormState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let counter = format!("{} ", form.counter());
    let room = usize::from(r.width).saturating_sub(width(&counter) + 12);
    let line = Line::from(vec![
        Span::styled(" kb", theme.text().add_modifier(Modifier::BOLD)),
        Span::styled(" · variables · ", theme.muted()),
        Span::styled(
            truncate(&form.title, room),
            theme.text().add_modifier(Modifier::BOLD),
        ),
    ]);
    buf.set_line(r.x, r.y, &line, r.width);
    let x = r.x + r.width.saturating_sub(width(&counter) as u16);
    buf.set_string(x, r.y, &counter, theme.muted());
}

/// The rows a field takes: its name line, its input line, an error line, and the suggestions
/// when it has the focus.
fn field_rows(form: &FormState, i: usize) -> usize {
    let f = &form.fields[i];
    let mut rows = 2 + usize::from(f.error.is_some());
    if i == form.focus {
        rows += form.suggestions().len();
    }
    rows
}

fn draw_fields(form: &FormState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let avail = usize::from(r.height);
    // scroll so the focused field is fully visible
    let mut start = 0;
    while start < form.focus
        && (start..=form.focus)
            .map(|i| field_rows(form, i))
            .sum::<usize>()
            > avail
    {
        start += 1;
    }
    let mut y = r.y;
    let end = r.y + r.height;
    for i in start..form.fields.len() {
        let rows = field_rows(form, i) as u16;
        if y + rows > end && i != start {
            break;
        }
        y = draw_field(
            form,
            i,
            theme,
            Rect::new(r.x, y, r.width, end.saturating_sub(y)),
            buf,
        );
    }
}

fn source_label(f: &Field) -> String {
    match f.source {
        Source::Env => format!("from ${}", f.var.env.as_deref().unwrap_or("env")),
        Source::Remembered => "remembered".into(),
        Source::Default => "default".into(),
        Source::Typed => "edited".into(),
        Source::Flag => "flag".into(),
        Source::None => "required".into(),
    }
}

fn kind_label(f: &Field) -> String {
    match f.var.kind {
        VarKind::Enum => "enum".into(),
        k => VarKind::NAMES[k as usize].into(),
    }
}

/// Draws one field at the top of `r`; returns the row after it.
fn draw_field(form: &FormState, i: usize, theme: &Theme, r: Rect, buf: &mut Buffer) -> u16 {
    let f = &form.fields[i];
    let focused = i == form.focus;
    let mut y = r.y;
    let marker = if focused { "▸ " } else { "  " };
    let name_style = if focused {
        theme.text().add_modifier(Modifier::BOLD)
    } else {
        theme.text()
    };
    let mut spans = vec![
        Span::styled(marker, theme.warning()),
        Span::styled(f.var.name.clone(), name_style),
        Span::styled(format!("  {}", kind_label(f)), theme.muted()),
    ];
    if matches!(f.var.kind, VarKind::Number | VarKind::Text)
        && let Some(d) = f
            .var
            .default
            .as_ref()
            .filter(|_| f.source != Source::Default)
    {
        spans.push(Span::styled(format!("  default {d}"), theme.muted()));
    }
    spans.push(Span::styled(
        format!("  · {}", source_label(f)),
        theme.muted(),
    ));
    match f.var.escape {
        Escape::None => spans.push(Span::styled("  raw", theme.warning())),
        Escape::Shell => spans.push(Span::styled("  shell-quoted", theme.muted())),
        Escape::Sql => spans.push(Span::styled("  sql-quoted", theme.muted())),
        Escape::Auto => {}
    }
    if !f.var.declared {
        spans.push(Span::styled("  undeclared", theme.warning()));
    }
    buf.set_line(r.x, y, &Line::from(spans), r.width);
    y += 1;
    draw_input(
        f,
        focused,
        theme,
        Rect::new(r.x + 2, y, r.width.saturating_sub(2), 1),
        buf,
    );
    y += 1;
    if let Some(e) = &f.error {
        let text = truncate(&format!("✗ {e}"), usize::from(r.width.saturating_sub(2)));
        buf.set_string(r.x + 2, y, text, theme.warning());
        y += 1;
    }
    if focused {
        let sel = form.suggest;
        for (n, item) in form.suggestions().iter().enumerate() {
            let style = if sel == Some(n) {
                theme.selection()
            } else {
                theme.muted()
            };
            let text = truncate(&format!("↳ {item}"), usize::from(r.width.saturating_sub(4)));
            buf.set_string(r.x + 4, y, text, style);
            y += 1;
        }
    }
    y
}

fn draw_input(f: &Field, focused: bool, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let w = usize::from(r.width);
    if f.var.kind == VarKind::Enum {
        let mut spans = Vec::new();
        for (i, o) in f.var.options.iter().enumerate() {
            let chosen = f.choice == Some(i);
            let style = match (chosen, focused) {
                (true, true) => theme.selection(),
                (true, false) => theme.text().add_modifier(Modifier::BOLD),
                (false, _) => theme.muted(),
            };
            let text = if chosen {
                format!("[{o}]")
            } else {
                format!(" {o} ")
            };
            spans.push(Span::styled(text, style));
            spans.push(Span::raw(" "));
        }
        buf.set_line(r.x, r.y, &Line::from(spans), r.width);
        return;
    }
    if f.locked {
        let env = f.var.env.as_deref().unwrap_or("env");
        let mut text = format!("{MASK} (from ${env})");
        if focused {
            text.push_str("   ^R replace");
        }
        buf.set_string(r.x, r.y, truncate(&text, w), theme.text());
        return;
    }
    let shown: Vec<char> = if f.secret() {
        if f.editor.is_empty() {
            Vec::new()
        } else {
            MASK.chars().collect()
        }
    } else {
        f.editor.text().chars().collect()
    };
    // the cursor in a masked field sits at the end of the mask
    let cursor = if f.secret() {
        shown.len()
    } else {
        f.editor.cursor()
    };
    let mut start = 0;
    while start < cursor
        && shown[start..cursor]
            .iter()
            .map(|c| width(&c.to_string()))
            .sum::<usize>()
            >= w
    {
        start += 1;
    }
    let mut x = r.x;
    let mut used = 0;
    for (i, c) in shown.iter().enumerate().skip(start) {
        let s = c.to_string();
        let cw = width(&s);
        if used + cw > w {
            break;
        }
        let style = if focused && i == cursor {
            theme.cursor()
        } else {
            theme.text()
        };
        buf.set_string(x, r.y, &s, style);
        x += cw as u16;
        used += cw;
    }
    if focused && cursor >= shown.len() && used < w {
        buf.set_string(x, r.y, " ", theme.cursor());
    } else if shown.is_empty() && !focused {
        buf.set_string(r.x, r.y, "(empty)", theme.muted());
    }
}

fn draw_preview(form: &FormState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border())
        .title(Span::styled(" Preview ", theme.muted()));
    let inner = block.inner(r);
    block.render(r, buf);
    let resolved = form.preview();
    let Some(primary) = resolved.primary() else {
        return;
    };
    // fields outside the payload (`cwd`, `range`, `log_groups`, ...) come first, one per line
    let mut text = String::new();
    let mut marks: Vec<std::ops::Range<usize>> = Vec::new();
    for out in resolved
        .outputs
        .iter()
        .filter(|o| o.target != primary.target)
    {
        text.push_str(&format!("{}: ", target_label(&out.target)));
        let (shown, m) = out.display();
        marks.extend(
            m.into_iter()
                .map(|r| r.start + text.len()..r.end + text.len()),
        );
        text.push_str(&shown);
        text.push('\n');
    }
    let (shown, m) = primary.display();
    marks.extend(
        m.into_iter()
            .map(|r| r.start + text.len()..r.end + text.len()),
    );
    text.push_str(&shown);
    // ponytail: the preview shows the top of the text and does not scroll; add Shift-Up/Down like the
    // main preview if a payload taller than the pane ever matters
    // long lines wrap, so the whole result can be read
    let mut y = inner.y;
    let end = inner.y + inner.height;
    let mut offset = 0;
    for raw in text.split('\n') {
        // the final empty piece after a trailing newline is not a line
        if offset >= text.len() || y >= end {
            break;
        }
        let mut x = inner.x;
        for (i, c) in raw.char_indices() {
            let s = c.to_string();
            let cw = width(&s) as u16;
            if x + cw > inner.x + inner.width {
                y += 1;
                x = inner.x;
                if y >= end {
                    return;
                }
            }
            let style: Style = if marks.iter().any(|m| m.contains(&(offset + i))) {
                theme.variable().add_modifier(Modifier::BOLD)
            } else {
                theme.text()
            };
            buf.set_string(x, y, &s, style);
            x += cw;
        }
        offset += raw.len() + 1;
        y += 1;
    }
}

fn target_label(t: &Target) -> String {
    match t {
        Target::Payload => "payload".into(),
        Target::Url => "url".into(),
        Target::Cwd => "cwd".into(),
        Target::Range => "range".into(),
        Target::LogGroup(i) => format!("log_groups[{i}]"),
    }
}

fn draw_keybar(state: &AppState, form: &FormState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let w = usize::from(r.width).saturating_sub(2);
    let (text, style) = if let Some(m) = &state.message {
        let style = if m.kind == MessageKind::Warning {
            theme.warning()
        } else {
            theme.text()
        };
        (truncate(&m.text, w), style)
    } else if let Some(n) = &form.notice {
        (truncate(n, w), theme.warning())
    } else {
        let mut bar = KEY_BAR.to_string();
        let f = &form.fields[form.focus];
        if f.locked {
            bar.push_str(" · ^R replace");
        }
        if !form.suggestions().is_empty() {
            bar.push_str(" · Alt-↓ recent");
        }
        (truncate(&bar, w), theme.muted())
    };
    buf.set_string(r.x + 1, r.y, text, style);
}
