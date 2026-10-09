//! The add form (screen 4): drawing. A pure function of the state, like `view::draw`.

use kb_core::entry::EntryType;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};

use super::add::{AddState, Field};
use super::app::{AppState, MessageKind};
use super::input::LineEditor;
use super::layout::{WIDE, pad, truncate, width};
use super::theme::Theme;

/// The key bar when nothing else needs saying.
pub const KEY_BAR: &str = "^S save · ^E edit body · ^P preview · Tab next · Esc discard";
const KEY_BAR_NO_BODY: &str = "^S save · ^P preview · Tab next · Esc discard";
/// Where the label ends and the input begins in the left column.
const LABEL: u16 = 9;

pub fn draw_add(state: &AppState, add: &AddState, theme: &Theme, area: Rect, buf: &mut Buffer) {
    if area.height < 3 {
        return;
    }
    let header = format!(" {}", add.header());
    buf.set_string(
        area.x,
        area.y,
        truncate(&header, usize::from(area.width)),
        theme.text().add_modifier(Modifier::BOLD),
    );
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(2),
    );
    if let Some(model) = &add.preview {
        let src = super::view::render_preview(model, theme, body.width.saturating_sub(2).max(10));
        blit(
            &src,
            buf,
            Rect::new(
                body.x + 1,
                body.y,
                body.width.saturating_sub(2),
                body.height,
            ),
        );
    } else if area.width >= WIDE {
        let left_w = (area.width * 45 / 100).max(44).min(area.width / 2 + 6);
        let left = Rect::new(body.x, body.y, left_w, body.height);
        let right = Rect::new(body.x + left_w, body.y, area.width - left_w, body.height);
        draw_left(add, theme, left, buf);
        draw_right(add, theme, right, buf);
    } else {
        let left_h = left_rows(add).min(body.height.saturating_sub(5)).max(6);
        let left = Rect::new(body.x, body.y, body.width, left_h);
        let right = Rect::new(body.x, body.y + left_h, body.width, body.height - left_h);
        draw_left(add, theme, left, buf);
        draw_right(add, theme, right, buf);
    }
    draw_keybar(
        state,
        add,
        theme,
        Rect::new(area.x, area.y + area.height - 1, area.width, 1),
        buf,
    );
}

/// Copies `src` into `dst` at `area`, as far as both reach.
fn blit(src: &Buffer, dst: &mut Buffer, area: Rect) {
    for y in 0..src.area.height.min(area.height) {
        for x in 0..src.area.width.min(area.width) {
            if let (Some(from), Some(to)) =
                (src.cell((x, y)), dst.cell_mut((area.x + x, area.y + y)))
            {
                *to = from.clone();
            }
        }
    }
}

/// Rows the left column wants.
fn left_rows(add: &AddState) -> u16 {
    let vars = if add.draft.entry_type.has_payload() {
        2 + add.declared().len().max(1) + add.draft.suggestions.len()
    } else {
        0
    };
    (4 + vars + 2) as u16
}

fn label(buf: &mut Buffer, r: Rect, y: u16, text: &str, focused: bool, theme: &Theme) {
    let style = if focused {
        theme.text().add_modifier(Modifier::BOLD)
    } else {
        theme.muted()
    };
    let marker = if focused { "▸" } else { " " };
    buf.set_string(r.x, y, format!("{marker}{text}"), style);
}

