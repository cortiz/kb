//! Drawing. `draw` is a pure function of the state, the theme and the area, so a screen can be
//! rendered into a buffer and compared with a stored snapshot.

use std::ops::Range;

use kb_core::entry::EntryType;
use kb_core::query::Field;
use kb_core::search::SearchResult;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};

use super::app::{AppState, LayoutMode, MessageKind, Overlay};
use super::highlight::Span as TokSpan;
use super::keys::{self, BarKind, Mode};
use super::layout::{self, Layout, Screen, pad, truncate, truncate_middle, width};
use super::markdown;
use super::preview::{PreviewModel, VarRow};
use super::theme::Class;
use super::theme::Theme;

pub fn draw(state: &AppState, theme: &Theme, area: Rect, buf: &mut Buffer) {
    buf.set_style(area, Style::default());
    let full = state.layout_mode == LayoutMode::FullPreview;
    let screen = layout::layout(area, full);
    if let (Screen::Ok(_), Some(add)) = (&screen, &state.add) {
        super::add_view::draw_add(state, add, theme, area, buf);
        return;
    }
    if let (Screen::Ok(_), Some(form)) = (&screen, &state.form) {
        super::form_view::draw_form(state, form, theme, area, buf);
        return;
    }
    let Screen::Ok(l) = screen else {
        let msg = "Terminal too small";
        let x = area.x + area.width.saturating_sub(width(msg) as u16) / 2;
        let y = area.y + area.height / 2;
        buf.set_string(x, y, truncate(msg, usize::from(area.width)), theme.text());
        return;
    };
    draw_header(state, theme, l.header, buf);
    draw_search(state, theme, l.search, buf);
    draw_chips(state, theme, l.chips, buf);
    if let Some(list) = l.list {
        draw_list(state, theme, list, buf);
    }
    if let Some(preview) = l.preview {
        draw_preview(state, theme, preview, buf);
    }
    draw_keybar(state, theme, &l, buf);
    if state.overlay == Overlay::Help {
        draw_help(theme, area, buf);
    }
}

fn line(buf: &mut Buffer, x: u16, y: u16, spans: Vec<Span<'_>>, max: u16) {
    buf.set_line(x, y, &Line::from(spans), max);
}

fn draw_header(state: &AppState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let w = usize::from(r.width);
    let mut spans = vec![Span::styled(
        " kb",
        theme.text().add_modifier(Modifier::BOLD),
    )];
    let known = state.loaded || state.status.last_refresh.is_some();
    let mut rest = format!(" · {}", state.root);
    let mut tail = Vec::new();
    if known {
        tail.push(Span::styled(
            format!(" · {} entries", state.status.indexed),
            theme.muted(),
        ));
    }
    if state.status.skipped > 0 {
        tail.push(Span::styled(
            format!(" · ⚠ {} with errors", state.status.skipped),
            theme.warning(),
        ));
    }
    if state.options.config_problems > 0 {
        let n = state.options.config_problems;
        tail.push(Span::styled(
            format!(
                " · ⚠ config: {n} problem{} (run kb doctor)",
                if n == 1 { "" } else { "s" }
            ),
            theme.warning(),
        ));
    }
    if state.status.refreshing {
        tail.push(Span::styled(" · refreshing…", theme.muted()));
    }
    let tail_w: usize = tail.iter().map(|s| width(&s.content)).sum();
    let room = w.saturating_sub(3 + tail_w);
    rest = truncate_middle(&rest, room);
    spans.push(Span::styled(rest, theme.muted()));
    spans.extend(tail);
    line(buf, r.x, r.y, spans, r.width);
}

