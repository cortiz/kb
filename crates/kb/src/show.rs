//! `kb show`: an entry as text, drawn by the same code as the UI's preview pane.

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use kb_core::search::{Kb, KbError};

use crate::tui::preview::build_preview;
use crate::tui::theme::Theme;
use crate::tui::view::render_preview;

/// The narrowest and the default width of the text, in columns.
const MIN_WIDTH: u16 = 40;
pub const DEFAULT_WIDTH: u16 = 100;

/// The preview of entry `id` as text, `width` columns wide. With `color` the text carries ANSI
/// color codes and the payload is syntax highlighted; without, it is plain.
pub fn preview_text(
    kb: &Kb,
    id: &str,
    theme: &Theme,
    color: bool,
    width: u16,
) -> Result<String, KbError> {
    let entry = kb.entry(id)?;
    let path = kb.entry_path(id)?;
    let modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok());
    let uses = kb.usage(id).map_or(0, |u| u.count);
    let model = build_preview(&entry, modified, uses, color);
    // two columns of padding are part of the drawing and are cut off again below
    let buf = render_preview(&model, theme, width.max(MIN_WIDTH) + 2);
    Ok(buffer_to_text(&buf, color))
}

/// The SGR parameters that set `fg` (or `bg` when `background`), or nothing for the default.
fn color_params(c: Color, background: bool) -> Option<String> {
    let base = if background { 40 } else { 30 };
    let bright = if background { 100 } else { 90 };
    let named = |n: u8| Some((base + n).to_string());
    let light = |n: u8| Some((bright + n).to_string());
    match c {
        Color::Reset => None,
        Color::Black => named(0),
        Color::Red => named(1),
        Color::Green => named(2),
        Color::Yellow => named(3),
        Color::Blue => named(4),
        Color::Magenta => named(5),
        Color::Cyan => named(6),
        Color::Gray => named(7),
        Color::DarkGray => light(0),
        Color::LightRed => light(1),
        Color::LightGreen => light(2),
        Color::LightYellow => light(3),
        Color::LightBlue => light(4),
        Color::LightMagenta => light(5),
        Color::LightCyan => light(6),
        Color::White => light(7),
        Color::Indexed(n) => Some(format!("{};5;{n}", base + 8)),
        Color::Rgb(r, g, b) => Some(format!("{};2;{r};{g};{b}", base + 8)),
    }
}

/// The escape sequence that switches to `style` from a reset terminal.
fn sgr(style: Style) -> String {
    let mut params: Vec<String> = Vec::new();
    for (flag, code) in [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::ITALIC, "3"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::SLOW_BLINK, "5"),
        (Modifier::REVERSED, "7"),
        (Modifier::HIDDEN, "8"),
        (Modifier::CROSSED_OUT, "9"),
    ] {
        if style.add_modifier.contains(flag) && !style.sub_modifier.contains(flag) {
            params.push(code.to_string());
        }
    }
    params.extend(style.fg.and_then(|c| color_params(c, false)));
    params.extend(style.bg.and_then(|c| color_params(c, true)));
    if params.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", params.join(";"))
    }
}

/// `style` without the "reset" colors and cancelled modifiers a buffer cell carries, so a cell
/// with no styling compares equal to `Style::default()`.
fn effective(style: Style) -> Style {
    let mut s = Style::default();
    if let Some(c) = style.fg.filter(|c| *c != Color::Reset) {
        s = s.fg(c);
    }
    if let Some(c) = style.bg.filter(|c| *c != Color::Reset) {
        s = s.bg(c);
    }
    s.add_modifier = style.add_modifier - style.sub_modifier;
    s
}