fn draw_left(add: &AddState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let end = r.y + r.height;
    let mut y = r.y;
    let input_x = r.x + LABEL;
    let input_w = r.width.saturating_sub(LABEL + 1);
    let focused = |f: Field| add.focus == f;

    // type chips
    label(buf, r, y, "Type", focused(Field::Type), theme);
    let mut spans = Vec::new();
    for ty in EntryType::ALL {
        if ty == add.draft.entry_type {
            let style = if focused(Field::Type) {
                theme.badge(ty).add_modifier(Modifier::REVERSED)
            } else {
                theme.badge(ty)
            };
            spans.push(Span::styled(format!("[{}]", ty.name()), style));
        } else {
            spans.push(Span::styled(ty.name().to_string(), theme.muted()));
        }
        spans.push(Span::raw(" "));
    }
    buf.set_line(input_x, y, &Line::from(spans), input_w);
    y += 2;

    // title
    if y < end {
        label(buf, r, y, "Title", focused(Field::Title), theme);
        draw_input(
            buf,
            Rect::new(input_x, y, input_w.saturating_sub(7), 1),
            &add.title,
            focused(Field::Title),
            "(required)",
            theme,
        );
        let n = add.title.text().chars().count();
        if n >= 100 {
            let counter = format!("{n}/{}", kb_core::entry::MAX_TITLE_CHARS);
            let style = if n > kb_core::entry::MAX_TITLE_CHARS {
                theme.warning()
            } else {
                theme.muted()
            };
            buf.set_string(
                r.x + r.width.saturating_sub(counter.len() as u16 + 1),
                y,
                counter,
                style,
            );
        }
        y += 1;
    }
    // tags and their completions
    if y < end {
        label(buf, r, y, "Tags", focused(Field::Tags), theme);
        let tags_w = input_w;
        draw_input(
            buf,
            Rect::new(input_x, y, tags_w, 1),
            &add.tags,
            focused(Field::Tags),
            "(comma separated)",
            theme,
        );
        if !add.tag_text_ok() {
            buf.set_string(r.x + r.width.saturating_sub(2), y, "✗", theme.warning());
        }
        y += 1;
    }
    let completions = add.completions();
    if y < end && !completions.is_empty() {
        let chips: Vec<Span<'_>> = completions
            .iter()
            .flat_map(|c| {
                [
                    Span::styled(format!("‹{c}›"), theme.chip_dim()),
                    Span::raw(" "),
                ]
            })
            .collect();
        buf.set_line(input_x, y, &Line::from(chips), input_w);
        y += 1;
    }
    y += 1;

    // variables
    if add.draft.entry_type.has_payload() && y < end {
        label(buf, r, y, "Variables", focused(Field::Variables), theme);
        y += 1;
        if add.declared().is_empty() && y < end {
            buf.set_string(
                r.x + 2,
                y,
                "(none: write {{name}} in the body)",
                theme.muted(),
            );
            y += 1;
        }
        for v in add.declared() {
            if y >= end {
                break;
            }
            let kind = pad(super::add::kind_name(v.kind), 7);
            let mut spans = vec![
                Span::raw("  "),
                Span::styled(pad(&v.name, 12), theme.text()),
                Span::styled(kind, theme.muted()),
                Span::styled(format!("{{{{{}}}}}", v.name), theme.variable()),
            ];
            if let Some(d) = &v.default {
                spans.push(Span::styled(format!("  = {d}"), theme.muted()));
            }
            buf.set_line(r.x, y, &Line::from(spans), r.width);
            y += 1;
        }
        y += 1;
    }

    // where it goes
    if y < end {
        label(buf, r, y, "Saves to", focused(Field::Path), theme);
        let derived = add
            .derived_path()
            .map(|p| format!("{p}.md"))
            .unwrap_or_else(|| "(type a path)".into());
        if add.path.is_empty() && !focused(Field::Path) {
            buf.set_string(
                input_x,
                y,
                truncate(&derived, usize::from(input_w)),
                theme.muted(),
            );
        } else {
            draw_input(
                buf,
                Rect::new(input_x, y, input_w, 1),
                &add.path,
                focused(Field::Path),
                &derived,
                theme,
            );
        }
    }
}

fn draw_right(add: &AddState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    if r.height < 3 {
        return;
    }
    let notes_h = (r.height / 3).clamp(4, 8).min(r.height - 2);
    let top = Rect::new(r.x, r.y, r.width, r.height - notes_h);
    let bottom = Rect::new(r.x, r.y + top.height, r.width, notes_h);
    if add.draft.entry_type.has_payload() {
        draw_body(add, theme, top, buf);
    } else {
        draw_details(add, theme, top, buf);
    }
    draw_notes(add, theme, bottom, buf);
}