fn draw_search(state: &AppState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let counter = format!("{} / {} ", state.total, state.status.indexed);
    let counter_w = width(&counter) as u16;
    let prompt = " › ";
    buf.set_string(r.x, r.y, prompt, theme.warning());
    let text = state.input.editor.text();
    let avail = usize::from(r.width.saturating_sub(counter_w + 4));
    let cursor = state.input.editor.cursor();
    // show the part of the text around the cursor when it is longer than the line
    let chars: Vec<char> = text.chars().collect();
    let mut start = 0;
    while start < cursor
        && chars[start..cursor]
            .iter()
            .map(|c| width(&c.to_string()))
            .sum::<usize>()
            >= avail
    {
        start += 1;
    }
    let mut x = r.x + width(prompt) as u16;
    let mut used = 0usize;
    for (i, c) in chars.iter().enumerate().skip(start) {
        let s = c.to_string();
        let w = width(&s);
        if used + w > avail {
            break;
        }
        let style = if i == cursor {
            theme.cursor()
        } else {
            theme.text()
        };
        buf.set_string(x, r.y, &s, style);
        x += w as u16;
        used += w;
    }
    if cursor >= chars.len() && used < avail + 1 {
        buf.set_string(x, r.y, " ", theme.cursor());
    }
    let cx = r.x + r.width.saturating_sub(counter_w);
    buf.set_string(cx, r.y, &counter, theme.muted());
}

/// One chip: its text and style.
type ChipItem = (String, Style);

/// Whether a type chip stays at a shrink level: `(active, count)` in, keep out.
type Keep<'a> = &'a dyn Fn(bool, usize) -> bool;

fn chips_width(items: &[ChipItem]) -> usize {
    1 + items.iter().map(|(t, _)| width(t) + 1).sum::<usize>()
}

/// The chips row for `cols` columns. Active filters always win: when everything does not fit,
/// empty type chips go first, then the counts, then the inactive types, and only then filters
/// from the right.
fn chip_items(state: &AppState, theme: &Theme, cols: usize) -> Vec<ChipItem> {
    let active_types = state.input.type_chips();
    let all: usize = state.type_counts.iter().sum();
    let all_style = if active_types.is_empty() {
        theme.chip_active()
    } else {
        theme.text()
    };
    let filters: Vec<ChipItem> = state
        .input
        .chips
        .iter()
        .filter(|c| !matches!(c, super::input::Chip::Type(_)))
        .map(|c| (format!(" {} × ", c.syntax()), theme.chip_active()))
        .collect();
    let build = |keep: Keep, counts: bool| -> Vec<ChipItem> {
        let mut items = vec![(format!(" all {all} "), all_style)];
        for (i, ty) in EntryType::ALL.iter().enumerate() {
            let n = state.type_counts[i];
            let active = active_types.contains(ty);
            if !keep(active, n) {
                continue;
            }
            let style = if active {
                theme.chip_active()
            } else if n == 0 {
                theme.chip_dim()
            } else {
                theme.text()
            };
            let text = if counts {
                format!(" {} {n} ", ty.name())
            } else {
                format!(" {} ", ty.name())
            };
            items.push((text, style));
        }
        items.extend(filters.iter().cloned());
        items
    };
    let levels: [(Keep, bool); 4] = [
        (&|_, _| true, true),
        (&|a, n| a || n > 0, true),
        (&|a, n| a || n > 0, false),
        (&|a, _| a, false),
    ];
    let mut items = Vec::new();
    for (keep, counts) in levels {
        items = build(keep, counts);
        if chips_width(&items) <= cols {
            return items;
        }
    }
    while items.len() > 1 && chips_width(&items) > cols {
        items.pop();
    }
    items
}

fn draw_chips(state: &AppState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let mut x = r.x + 1;
    for (text, style) in chip_items(state, theme, usize::from(r.width)) {
        buf.set_string(x, r.y, &text, style);
        x += width(&text) as u16 + 1;
    }
}

/// `text` truncated to `max` columns, with `ranges` (character indexes) in the match style.
fn highlighted(
    text: &str,
    ranges: &[Range<usize>],
    base: Style,
    hl: Style,
    max: usize,
) -> Vec<Span<'static>> {
    let t = truncate(text, max);
    let original = text.chars().count();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_hl = false;
    for (i, c) in t.chars().enumerate() {
        let is_hl = i < original && ranges.iter().any(|r| r.contains(&i));
        if is_hl != run_hl && !run.is_empty() {
            spans.push(Span::styled(
                std::mem::take(&mut run),
                if run_hl { hl } else { base },
            ));
        }
        run_hl = is_hl;
        run.push(c);
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, if run_hl { hl } else { base }));
    }
    spans
}

fn ranges_of(r: &SearchResult, field: Field) -> Vec<Range<usize>> {
    r.matches
        .iter()
        .find(|m| m.field == field)
        .map(|m| m.ranges.clone())
        .unwrap_or_default()
}