/// The buffer as lines of text. The first and last column (padding) are dropped, trailing blanks
/// are trimmed, and with `color` every change of style is an ANSI sequence.
pub fn buffer_to_text(buf: &Buffer, color: bool) -> String {
    let area = buf.area;
    let mut out = String::new();
    for y in 0..area.height {
        // (text, style) per cell, skipping the cells a wide character covers
        let mut cells: Vec<(String, Style)> = Vec::new();
        let mut x = 1;
        while x < area.width.saturating_sub(1) {
            let cell = &buf[(area.x + x, area.y + y)];
            let symbol = cell.symbol().to_string();
            let w = UnicodeWidthStr::width(symbol.as_str()).max(1);
            cells.push((symbol, effective(cell.style())));
            x += u16::try_from(w).unwrap_or(1);
        }
        while cells.last().is_some_and(|(s, st)| {
            s.trim().is_empty()
                && st.bg.is_none_or(|b| b == Color::Reset)
                && !st.add_modifier.contains(Modifier::UNDERLINED)
                && !st.add_modifier.contains(Modifier::REVERSED)
        }) {
            cells.pop();
        }
        let mut current = Style::default();
        for (symbol, style) in cells {
            if color && style != current {
                if current != Style::default() {
                    out.push_str("\x1b[0m");
                }
                out.push_str(&sgr(style));
                current = style;
            }
            out.push_str(&symbol);
        }
        if color && current != Style::default() {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn buf(width: u16, rows: &[&[(&str, Style)]]) -> Buffer {
        let mut b = Buffer::empty(Rect::new(0, 0, width, rows.len() as u16));
        for (y, row) in rows.iter().enumerate() {
            let mut x = 1; // column 0 is the padding
            for (text, style) in *row {
                b.set_string(x, y as u16, text, *style);
                x += UnicodeWidthStr::width(*text) as u16;
            }
        }
        b
    }

    #[test]
    fn plain_text_drops_padding_and_trailing_blanks() {
        let b = buf(
            20,
            &[
                &[("hello", Style::default())],
                &[],
                &[("a b", Style::default())],
            ],
        );
        assert_eq!(buffer_to_text(&b, false), "hello\n\na b\n");
        let styled = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
        let b = buf(20, &[&[("hi", styled)]]);
        assert_eq!(
            buffer_to_text(&b, false),
            "hi\n",
            "no color: styles are dropped"
        );
    }

    #[test]
    fn color_text_switches_style_and_resets_at_the_end_of_the_line() {
        let red = Style::default().fg(Color::Red);
        let b = buf(
            20,
            &[&[("a", Style::default()), ("b", red), ("c", Style::default())]],
        );
        assert_eq!(buffer_to_text(&b, true), "a\x1b[31mb\x1b[0mc\n");
        let b = buf(20, &[&[("x", red)]]);
        assert_eq!(buffer_to_text(&b, true), "\x1b[31mx\x1b[0m\n");
    }

    #[test]
    fn indexed_rgb_background_and_modifiers() {
        assert_eq!(
            sgr(Style::default().fg(Color::Indexed(208))),
            "\x1b[38;5;208m"
        );
        assert_eq!(
            sgr(Style::default().fg(Color::Rgb(1, 2, 3))),
            "\x1b[38;2;1;2;3m"
        );
        assert_eq!(sgr(Style::default().bg(Color::Indexed(7))), "\x1b[48;5;7m");
        assert_eq!(
            sgr(Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD | Modifier::ITALIC)),
            "\x1b[1;3;32m"
        );
        assert_eq!(sgr(Style::default().fg(Color::DarkGray)), "\x1b[90m");
        assert_eq!(sgr(Style::default()), "");
        assert_eq!(sgr(Style::default().fg(Color::Reset)), "");
    }

    #[test]
    fn wide_characters_take_two_columns_and_are_not_doubled() {
        let b = buf(20, &[&[("日本x", Style::default())]]);
        assert_eq!(buffer_to_text(&b, false), "日本x\n");
    }

    #[test]
    fn a_coloured_background_keeps_its_trailing_cell() {
        let badge = Style::default().bg(Color::Blue);
        let b = buf(20, &[&[("sql ", badge)]]);
        let text = buffer_to_text(&b, true);
        assert!(text.contains("sql "), "{text:?}");
    }
}
