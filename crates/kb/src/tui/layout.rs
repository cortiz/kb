//! Where things go on the screen, and fitting text into a width.

use ratatui::layout::Rect;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 15;
/// From this width on the list and the preview are shown side by side.
pub const WIDE: u16 = 100;
pub const MIN_LIST: u16 = 34;
/// Rows used by the header, search line, chips row and key bar.
pub const FIXED_ROWS: u16 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub header: Rect,
    pub search: Rect,
    pub chips: Rect,
    pub list: Option<Rect>,
    pub preview: Option<Rect>,
    pub keybar: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    TooSmall,
    Ok(Layout),
}

/// The regions for a terminal of `area`. `full_preview` gives the whole body to the preview.
pub fn layout(area: Rect, full_preview: bool) -> Screen {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return Screen::TooSmall;
    }
    let row = |y: u16| Rect::new(area.x, area.y + y, area.width, 1);
    let body_h = area.height - FIXED_ROWS;
    let body = Rect::new(area.x, area.y + 3, area.width, body_h);
    let (list, preview) = if full_preview {
        (None, Some(body))
    } else if area.width >= WIDE {
        let list_w = (area.width * 40 / 100).max(MIN_LIST);
        (
            Some(Rect::new(body.x, body.y, list_w, body_h)),
            Some(Rect::new(
                body.x + list_w,
                body.y,
                area.width - list_w,
                body_h,
            )),
        )
    } else {
        (Some(body), None)
    };
    Screen::Ok(Layout {
        header: row(0),
        search: row(1),
        chips: row(2),
        list,
        preview,
        keybar: row(area.height - 1),
    })
}

/// Display width in terminal columns.
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// `s` cut to at most `max` columns, with `…` at the end when something was cut. A wide
/// character that would straddle the edge is dropped, never split.
pub fn truncate(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let budget = max - 1;
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// `s` padded with spaces to exactly `cols` columns (after truncating when too long).
pub fn pad(s: &str, cols: usize) -> String {
    let mut t = truncate(s, cols);
    let w = width(&t);
    t.push_str(&" ".repeat(cols.saturating_sub(w)));
    t
}

/// Like `truncate` but cuts from the middle, keeping the start and the end (the last path
/// component stays readable).
pub fn truncate_middle(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max <= 1 {
        return truncate(s, max);
    }
    let budget = max - 1;
    let tail_w = budget * 2 / 3;
    let head_w = budget - tail_w;
    let mut head = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > head_w {
            break;
        }
        head.push(c);
        used += w;
    }
    let mut tail: Vec<char> = Vec::new();
    used = 0;
    for c in s.chars().rev() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > tail_w {
            break;
        }
        tail.push(c);
        used += w;
    }
    tail.reverse();
    format!("{head}…{}", tail.into_iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: u16, h: u16) -> Rect {
        Rect::new(0, 0, w, h)
    }

    fn ok(w: u16, h: u16, full: bool) -> Layout {
        match layout(area(w, h), full) {
            Screen::Ok(l) => l,
            Screen::TooSmall => panic!("{w}x{h} too small"),
        }
    }

    #[test]
    fn fixed_rows_and_body_height() {
        let l = ok(120, 40, false);
        for (r, y) in [(l.header, 0), (l.search, 1), (l.chips, 2), (l.keybar, 39)] {
            assert_eq!((r.y, r.height, r.width), (y, 1, 120));
        }
        let (list, preview) = (l.list.unwrap(), l.preview.unwrap());
        assert_eq!((list.y, list.height), (3, 36));
        assert_eq!((preview.y, preview.height), (3, 36));
    }

    #[test]
    fn wide_splits_forty_sixty_with_a_minimum_list() {
        let l = ok(120, 40, false);
        assert_eq!(l.list.unwrap().width, 48);
        assert_eq!(l.preview.unwrap().width, 72);
        assert_eq!(l.preview.unwrap().x, 48);
        let l = ok(100, 30, false);
        assert_eq!(l.list.unwrap().width, 40);
        let l = ok(200, 60, false);
        assert_eq!(l.list.unwrap().width + l.preview.unwrap().width, 200);
        assert!(l.list.unwrap().width >= MIN_LIST);
    }

    #[test]
    fn narrow_shows_the_list_only_and_full_preview_takes_the_body() {
        let l = ok(99, 30, false);
        assert!(l.preview.is_none() && l.list.unwrap().width == 99);
        let l = ok(99, 30, true);
        assert!(l.list.is_none() && l.preview.unwrap().width == 99);
        let l = ok(60, 15, false);
        assert_eq!(l.list.unwrap().height, 11);
    }

    #[test]
    fn too_small_below_sixty_by_fifteen() {
        assert_eq!(layout(area(59, 24), false), Screen::TooSmall);
        assert_eq!(layout(area(80, 14), false), Screen::TooSmall);
        assert!(matches!(layout(area(60, 15), false), Screen::Ok(_)));
    }

    #[test]
    fn truncation_uses_display_width_and_never_splits_wide_characters() {
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello world", 8), "hello w…");
        let cjk = "日本語日本語日本語"; // 18 columns
        assert_eq!(width(cjk), 18);
        let t = truncate("日本語日本語", 10);
        assert!(t.ends_with('…') && width(&t) <= 10, "{t}");
        let t = truncate("日本語日本語", 7);
        assert_eq!(t, "日本語…");
        assert_eq!(width(&t), 7);
        let t = truncate("ab🚀cd", 4);
        assert!(
            !t.contains('🚀') && t.ends_with('…') && width(&t) <= 4,
            "{t}"
        );
        assert_eq!(truncate("🚀", 2), "🚀");
        assert_eq!(truncate("anything", 0), "");
        assert_eq!(truncate("anything", 1), "…");
    }

    #[test]
    fn padding_reaches_the_exact_width() {
        assert_eq!(width(&pad("ab", 6)), 6);
        assert_eq!(width(&pad("日本", 6)), 6);
        assert_eq!(width(&pad("日本語日本語", 5)), 5);
    }

    #[test]
    fn middle_truncation_keeps_the_last_component() {
        let p = "/Users/carlos/work/knowledge/base/kb";
        let t = truncate_middle(p, 20);
        assert!(
            width(&t) <= 20 && t.contains('…') && t.ends_with("/base/kb"),
            "{t}"
        );
        assert_eq!(truncate_middle("~/kb", 20), "~/kb");
    }
}