/// Draws a bordered box and returns its inside.
fn boxed(title: &str, theme: &Theme, r: Rect, buf: &mut Buffer) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border())
        .title(Span::styled(format!(" {title} "), theme.muted()));
    let inner = block.inner(r);
    block.render(r, buf);
    inner
}

fn wrap_words(text: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if !cur.is_empty() && width(&cur) + 1 + width(word) > cols {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn draw_list(state: &AppState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let inner = boxed("results", theme, r, buf);
    if inner.width < 4 || inner.height == 0 {
        return;
    }
    let cols = usize::from(inner.width);
    if state.empty_kb {
        let msg = format!(
            "No entries yet. Run `kb init` or add a file to {}.",
            state.root
        );
        for (i, l) in wrap_words(&msg, cols - 2)
            .iter()
            .take(usize::from(inner.height))
            .enumerate()
        {
            buf.set_string(inner.x + 1, inner.y + i as u16, l, theme.muted());
        }
        return;
    }
    if state.results.is_empty() {
        if state.loaded {
            buf.set_string(inner.x + 1, inner.y, "No matches", theme.muted());
        }
        return;
    }
    let rows = usize::from(inner.height) / 2;
    for (n, result) in state
        .results
        .iter()
        .enumerate()
        .skip(state.list_offset)
        .take(rows)
    {
        let y = inner.y + ((n - state.list_offset) * 2) as u16;
        let selected = n == state.selected;
        if selected {
            buf.set_style(Rect::new(inner.x, y, inner.width, 2), theme.selection());
        }
        let badge = pad(result.entry_type.name(), 6);
        let title_cols = cols.saturating_sub(8);
        let mut spans = vec![
            Span::raw(" "),
            Span::styled(badge, theme.badge(result.entry_type)),
            Span::raw(" "),
        ];
        spans.extend(highlighted(
            &result.title,
            &ranges_of(result, Field::Title),
            theme.text(),
            theme.matched(),
            title_cols,
        ));
        line(buf, inner.x, y, spans, inner.width);
        // second line: tags and use count
        let count = if result.use_count > 0 {
            format!("×{}", result.use_count)
        } else {
            String::new()
        };
        let count_w = if count.is_empty() {
            0
        } else {
            width(&count) + 1
        };
        let tag_cols = cols.saturating_sub(8 + count_w);
        let mut spans = vec![Span::raw("        ")];
        spans.extend(tags_spans(result, theme, tag_cols));
        line(buf, inner.x, y + 1, spans, inner.width);
        if !count.is_empty() {
            let x = inner.x + inner.width - count_w as u16;
            buf.set_string(x, y + 1, &count, theme.muted());
        }
    }
}

/// The `#tag` list with match highlights mapped from the joined-tags ranges.
fn tags_spans(result: &SearchResult, theme: &Theme, max: usize) -> Vec<Span<'static>> {
    let ranges = ranges_of(result, Field::Tags);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut offset = 0;
    let mut used = 0usize;
    for tag in &result.tags {
        let len = tag.chars().count();
        let local: Vec<Range<usize>> = ranges
            .iter()
            .filter(|x| x.start >= offset && x.end <= offset + len)
            .map(|x| x.start - offset + 1..x.end - offset + 1)
            .collect();
        let shown = format!("#{tag}");
        let sep = usize::from(!spans.is_empty());
        if used + sep + width(&shown) > max {
            if max > used + sep {
                spans.push(Span::styled("…", theme.muted()));
            }
            break;
        }
        if sep == 1 {
            spans.push(Span::raw(" "));
        }
        spans.extend(highlighted(
            &shown,
            &local,
            theme.muted(),
            theme.matched(),
            max,
        ));
        used += sep + width(&shown);
        offset += len + 1;
    }
    spans
}

/// A line of the preview, by what it shows. Building these is cheap; only the visible ones are
/// turned into styled text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Title,
    Url(usize),
    Meta,
    Path,
    Code(usize),
    Blank,
    Rule(&'static str),
    Payload(usize),
    Var(usize),
    Field(usize),
    Notes(usize),
}

struct PreviewLayout {
    rows: Vec<Row>,
    urls: Vec<String>,
    code: Vec<(&'static str, String)>,
    notes: Vec<Vec<TokSpan>>,
}

/// Splits `s` into pieces of at most `cols` columns (URLs have no spaces to wrap at).
fn chunk(s: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = width(&c.to_string());
        if used + w > cols && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            used = 0;
        }
        cur.push(c);
        used += w;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn preview_layout(m: &PreviewModel, cols: usize) -> PreviewLayout {
    let mut rows = vec![Row::Title];
    let mut urls = Vec::new();
    if let Some(u) = &m.url {
        urls = chunk(u, cols);
        rows.extend((0..urls.len()).map(Row::Url));
    }
    rows.push(Row::Meta);
    rows.push(Row::Path);
    let mut code = Vec::new();
    if let Some(c) = &m.code {
        code.push(("repo", c.repo.clone()));
        code.push(("path", c.path.clone()));
        code.push((
            "line",
            match c.end_line {
                Some(e) => format!("{}–{}", c.line, e),
                None => c.line.to_string(),
            },
        ));
        code.push(("ref", c.git_ref.clone()));
        if let Some(s) = &c.symbol {
            code.push(("symbol", s.clone()));
        }
        rows.push(Row::Blank);
        rows.extend((0..code.len()).map(Row::Code));
    }
    if !m.payload.is_empty() {
        rows.push(Row::Blank);
        rows.push(Row::Rule("payload"));
        rows.extend((0..m.payload.len()).map(Row::Payload));
    }
    if !m.vars.is_empty() {
        rows.push(Row::Blank);
        rows.push(Row::Rule("variables"));
        rows.extend((0..m.vars.len()).map(Row::Var));
    }
    if !m.fields.is_empty() {
        rows.push(Row::Blank);
        rows.push(Row::Rule("fields"));
        rows.extend((0..m.fields.len()).map(Row::Field));
    }
    let notes = markdown::wrap(&m.notes, cols);
    if !notes.is_empty() {
        rows.push(Row::Blank);
        rows.push(Row::Rule("notes"));
        rows.extend((0..notes.len()).map(Row::Notes));
    }
    PreviewLayout {
        rows,
        urls,
        code,
        notes,
    }
}

/// The inside of the preview box for the current size and layout mode.
fn preview_inner(state: &AppState) -> Option<Rect> {
    let area = Rect::new(0, 0, state.size.0, state.size.1);
    let Screen::Ok(l) = layout::layout(area, state.layout_mode == LayoutMode::FullPreview) else {
        return None;
    };
    let r = l.preview?;
    (r.width > 2 && r.height > 2).then(|| Rect::new(r.x + 1, r.y + 1, r.width - 2, r.height - 2))
}

/// `(rows in the preview, rows that fit)` for the selected entry, or `None` without a preview.
pub fn preview_metrics(state: &AppState) -> Option<(usize, usize)> {
    let inner = preview_inner(state)?;
    let model = state.preview.as_ref()?;
    let cols = usize::from(inner.width).saturating_sub(2).max(4);
    Some((
        preview_layout(model, cols).rows.len(),
        usize::from(inner.height),
    ))
}

fn tok(spans: &[TokSpan], theme: &Theme) -> Vec<Span<'static>> {
    spans
        .iter()
        .map(|s| Span::styled(s.text.clone(), theme.class_style(s.class)))
        .collect()
}

/// Cuts styled spans to `cols` columns, ending with `›` when something was cut.
fn cut(spans: Vec<Span<'static>>, cols: usize, theme: &Theme) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|s| width(&s.content)).sum();
    if total <= cols {
        return spans;
    }
    let budget = cols.saturating_sub(1);
    let mut out = Vec::new();
    let mut used = 0;
    'outer: for s in spans {
        let mut text = String::new();
        for c in s.content.chars() {
            let w = width(&c.to_string());
            if used + w > budget {
                if !text.is_empty() {
                    out.push(Span::styled(text, s.style));
                }
                break 'outer;
            }
            text.push(c);
            used += w;
        }
        out.push(Span::styled(text, s.style));
    }
    out.push(Span::styled("›", theme.muted()));
    out
}