fn boxed(title: &str, focused: bool, theme: &Theme, r: Rect, buf: &mut Buffer) -> Rect {
    let style = if focused {
        theme.text()
    } else {
        theme.border()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(Span::styled(
            format!(" {title} "),
            if focused {
                theme.text().add_modifier(Modifier::BOLD)
            } else {
                theme.muted()
            },
        ));
    let inner = block.inner(r);
    block.render(r, buf);
    inner
}

fn draw_body(add: &AddState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let inner = boxed("Body", add.focus == Field::Body, theme, r, buf);
    if inner.width < 6 || inner.height == 0 {
        return;
    }
    if add.body_lines.is_empty() {
        buf.set_string(
            inner.x + 1,
            inner.y,
            "(empty: ^E to write it in your editor)",
            theme.muted(),
        );
        return;
    }
    let gutter = add.body_lines.len().to_string().len();
    let cols = usize::from(inner.width).saturating_sub(gutter + 2);
    // byte offset of the start of each line, to place the secret ranges
    let mut starts = Vec::with_capacity(add.body_lines.len());
    let mut at = 0;
    for line in add.draft.body.split('\n') {
        starts.push(at);
        at += line.len() + 1;
    }
    for (row, line) in add
        .body_lines
        .iter()
        .enumerate()
        .skip(add.body_scroll)
        .take(usize::from(inner.height))
    {
        let y = inner.y + (row - add.body_scroll) as u16;
        let mut spans = vec![Span::styled(
            format!("{:>gutter$} ", row + 1),
            theme.muted(),
        )];
        let mut used = 0;
        for s in line {
            let text = truncate(&s.text, cols.saturating_sub(used));
            used += width(&text);
            spans.push(Span::styled(text, theme.class_style(s.class)));
        }
        buf.set_line(inner.x + 1, y, &Line::from(spans), inner.width - 1);
        // likely secrets on this line
        let line_start = starts.get(row).copied().unwrap_or(0);
        let line_end = line_start + add.draft.body.split('\n').nth(row).map_or(0, str::len);
        for range in &add.secret {
            if range.start < line_end && range.end > line_start {
                let from = add.draft.body[line_start..range.start.max(line_start)]
                    .chars()
                    .count();
                let to = add.draft.body[line_start..range.end.min(line_end)]
                    .chars()
                    .count();
                for col in from..to {
                    let x = inner.x + 1 + (gutter + 1 + col) as u16;
                    if x < inner.x + inner.width {
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            cell.set_style(theme.warning().add_modifier(Modifier::REVERSED));
                        }
                    }
                }
            }
        }
    }
    if add.body_scroll + usize::from(inner.height) < add.body_lines.len() {
        let t = "↓ more";
        buf.set_string(
            inner.x + inner.width - width(t) as u16 - 1,
            inner.y + inner.height - 1,
            t,
            theme.muted(),
        );
    }
}

fn draw_details(add: &AddState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let inner = boxed(
        if add.draft.entry_type == EntryType::Link {
            "Link"
        } else {
            "Code location"
        },
        add.focus == Field::Fields,
        theme,
        r,
        buf,
    );
    for (i, (label, editor)) in add.type_fields.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        let y = inner.y + i as u16;
        let focused = add.focus == Field::Fields && add.type_row == i;
        buf.set_string(
            inner.x + 1,
            y,
            pad(label, 9),
            if focused {
                theme.text().add_modifier(Modifier::BOLD)
            } else {
                theme.muted()
            },
        );
        let x = inner.x + 10;
        draw_input(
            buf,
            Rect::new(x, y, inner.width.saturating_sub(11), 1),
            editor,
            focused,
            "",
            theme,
        );
    }
}

fn draw_notes(add: &AddState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let inner = boxed("Notes (optional)", add.focus == Field::Notes, theme, r, buf);
    if inner.width < 4 || inner.height == 0 {
        return;
    }
    let (row, col) = add.notes.position();
    let first = row.saturating_sub(usize::from(inner.height) - 1);
    let cols = usize::from(inner.width) - 2;
    for (i, line) in add
        .notes
        .lines()
        .iter()
        .enumerate()
        .skip(first)
        .take(usize::from(inner.height))
    {
        buf.set_string(
            inner.x + 1,
            inner.y + (i - first) as u16,
            truncate(line, cols),
            theme.text(),
        );
    }
    if add.focus == Field::Notes {
        let x = inner.x + 1 + col.min(cols) as u16;
        let y = inner.y + (row - first) as u16;
        if x < inner.x + inner.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_style(theme.cursor());
            }
        }
    }
}

/// A one-line input with a cursor; `hint` is shown dimmed when it is empty.
pub fn draw_input(
    buf: &mut Buffer,
    r: Rect,
    editor: &LineEditor,
    focused: bool,
    hint: &str,
    theme: &Theme,
) {
    let w = usize::from(r.width);
    if w == 0 {
        return;
    }
    let shown: Vec<char> = editor.text().chars().collect();
    if shown.is_empty() {
        if focused {
            buf.set_string(r.x, r.y, " ", theme.cursor());
            buf.set_string(
                r.x + 1,
                r.y,
                truncate(hint, w.saturating_sub(1)),
                theme.muted(),
            );
        } else {
            buf.set_string(r.x, r.y, truncate(hint, w), theme.muted());
        }
        return;
    }
    let cursor = editor.cursor();
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
    }
}

fn draw_keybar(state: &AppState, add: &AddState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let w = usize::from(r.width).saturating_sub(2);
    let (text, style): (String, Style) = if add.confirm_discard {
        ("Discard this entry? (y/N)".into(), theme.warning())
    } else if let Some(m) = &add.message {
        (m.clone(), theme.warning())
    } else if let Some(m) = &state.message {
        (
            m.text.clone(),
            if m.kind == MessageKind::Warning {
                theme.warning()
            } else {
                theme.text()
            },
        )
    } else {
        let bar = if add.draft.entry_type.has_payload() {
            KEY_BAR
        } else {
            KEY_BAR_NO_BODY
        };
        (bar.into(), theme.muted())
    };
    buf.set_string(r.x + 1, r.y, truncate(&text, w), style);
}
