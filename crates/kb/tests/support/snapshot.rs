//! Renders UI states into text snapshots (characters plus styles) and compares them with the
//! stored files in `tests/snapshots/tui/`.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kb_cli::tui::app::{Action, AppState, Options, Page, RefreshInfo};
use kb_cli::tui::preview::build_preview_with;
use kb_cli::tui::theme::{Theme, ThemeName};
use kb_cli::tui::view;
use kb_core::query::Query;
use kb_core::search::Kb;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

pub const NOW_SECS: u64 = 1_790_000_000;
/// The mtime every fixture file gets, so "edited" dates and ordering do not depend on checkout time.
pub const FIXED_MTIME: u64 = 1_788_000_000;

pub fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOW_SECS)
}

/// A copy of `tests/fixtures/<name>` with fixed modification times.
pub fn fixture_copy(name: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("kb");
    super::copy_dir(&super::fixtures().join(name), &root);
    set_mtimes(&root);
    (tmp, root)
}

fn set_mtimes(dir: &Path) {
    let t = UNIX_EPOCH + Duration::from_secs(FIXED_MTIME);
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            set_mtimes(&p);
        } else {
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
    }
}

pub fn theme_of(name: &str) -> Theme {
    match name {
        "dark" => Theme::dark(),
        "light" => Theme::light(),
        "nocolor" => Theme::no_color(),
        other => panic!("unknown theme {other}"),
    }
}

/// What the UI sends to the search for text typed as `query` (chips first, then the free text).
fn state_query(typed: &str) -> String {
    let mut input = kb_cli::tui::input::SearchInput::new("");
    for c in typed.chars() {
        input.insert(c);
    }
    input.query_string()
}

/// The state the UI is in after searching `query` in `root` and selecting `select` (if given).
pub fn state_for(
    root: &Path,
    query: &str,
    select: Option<&str>,
    size: (u16, u16),
    theme: &Theme,
) -> AppState {
    let mut kb = Kb::open(root).unwrap();
    let refresh = kb.refresh().unwrap();
    let mut q = Query::parse(&state_query(query));
    q.limit = 200;
    let outcome = kb.query(&q, now()).unwrap();
    let mut state = AppState::new(Options::default(), size, "", "~/kb".to_string());
    let t0 = Instant::now();
    // typed key by key, so filter tokens followed by a space become chips as they do for a user
    for c in query.chars() {
        state.update(
            Action::Key(ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Char(c),
                ratatui::crossterm::event::KeyModifiers::NONE,
            )),
            t0,
        );
    }
    state.update(
        Action::Refreshed(RefreshInfo {
            indexed: refresh.indexed,
            skipped: refresh.skipped,
            duration: Duration::from_millis(5),
        }),
        t0,
    );
    let generation = state.search_gen;
    let page = Page {
        results: outcome.results,
        total: outcome.total,
        type_counts: outcome.type_counts,
        indexed: outcome.indexed,
        skipped: outcome.skipped,
        notices: outcome.notices,
    };
    state.update(Action::Results(generation, page), t0);
    if let Some(id) = select {
        let at = state
            .results
            .iter()
            .position(|r| r.id == id)
            .unwrap_or_else(|| panic!("{id} is not in the results"));
        for _ in 0..at {
            state.update(
                Action::Key(ratatui::crossterm::event::KeyEvent::new(
                    ratatui::crossterm::event::KeyCode::Down,
                    ratatui::crossterm::event::KeyModifiers::NONE,
                )),
                t0,
            );
        }
    }
    if let Some(r) = state.selected_result() {
        let (id, modified, uses) = (r.id.clone(), r.modified, r.use_count);
        let entry = kb.entry(&id).unwrap();
        let lookup = |target: &str| kb_cli::show::link_lookup(&kb, target);
        let model = build_preview_with(&entry, modified, uses, theme.is_color(), Some(&lookup));
        state.update(Action::PreviewReady(state.preview_gen, Box::new(model)), t0);
    }
    state.status.refreshing = false;
    state
}

fn mods(m: Modifier) -> String {
    let mut out = Vec::new();
    for (flag, name) in [
        (Modifier::BOLD, "bold"),
        (Modifier::DIM, "dim"),
        (Modifier::ITALIC, "italic"),
        (Modifier::UNDERLINED, "underline"),
        (Modifier::REVERSED, "reverse"),
    ] {
        if m.contains(flag) {
            out.push(name);
        }
    }
    out.join("+")
}

fn style_text(s: Style) -> String {
    let mut parts = Vec::new();
    if let Some(c) = s.fg.filter(|c| *c != ratatui::style::Color::Reset) {
        parts.push(format!("fg={c}"));
    }
    if let Some(c) = s.bg.filter(|c| *c != ratatui::style::Color::Reset) {
        parts.push(format!("bg={c}"));
    }
    let m = mods(s.add_modifier);
    if !m.is_empty() {
        parts.push(m);
    }
    parts.join(" ")
}

/// The screen as text lines, then the style runs.
pub fn render(state: &AppState, theme: &Theme) -> String {
    let (w, h) = state.size;
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    view::draw(state, theme, area, &mut buf);
    let mut out = String::new();
    for y in 0..h {
        // A wide glyph takes two cells; the second one holds no text of its own.
        let mut line = String::new();
        let mut x = 0;
        while x < w {
            let symbol = buf[(x, y)].symbol();
            line.push_str(symbol);
            x += u16::try_from(unicode_width::UnicodeWidthStr::width(symbol).max(1)).unwrap_or(1);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push_str("--- styles ---\n");
    for y in 0..h {
        let mut x = 0;
        while x < w {
            let st = style_text(buf[(x, y)].style());
            let mut end = x + 1;
            while end < w && style_text(buf[(end, y)].style()) == st {
                end += 1;
            }
            if !st.is_empty() {
                let _ = writeln!(out, "{y}:{x}..{end} {st}");
            }
            x = end;
        }
    }
    out
}

pub fn snapshot_path(name: &str) -> PathBuf {
    super::fixtures()
        .join("../snapshots/tui")
        .join(format!("{name}.txt"))
}

pub fn assert_snapshot(name: &str, actual: &str) {
    let path = snapshot_path(name);
    if std::env::var_os("KB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let Ok(want) = std::fs::read_to_string(&path) else {
        panic!(
            "missing snapshot {}; run with KB_UPDATE_SNAPSHOTS=1 and review it",
            path.display()
        );
    };
    if want != actual {
        let line = want
            .lines()
            .zip(actual.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| want.lines().count().min(actual.lines().count()));
        panic!(
            "snapshot {name} differs at line {}:\n  stored: {:?}\n  actual: {:?}\n(run with KB_UPDATE_SNAPSHOTS=1 to update, then review the diff)",
            line + 1,
            want.lines().nth(line),
            actual.lines().nth(line)
        );
    }
}

pub fn theme_name(t: &Theme) -> &'static str {
    match t.name {
        ThemeName::Dark => "dark",
        ThemeName::Light => "light",
        ThemeName::NoColor => "nocolor",
    }
}