fn draw_preview(state: &AppState, theme: &Theme, r: Rect, buf: &mut Buffer) {
    let inner = boxed("preview", theme, r, buf);
    if inner.width < 4 || inner.height == 0 {
        return;
    }
    let cols = usize::from(inner.width) - 2;
    if state.results.is_empty() && state.loaded && !state.empty_kb {
        let tips = [
            "word word          all words must match",
            "\"exact phrase\"",
            "type:sql  tag:prod  -tag:old",
            "var:region  repo:owner/name  in:title",
        ];
        buf.set_string(
            inner.x + 1,
            inner.y,
            "Search tips",
            theme.text().add_modifier(Modifier::BOLD),
        );
        for (i, t) in tips.iter().enumerate() {
            buf.set_string(
                inner.x + 1,
                inner.y + 2 + i as u16,
                truncate(t, cols),
                theme.muted(),
            );
        }
        return;
    }
    let Some(m) = &state.preview else {
        if !state.results.is_empty() {
            buf.set_string(inner.x + 1, inner.y, "…", theme.muted());
        }
        return;
    };
    let lay = preview_layout(m, cols);
    let height = usize::from(inner.height);
    let total = lay.rows.len();
    let scroll = state.preview_scroll.min(total.saturating_sub(height));
    let stale = state.selected_id.as_deref().is_some_and(|id| id != m.id);
    draw_preview_rows(m, &lay, stale, theme, inner, scroll, buf);
    if scroll > 0 {
        let t = "↑ more";
        buf.set_string(
            inner.x + inner.width - width(t) as u16 - 1,
            inner.y,
            t,
            theme.muted(),
        );
    }
    if scroll + height < total {
        let t = "↓ more";
        buf.set_string(
            inner.x + inner.width - width(t) as u16 - 1,
            inner.y + inner.height - 1,
            t,
            theme.muted(),
        );
    }
}

/// Draws rows `scroll..` of the preview into `inner` (one column of padding on each side).
fn draw_preview_rows(
    m: &PreviewModel,
    lay: &PreviewLayout,
    stale: bool,
    theme: &Theme,
    inner: Rect,
    scroll: usize,
    buf: &mut Buffer,
) {
    let cols = usize::from(inner.width).saturating_sub(2);
    let height = usize::from(inner.height);
    let gutter = m.payload.len().to_string().len();
    for (n, row) in lay.rows.iter().enumerate().skip(scroll).take(height) {
        let y = inner.y + (n - scroll) as u16;
        let x = inner.x + 1;
        let spans: Vec<Span<'static>> = match *row {
            Row::Blank => continue,
            Row::Title => {
                let t = if stale {
                    format!("{} …", m.title)
                } else {
                    m.title.clone()
                };
                vec![Span::styled(
                    truncate(&t, cols),
                    theme.text().add_modifier(Modifier::BOLD),
                )]
            }
            Row::Url(i) => vec![Span::styled(
                lay.urls[i].clone(),
                theme.class_style(Class::Link),
            )],
            Row::Meta => meta_spans(m, theme, cols),
            Row::Path => vec![Span::styled(truncate(&m.path, cols), theme.muted())],
            Row::Code(i) => {
                let (label, value) = &lay.code[i];
                vec![
                    Span::styled(format!("{label:<7}"), theme.muted()),
                    Span::styled(truncate(value, cols.saturating_sub(7)), theme.text()),
                ]
            }
            Row::Rule(name) => {
                let head = format!("── {name} ");
                let fill = "─".repeat(cols.saturating_sub(width(&head)));
                vec![Span::styled(format!("{head}{fill}"), theme.border())]
            }
            Row::Payload(i) => {
                let mut spans = vec![Span::styled(format!("{:>gutter$} ", i + 1), theme.muted())];
                spans.extend(cut(
                    tok(&m.payload[i], theme),
                    cols.saturating_sub(gutter + 1),
                    theme,
                ));
                spans
            }
            Row::Var(i) => var_spans(&m.vars[i], theme, cols),
            Row::Field(i) => {
                let (k, v) = &m.fields[i];
                vec![
                    Span::styled(format!("{k:<11}"), theme.muted()),
                    Span::styled(truncate(v, cols.saturating_sub(11)), theme.text()),
                ]
            }
            Row::Notes(i) => tok(&lay.notes[i], theme),
        };
        buf.set_line(x, y, &Line::from(spans), inner.width.saturating_sub(1));
    }
}

/// The whole preview of one entry as a buffer `width` columns wide and exactly tall enough, for
/// `kb show`. It is the same drawing code as the UI's preview pane.
pub fn render_preview(m: &PreviewModel, theme: &Theme, width: u16) -> Buffer {
    let cols = usize::from(width).saturating_sub(2).max(4);
    let lay = preview_layout(m, cols);
    let height = u16::try_from(lay.rows.len()).unwrap_or(u16::MAX);
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    draw_preview_rows(m, &lay, false, theme, area, 0, &mut buf);
    buf
}

fn meta_spans(m: &PreviewModel, theme: &Theme, cols: usize) -> Vec<Span<'static>> {
    let Some(meta) = &m.meta else {
        return Vec::new();
    };
    let mut spans = vec![Span::styled(
        meta.entry_type.name().to_string(),
        theme.badge(meta.entry_type),
    )];
    if !meta.tags.is_empty() {
        let tags: Vec<String> = meta.tags.iter().map(|t| format!("#{t}")).collect();
        spans.push(Span::styled(
            format!(" · {}", tags.join(" ")),
            theme.muted(),
        ));
    }
    if meta.use_count > 0 {
        let times = if meta.use_count == 1 { "time" } else { "times" };
        spans.push(Span::styled(
            format!(" · used {} {times}", meta.use_count),
            theme.muted(),
        ));
    }
    if let Some(e) = &meta.edited {
        spans.push(Span::styled(format!(" · edited {e}"), theme.muted()));
    }
    let _ = cols;
    spans
}

fn var_spans(v: &VarRow, theme: &Theme, cols: usize) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        format!("{:<12}", v.name),
        theme.text().add_modifier(Modifier::BOLD),
    )];
    if v.secret {
        spans.push(Span::styled("secret (prompted)".to_string(), theme.muted()));
    } else {
        spans.push(Span::styled(format!("{:<7}", v.kind), theme.muted()));
        if !v.options.is_empty() {
            spans.push(Span::styled(
                format!(" {}", v.options.join("|")),
                theme.text(),
            ));
        }
        if let Some(d) = &v.default {
            spans.push(Span::styled(format!(" = {d}"), theme.text()));
        }
    }
    if let Some(d) = &v.description {
        spans.push(Span::styled(format!(" — {d}"), theme.muted()));
    }
    let used: usize = spans.iter().map(|s| width(&s.content)).sum();
    if used > cols {
        return cut(spans, cols, theme);
    }
    spans
}

fn preview_scrolls(state: &AppState) -> bool {
    preview_metrics(state).is_some_and(|(rows, height)| rows > height)
}

/// The key bar for the current state, fitted to `width` columns.
pub fn key_bar_text(state: &AppState, width: usize) -> String {
    keys::key_bar(bar_kind(state), width)
}

fn bar_kind(state: &AppState) -> BarKind {
    let blank = state.input.is_blank();
    match state.mode() {
        Mode::TooSmall => BarKind::TooSmall,
        Mode::Form => BarKind::TooSmall,
        Mode::Help => BarKind::Help,
        Mode::FullPreview => BarKind::FullPreview,
        Mode::Search => {
            if state.empty_kb {
                BarKind::EmptyKb
            } else if state.results.is_empty() {
                BarKind::NoResults { text_empty: blank }
            } else {
                BarKind::Results {
                    text_empty: blank,
                    preview_scrolls: preview_scrolls(state),
                }
            }
        }
    }
}

/// The help overlay: every binding, from the same table that drives the keys. When the full
/// list does not fit the height it is drawn as two columns of short lines.
fn draw_help(theme: &Theme, area: Rect, buf: &mut Buffer) {
    let long = keys::help_lines();
    let fits = long.len() + 2 <= usize::from(area.height).saturating_sub(2);
    let groups: Vec<&str> = keys::Group::ALL.iter().map(|g| g.title()).collect();
    let max_w = usize::from(area.width).saturating_sub(4);
    if fits {
        let want_w = long.iter().map(|l| width(l)).max().unwrap_or(40) + 4;
        let w = want_w.min(max_w).max(20) as u16;
        let h = (long.len() as u16 + 2).max(5);
        let r = help_box(area, w, h);
        clear(buf, r);
        let inner = boxed("keys", theme, r, buf);
        for (i, l) in long.iter().take(usize::from(inner.height)).enumerate() {
            let style = if groups.contains(&l.as_str()) {
                theme.text().add_modifier(Modifier::BOLD)
            } else {
                theme.text()
            };
            buf.set_string(
                inner.x + 1,
                inner.y + i as u16,
                truncate(l, usize::from(inner.width) - 2),
                style,
            );
        }
        return;
    }
    let compact = keys::help_compact();
    let note = keys::help_note();
    let rows_per_col = compact.len().div_ceil(2);
    let h = ((rows_per_col + note.len() + 3) as u16)
        .min(area.height.saturating_sub(2))
        .max(5);
    let w = (max_w as u16).max(20);
    let r = help_box(area, w, h);
    clear(buf, r);
    let inner = boxed("keys", theme, r, buf);
    let col_w = usize::from(inner.width).saturating_sub(2) / 2;
    let body_rows = usize::from(inner.height).saturating_sub(note.len() + 1);
    for (i, l) in compact.iter().enumerate() {
        let (col, row) = (i / rows_per_col, i % rows_per_col);
        if row >= body_rows {
            continue;
        }
        buf.set_string(
            inner.x + 1 + (col * col_w) as u16,
            inner.y + row as u16,
            truncate(l, col_w.saturating_sub(1)),
            theme.text(),
        );
    }
    for (i, l) in note.iter().enumerate() {
        let y = inner.y + inner.height.saturating_sub((note.len() - i) as u16);
        buf.set_string(
            inner.x + 1,
            y,
            truncate(l, usize::from(inner.width) - 2),
            theme.muted(),
        );
    }
}

/// The centered rectangle for the overlay.
fn help_box(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    )
}

/// Blanks `r` so the overlay does not show what is behind it.
fn clear(buf: &mut Buffer, r: Rect) {
    for y in r.y..r.y + r.height {
        buf.set_string(r.x, y, " ".repeat(usize::from(r.width)), Style::default());
    }
}

fn draw_keybar(state: &AppState, theme: &Theme, l: &Layout, buf: &mut Buffer) {
    let r = l.keybar;
    let w = usize::from(r.width).saturating_sub(2);
    if let Some(p) = &state.prompt {
        let mut text = p.text();
        if let Some(m) = &state.message {
            text = format!("{text}   {}", m.text);
        }
        buf.set_string(r.x + 1, r.y, truncate(&text, w), theme.warning());
        return;
    }
    let (text, style) = match &state.message {
        Some(m) => (
            truncate(&m.text, w),
            if m.kind == MessageKind::Warning {
                theme.warning()
            } else {
                theme.text()
            },
        ),
        None => (key_bar_text(state, w), theme.muted()),
    };
    buf.set_string(r.x + 1, r.y, text, style);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tui::app::tests::{page, state};

    pub(crate) fn render(state: &AppState, theme: &Theme, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        draw(state, theme, area, &mut buf);
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn the_header_warns_about_config_problems() {
        let mut s = state();
        s.loaded = true;
        s.status.refreshing = false;
        s.status.indexed = 284;
        let quiet = render(&s, &Theme::dark(), 120, 40);
        assert!(!quiet[0].contains("config"), "{}", quiet[0]);
        s.options.config_problems = 2;
        let lines = render(&s, &Theme::dark(), 120, 40);
        assert!(
            lines[0].contains("284 entries")
                && lines[0].contains("⚠ config: 2 problems (run kb doctor)"),
            "{}",
            lines[0]
        );
        s.options.config_problems = 1;
        let lines = render(&s, &Theme::dark(), 120, 40);
        assert!(
            lines[0].contains("⚠ config: 1 problem (run kb doctor)"),
            "{}",
            lines[0]
        );
        // narrow: the root path gives way, the warning and the count stay
        let lines = render(&s, &Theme::dark(), 64, 24);
        assert!(
            lines[0].contains("284 entries") && lines[0].contains("⚠ config: 1 problem"),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn the_empty_state_and_the_loaded_state_draw_the_documented_regions() {
        let mut s = state();
        let lines = render(&s, &Theme::dark(), 120, 40);
        assert!(
            lines[0].contains("kb")
                && lines[0].contains("~/kb")
                && lines[0].contains("refreshing…")
        );
        assert!(lines[1].starts_with(" › "));
        assert!(lines[2].contains("all 0") && lines[2].contains("sql 0"));
        assert!(lines[3].contains("results") && lines[3].contains("preview"));
        s.loaded = true;
        s.status.refreshing = false;
        s.status.indexed = 284;
        s.status.skipped = 3;
        let mut p = page(&["sumo/a", "sumo/b"]);
        p.results[0].title = "5xx by authorizer".into();
        p.results[0].tags = vec!["auth".into(), "lambda".into()];
        p.results[0].use_count = 7;
        p.total = 2;
        p.indexed = 284;
        p.skipped = 3;
        s.search_gen = 1;
        s.update(
            crate::tui::app::Action::Results(1, p),
            std::time::Instant::now(),
        );
        let lines = render(&s, &Theme::dark(), 120, 40);
        assert!(
            lines[0].contains("284 entries") && lines[0].contains("⚠ 3 with errors"),
            "{}",
            lines[0]
        );
        assert!(lines[1].trim_end().ends_with("2 / 284"), "{}", lines[1]);
        assert!(
            lines[4].contains("sql") && lines[4].contains("5xx by authorizer"),
            "{}",
            lines[4]
        );
        assert!(
            lines[5].contains("#auth #lambda") && lines[5].contains("×7"),
            "{}",
            lines[5]
        );
        assert!(lines[39].contains("Enter pick"), "{}", lines[39]);
    }

    #[test]
    fn empty_kb_and_no_results_have_their_messages() {
        let mut s = state();
        s.empty_kb = true;
        s.loaded = true;
        let lines = render(&s, &Theme::dark(), 120, 40);
        let list_text: String = lines
            .iter()
            .filter_map(|l| l.strip_prefix('│'))
            .map(|l| l.split('│').next().unwrap_or("").trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            list_text.contains("No entries yet. Run `kb init` or add a file to ~/kb."),
            "{list_text}"
        );
        let mut s = state();
        s.loaded = true;
        s.status.indexed = 5;
        let text = render(&s, &Theme::dark(), 120, 40).join("\n");
        assert!(
            text.contains("No matches")
                && text.contains("Search tips")
                && text.contains("type:sql"),
            "{text}"
        );
    }

    #[test]
    fn too_small_replaces_everything() {
        let s = state();
        let lines = render(&s, &Theme::dark(), 59, 20);
        assert_eq!(lines.iter().filter(|l| !l.is_empty()).count(), 1);
        assert!(lines.iter().any(|l| l.contains("Terminal too small")));
    }

    #[test]
    fn drawing_never_overflows_the_buffer_for_any_size() {
        let s = state();
        for (w, h) in [
            (60, 15),
            (61, 16),
            (99, 24),
            (100, 24),
            (101, 30),
            (300, 80),
        ] {
            let lines = render(&s, &Theme::dark(), w, h);
            assert_eq!(lines.len(), usize::from(h));
            assert!(lines.iter().all(|l| width(l) <= usize::from(w)), "{w}x{h}");
        }
    }

    #[test]
    fn active_filters_stay_visible_when_the_row_is_tight() {
        use crate::tui::app::Action;
        let mut s = AppState::new(
            crate::tui::app::Options::default(),
            (80, 24),
            "tag:prod -tag:stage latency",
            "~/kb".into(),
        );
        s.loaded = true;
        s.type_counts = [5, 4, 6, 5, 6, 3, 3];
        let g = s.search_gen;
        s.update(Action::Results(g, page(&["a"])), std::time::Instant::now());
        s.type_counts = [5, 4, 6, 5, 6, 3, 3];
        let row = render(&s, &Theme::dark(), 80, 24)[2].clone();
        assert!(
            row.contains("tag:prod ×") && row.contains("-tag:stage ×"),
            "{row}"
        );
        assert!(row.contains("all 32") && width(&row) <= 80);
        // wide enough: everything, with counts
        let wide = render(&s, &Theme::dark(), 160, 24)[2].clone();
        assert!(
            wide.contains("sql 5") && wide.contains("tag:prod ×"),
            "{wide}"
        );
        // the active type stays even when nothing else fits
        s.input.set_type(Some(EntryType::Prompt));
        let narrow = render(&s, &Theme::dark(), 60, 24)[2].clone();
        assert!(
            narrow.contains("prompt") && narrow.contains("tag:prod ×"),
            "{narrow}"
        );
        assert!(width(&narrow) <= 60);
    }
}
