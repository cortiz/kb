//! The state of the UI and how it changes. `update` is a pure function of the state, the action
//! and the time; everything impure (terminal, threads, clock) lives in `tui/mod.rs`.

use std::time::{Duration, Instant};

use kb_core::config::ThemeChoice;
use kb_core::entry_json::PrintMode;
use kb_core::search::{Notice, SearchResult};
use ratatui::crossterm::event::{KeyEvent, MouseEvent};

use super::input::{Change, SearchInput};
use super::keys::{self, Command, Key, Mode};
use super::layout::{self, Screen};
use super::preview::PreviewModel;

/// Results asked for at first; doubled when the selection nears the end.
pub const PAGE: usize = 60;
const NEAR_END: usize = 10;
const MESSAGE_FOR: Duration = Duration::from_secs(2);
const REFRESH_EVERY: Duration = Duration::from_secs(1);
const SLOW_REFRESH: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
pub struct Options {
    pub print: PrintMode,
    pub mouse: bool,
    pub theme: ThemeChoice,
    /// Turned off in timing tests so the refresh timer does not add noise.
    pub refresh_timer: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            print: PrintMode::Payload,
            mouse: false,
            theme: ThemeChoice::Auto,
            refresh_timer: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Page {
    pub results: Vec<SearchResult>,
    pub total: usize,
    pub type_counts: [usize; 7],
    pub indexed: usize,
    pub skipped: usize,
    pub notices: Vec<Notice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Split,
    FullPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub kind: MessageKind,
    /// `None` keeps the message until the next key.
    pub expires: Option<Instant>,
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub refreshing: bool,
    pub indexed: usize,
    pub skipped: usize,
    pub last_refresh: Option<Instant>,
    pub interval: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshInfo {
    pub indexed: usize,
    pub skipped: usize,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyOutcome {
    Copied,
    TooLarge,
    Failed(String),
}

pub enum Action {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize(u16, u16),
    Tick,
    Results(u64, Page),
    PreviewReady(u64, Box<PreviewModel>),
    Refreshed(RefreshInfo),
    WorkerFailed(String),
    CopyDone(CopyOutcome),
    /// The editor returned; `Some` carries the first lint error of the file.
    EditorReturned(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Search {
        generation: u64,
        query: String,
        limit: usize,
    },
    LoadPreview {
        generation: u64,
        id: String,
        modified: Option<i64>,
        use_count: u64,
    },
    Refresh,
    Pick(PrintMode),
    CopyPayload,
    EditEntry(String),
    RecordUse(String),
    Quit(i32),
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub root: String,
    pub input: SearchInput,
    pub results: Vec<SearchResult>,
    pub total: usize,
    pub type_counts: [usize; 7],
    pub selected: usize,
    pub selected_id: Option<String>,
    pub list_offset: usize,
    pub preview: Option<PreviewModel>,
    pub preview_id: Option<String>,
    /// Modification time of the file the preview was requested for.
    pub preview_mtime: Option<Option<i64>>,
    pub preview_scroll: usize,
    pub layout_mode: LayoutMode,
    pub overlay: Overlay,
    pub status: Status,
    pub message: Option<Message>,
    pub search_gen: u64,
    pub preview_gen: u64,
    pub limit: usize,
    pub jump_last: bool,
    pub size: (u16, u16),
    pub empty_kb: bool,
    pub options: Options,
    /// A search has been answered at least once.
    pub loaded: bool,
}

impl AppState {
    pub fn new(options: Options, size: (u16, u16), initial_query: &str, root: String) -> AppState {
        AppState {
            root,
            input: SearchInput::from_text(initial_query),
            results: Vec::new(),
            total: 0,
            type_counts: [0; 7],
            selected: 0,
            selected_id: None,
            list_offset: 0,
            preview: None,
            preview_id: None,
            preview_mtime: None,
            preview_scroll: 0,
            layout_mode: LayoutMode::Split,
            overlay: Overlay::None,
            status: Status {
                refreshing: true,
                interval: REFRESH_EVERY,
                ..Status::default()
            },
            message: None,
            search_gen: 0,
            preview_gen: 0,
            limit: PAGE,
            jump_last: false,
            size,
            empty_kb: false,
            options,
            loaded: false,
        }
    }

    pub fn mode(&self) -> Mode {
        let area = ratatui::layout::Rect::new(0, 0, self.size.0, self.size.1);
        if layout::layout(area, false) == Screen::TooSmall {
            Mode::TooSmall
        } else if self.overlay == Overlay::Help {
            Mode::Help
        } else if self.layout_mode == LayoutMode::FullPreview {
            Mode::FullPreview
        } else {
            Mode::Search
        }
    }

    /// Whole rows (two lines each) the list can show.
    pub fn list_rows(&self) -> usize {
        let area = ratatui::layout::Rect::new(0, 0, self.size.0, self.size.1);
        match layout::layout(area, false) {
            Screen::Ok(l) => l
                .list
                .map_or(1, |r| usize::from(r.height.saturating_sub(2)) / 2)
                .max(1),
            Screen::TooSmall => 1,
        }
    }

    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected)
    }

    /// What `kb search` would be given for the current text and chips.
    pub fn query_string(&self) -> String {
        self.input.query_string()
    }

    /// The first message to send when the UI starts.
    pub fn start(&self) -> Vec<Effect> {
        vec![Effect::Refresh]
    }

    fn search_effect(&mut self, limit: usize) -> Effect {
        self.search_gen += 1;
        Effect::Search {
            generation: self.search_gen,
            query: self.query_string(),
            limit,
        }
    }

    fn query_changed(&mut self) -> Vec<Effect> {
        self.selected = 0;
        self.selected_id = None;
        self.list_offset = 0;
        self.limit = PAGE;
        self.jump_last = false;
        vec![self.search_effect(PAGE)]
    }

    fn say(&mut self, text: &str, kind: MessageKind, now: Instant, timed: bool) {
        self.message = Some(Message {
            text: text.to_string(),
            kind,
            expires: timed.then_some(now + MESSAGE_FOR),
        });
    }

    fn ensure_visible(&mut self) {
        let rows = self.list_rows();
        if self.selected < self.list_offset {
            self.list_offset = self.selected;
        } else if self.selected >= self.list_offset + rows {
            self.list_offset = self.selected + 1 - rows;
        }
        let max_offset = self.results.len().saturating_sub(rows);
        self.list_offset = self.list_offset.min(max_offset);
    }

    /// Effects after the selection moved: load the preview when the entry changed, and fetch
    /// more results when the end is near.
    fn selection_moved(&mut self) -> Vec<Effect> {
        self.ensure_visible();
        let mut effects = Vec::new();
        self.selected_id = self.selected_result().map(|r| r.id.clone());
        let modified_now = self.selected_result().map(|r| r.modified);
        let changed_on_disk = self.selected_id == self.preview_id
            && self.preview_mtime.is_some()
            && modified_now != self.preview_mtime;
        if self.selected_id != self.preview_id || changed_on_disk {
            if !changed_on_disk {
                self.preview_scroll = 0;
            }
            self.preview_id.clone_from(&self.selected_id);
            if let Some(r) = self.selected_result() {
                let (id, modified, use_count) = (r.id.clone(), r.modified, r.use_count);
                self.preview_mtime = Some(modified);
                self.preview_gen += 1;
                effects.push(Effect::LoadPreview {
                    generation: self.preview_gen,
                    id,
                    modified,
                    use_count,
                });
            }
        }
        if self.results.len() < self.total && self.selected + NEAR_END >= self.results.len() {
            self.limit = (self.results.len() * 2).max(PAGE);
            effects.push(self.search_effect(self.limit));
        }
        effects
    }

    fn move_to(&mut self, index: usize) -> Vec<Effect> {
        if self.results.is_empty() {
            return Vec::new();
        }
        self.selected = index.min(self.results.len() - 1);
        self.selection_moved()
    }

    pub fn update(&mut self, action: Action, now: Instant) -> Vec<Effect> {
        match action {
            Action::Key(e) => {
                if self.message.as_ref().is_some_and(|m| m.expires.is_none()) {
                    self.message = None;
                }
                match Key::from_event(e).and_then(|k| keys::resolve(k, self.mode())) {
                    Some(command) => self.command(command, now),
                    None => Vec::new(),
                }
            }
            Action::Paste(text) => {
                if self.mode() != Mode::Search {
                    return Vec::new();
                }
                let mut changed = false;
                for c in text.chars().filter(|c| *c != '\n' && *c != '\r') {
                    changed |= self.input.insert(c) != Change::None;
                }
                if changed {
                    self.query_changed()
                } else {
                    Vec::new()
                }
            }
            Action::Mouse(m) => self.mouse(m),
            Action::Resize(w, h) => {
                self.size = (w, h);
                self.ensure_visible();
                self.scroll_to(i64::try_from(self.preview_scroll).unwrap_or(0));
                Vec::new()
            }
            Action::Tick => {
                if self
                    .message
                    .as_ref()
                    .and_then(|m| m.expires)
                    .is_some_and(|t| now >= t)
                {
                    self.message = None;
                }
                let due = self
                    .status
                    .last_refresh
                    .is_some_and(|t| now.duration_since(t) >= self.status.interval);
                if self.options.refresh_timer && due && !self.status.refreshing {
                    self.status.refreshing = true;
                    return vec![Effect::Refresh];
                }
                Vec::new()
            }
            Action::Results(generation, page) => self.results_arrived(generation, page),
            Action::PreviewReady(generation, model) => {
                if generation == self.preview_gen {
                    self.preview = Some(*model);
                }
                Vec::new()
            }
            Action::Refreshed(info) => {
                self.status.refreshing = false;
                self.status.indexed = info.indexed;
                self.status.skipped = info.skipped;
                self.status.last_refresh = Some(now);
                self.status.interval = if info.duration > SLOW_REFRESH {
                    info.duration * 4
                } else {
                    REFRESH_EVERY
                };
                self.empty_kb = info.indexed + info.skipped == 0;
                let limit = self.limit;
                vec![self.search_effect(limit)]
            }
            Action::WorkerFailed(message) => {
                self.status.refreshing = false;
                self.say(
                    &format!("Refresh failed: {message}"),
                    MessageKind::Warning,
                    now,
                    false,
                );
                Vec::new()
            }
            Action::CopyDone(outcome) => {
                match outcome {
                    CopyOutcome::Copied => {
                        self.say("Copied (terminal clipboard)", MessageKind::Info, now, true);
                    }
                    CopyOutcome::TooLarge => self.say(
                        "Too large for the terminal clipboard",
                        MessageKind::Warning,
                        now,
                        true,
                    ),
                    CopyOutcome::Failed(m) => {
                        self.say(
                            &format!("Copy failed: {m}"),
                            MessageKind::Warning,
                            now,
                            true,
                        );
                    }
                }
                Vec::new()
            }
            Action::EditorReturned(error) => {
                self.status.refreshing = true;
                if let Some(text) = error {
                    self.say(&text, MessageKind::Warning, now, false);
                }
                vec![Effect::Refresh]
            }
        }
    }

    fn results_arrived(&mut self, generation: u64, page: Page) -> Vec<Effect> {
        if generation != self.search_gen {
            return Vec::new();
        }
        let old_index = self.selected;
        self.results = page.results;
        self.total = page.total;
        self.type_counts = page.type_counts;
        self.status.indexed = page.indexed;
        self.status.skipped = page.skipped;
        self.empty_kb = page.indexed + page.skipped == 0;
        self.loaded = true;
        if self.results.is_empty() {
            self.selected = 0;
            self.selected_id = None;
            self.preview = None;
            self.preview_id = None;
            self.preview_mtime = None;
            self.list_offset = 0;
            return Vec::new();
        }
        if self.jump_last && self.results.len() == self.total {
            self.jump_last = false;
            self.selected = self.results.len() - 1;
        } else if let Some(id) = &self.selected_id {
            self.selected = self
                .results
                .iter()
                .position(|r| &r.id == id)
                .unwrap_or_else(|| old_index.min(self.results.len() - 1));
        } else {
            self.selected = 0;
        }
        self.selection_moved()
    }

    fn command(&mut self, command: Command, now: Instant) -> Vec<Effect> {
        match command {
            Command::Insert(c) => {
                let change = self.input.insert(c);
                self.edited(change)
            }
            Command::Backspace => {
                let change = self.input.backspace();
                self.edited(change)
            }
            Command::Delete => {
                let changed = self.input.editor.delete();
                self.edited(if changed { Change::Text } else { Change::None })
            }
            Command::DeleteWord => {
                let changed = self.input.editor.delete_word();
                self.edited(if changed { Change::Text } else { Change::None })
            }
            Command::ClearLine => {
                let changed = self.input.editor.clear();
                self.edited(if changed { Change::Text } else { Change::None })
            }
            Command::Left => {
                self.input.editor.left();
                Vec::new()
            }
            Command::Right => {
                self.input.editor.right();
                Vec::new()
            }
            Command::LineStart => {
                self.input.editor.home();
                Vec::new()
            }
            Command::LineEnd => {
                self.input.editor.end();
                Vec::new()
            }
            Command::Down => self.move_to(self.selected + 1),
            Command::Up => self.move_to(self.selected.saturating_sub(1)),
            Command::PageDown => {
                let rows = self.list_rows();
                self.move_to(self.selected + rows)
            }
            Command::PageUp => {
                let rows = self.list_rows();
                self.move_to(self.selected.saturating_sub(rows))
            }
            Command::First => self.move_to(0),
            Command::Last => {
                if self.results.len() < self.total {
                    self.jump_last = true;
                    vec![self.search_effect(0)]
                } else {
                    self.move_to(usize::MAX)
                }
            }
            Command::Pick => match self.selected_result() {
                Some(r) => vec![
                    Effect::RecordUse(r.id.clone()),
                    Effect::Pick(self.options.print),
                ],
                None => {
                    self.say("Nothing to pick", MessageKind::Info, now, true);
                    Vec::new()
                }
            },
            Command::Copy => {
                if self.selected_result().is_some() {
                    vec![Effect::CopyPayload]
                } else {
                    self.say("Nothing to pick", MessageKind::Info, now, true);
                    Vec::new()
                }
            }
            Command::Edit => match self.selected_result() {
                Some(r) => vec![Effect::EditEntry(r.id.clone())],
                None => {
                    self.say("Nothing to pick", MessageKind::Info, now, true);
                    Vec::new()
                }
            },
            Command::Refresh => {
                self.status.refreshing = true;
                vec![Effect::Refresh]
            }
            Command::Esc => {
                if self.layout_mode == LayoutMode::FullPreview {
                    self.layout_mode = LayoutMode::Split;
                    self.ensure_visible();
                    Vec::new()
                } else if self.input.is_blank() {
                    vec![Effect::Quit(130)]
                } else {
                    self.input.clear_all();
                    self.query_changed()
                }
            }
            Command::Quit => vec![Effect::Quit(130)],
            Command::HelpKey => {
                if self.input.editor.is_empty() {
                    self.overlay = Overlay::Help;
                    Vec::new()
                } else {
                    let change = self.input.insert('?');
                    self.edited(change)
                }
            }
            Command::Help => {
                self.overlay = Overlay::Help;
                Vec::new()
            }
            Command::CloseHelp => {
                self.overlay = Overlay::None;
                Vec::new()
            }
            Command::TogglePreview => {
                self.layout_mode = match self.layout_mode {
                    LayoutMode::Split => LayoutMode::FullPreview,
                    LayoutMode::FullPreview => LayoutMode::Split,
                };
                self.ensure_visible();
                self.scroll_to(i64::try_from(self.preview_scroll).unwrap_or(0));
                Vec::new()
            }
            Command::TypeNext => self.cycle_type(1),
            Command::TypePrev => self.cycle_type(-1),
            Command::ScrollDown => self.scroll(1),
            Command::ScrollUp => self.scroll(-1),
            Command::ScrollPageDown => self.scroll_page(1),
            Command::ScrollPageUp => self.scroll_page(-1),
        }
    }

    /// Tab and Shift-Tab: one type at a time through all the types and back to "all". After
    /// several typed types, forward continues after the last of them and back before the first.
    fn cycle_type(&mut self, dir: i64) -> Vec<Effect> {
        use kb_core::entry::EntryType;
        let all = EntryType::ALL;
        let typed = self.input.type_chips();
        let index = |t: &EntryType| all.iter().position(|x| x == t);
        let next: Option<EntryType> = if dir > 0 {
            match typed.last().and_then(index) {
                None => Some(all[0]),
                Some(i) => all.get(i + 1).copied(),
            }
        } else {
            match typed.first().and_then(index) {
                None => Some(all[all.len() - 1]),
                Some(0) => None,
                Some(i) => Some(all[i - 1]),
            }
        };
        self.input.set_type(next);
        self.query_changed()
    }

    /// Wheel scrolls the list or the preview under the pointer, a left click selects a row.
    /// Nothing else is handled, and nothing at all unless the config turned the mouse on.
    fn mouse(&mut self, m: MouseEvent) -> Vec<Effect> {
        use ratatui::crossterm::event::{MouseButton, MouseEventKind};
        use ratatui::layout::Position;
        if !self.options.mouse || self.mode() != Mode::Search && self.mode() != Mode::FullPreview {
            return Vec::new();
        }
        let area = ratatui::layout::Rect::new(0, 0, self.size.0, self.size.1);
        let Screen::Ok(l) = layout::layout(area, self.layout_mode == LayoutMode::FullPreview)
        else {
            return Vec::new();
        };
        let at = Position::new(m.column, m.row);
        let in_list = l.list.is_some_and(|r| r.contains(at));
        let in_preview = l.preview.is_some_and(|r| r.contains(at));
        match m.kind {
            MouseEventKind::ScrollDown if in_list => self.move_to(self.selected + 3),
            MouseEventKind::ScrollUp if in_list => self.move_to(self.selected.saturating_sub(3)),
            MouseEventKind::ScrollDown if in_preview => self.scroll(3),
            MouseEventKind::ScrollUp if in_preview => self.scroll(-3),
            MouseEventKind::Down(MouseButton::Left) if in_list => {
                let top = l.list.map_or(0, |r| r.y + 1);
                if m.row < top {
                    return Vec::new();
                }
                let row = usize::from(m.row - top) / 2 + self.list_offset;
                if row < self.results.len() {
                    self.move_to(row)
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    fn scroll_to(&mut self, wanted: i64) {
        let Some((rows, height)) = super::view::preview_metrics(self) else {
            return;
        };
        let max = i64::try_from(rows.saturating_sub(height)).unwrap_or(0);
        self.preview_scroll = usize::try_from(wanted.clamp(0, max)).unwrap_or(0);
    }

    fn scroll(&mut self, by: i64) -> Vec<Effect> {
        self.scroll_to(i64::try_from(self.preview_scroll).unwrap_or(0) + by);
        Vec::new()
    }

    fn scroll_page(&mut self, dir: i64) -> Vec<Effect> {
        let height = super::view::preview_metrics(self).map_or(1, |(_, h)| h.max(1));
        self.scroll(dir * i64::try_from(height).unwrap_or(1))
    }

    fn edited(&mut self, change: Change) -> Vec<Effect> {
        match change {
            Change::None => Vec::new(),
            Change::Text | Change::Chips => self.query_changed(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use kb_core::entry::EntryType;
    use kb_core::search::Tier;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    pub(crate) fn res(id: &str, ty: EntryType) -> SearchResult {
        SearchResult {
            id: id.to_string(),
            use_count: 0,
            updated: None,
            modified: None,
            entry_type: ty,
            title: id.to_string(),
            tags: vec![],
            path: format!("{id}.md"),
            score: 1.0,
            tier: Tier::Exact,
            matches: vec![],
            snippet: None,
        }
    }

    pub(crate) fn page(ids: &[&str]) -> Page {
        Page {
            results: ids.iter().map(|i| res(i, EntryType::Sql)).collect(),
            total: ids.len(),
            indexed: 100,
            skipped: 0,
            ..Page::default()
        }
    }

    pub(crate) fn key(code: KeyCode, mods: KeyModifiers) -> Action {
        Action::Key(KeyEvent::new(code, mods))
    }

    pub(crate) fn ch(c: char) -> Action {
        key(KeyCode::Char(c), KeyModifiers::NONE)
    }

    pub(crate) fn ctrl(c: char) -> Action {
        key(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    pub(crate) fn state() -> AppState {
        AppState::new(Options::default(), (120, 40), "", "~/kb".into())
    }

    fn loaded(ids: &[&str]) -> AppState {
        let mut s = state();
        let now = Instant::now();
        s.update(
            Action::Refreshed(RefreshInfo {
                indexed: 100,
                skipped: 0,
                duration: Duration::from_millis(10),
            }),
            now,
        );
        let g = s.search_gen;
        s.update(Action::Results(g, page(ids)), now);
        s
    }

    fn search_of(effects: &[Effect]) -> Option<(u64, String, usize)> {
        effects.iter().find_map(|e| match e {
            Effect::Search {
                generation,
                query,
                limit,
            } => Some((*generation, query.clone(), *limit)),
            _ => None,
        })
    }

    #[test]
    fn typing_searches_with_a_new_generation_and_resets_the_selection() {
        let mut s = loaded(&["a", "b", "c"]);
        let now = Instant::now();
        s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
        assert_eq!(s.selected, 1);
        let g0 = s.search_gen;
        let e = s.update(ch('5'), now);
        assert_eq!(search_of(&e), Some((g0 + 1, "5".into(), PAGE)));
        assert_eq!(s.selected, 0);
        let e = s.update(ch('x'), now);
        assert_eq!(search_of(&e), Some((g0 + 2, "5x".into(), PAGE)));
        assert_eq!(s.input.editor.text(), "5x");
    }

    #[test]
    fn stale_results_are_ignored_and_current_ones_replace_the_page() {
        let mut s = loaded(&["a", "b"]);
        let now = Instant::now();
        s.update(ch('x'), now);
        let stale = s.search_gen - 1;
        let before = s.results.clone();
        assert!(
            s.update(Action::Results(stale, page(&["z"])), now)
                .is_empty()
        );
        assert_eq!(s.results, before);
        let mut p = page(&["n1", "n2", "n3"]);
        p.type_counts = [3, 0, 0, 0, 0, 0, 0];
        p.indexed = 50;
        p.skipped = 2;
        let g = s.search_gen;
        let e = s.update(Action::Results(g, p), now);
        assert_eq!(s.results.len(), 3);
        assert_eq!((s.selected, s.total, s.type_counts[0]), (0, 3, 3));
        assert_eq!((s.status.indexed, s.status.skipped), (50, 2));
        assert!(
            e.iter()
                .any(|x| matches!(x, Effect::LoadPreview { id, .. } if id == "n1"))
        );
    }

    #[test]
    fn the_selection_follows_the_entry_across_refreshes() {
        let mut s = loaded(&["a", "b", "c", "d"]);
        let now = Instant::now();
        s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
        s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
        assert_eq!(s.selected_id.as_deref(), Some("c"));
        s.update(
            Action::Refreshed(RefreshInfo {
                indexed: 100,
                skipped: 0,
                duration: Duration::ZERO,
            }),
            now,
        );
        let g = s.search_gen;
        let e = s.update(Action::Results(g, page(&["x", "c", "d"])), now);
        assert_eq!((s.selected, s.selected_id.as_deref()), (1, Some("c")));
        assert!(
            !e.iter().any(|x| matches!(x, Effect::LoadPreview { .. })),
            "same entry: no new preview"
        );
        // the selected entry vanished: nearest remaining index
        s.update(
            Action::Refreshed(RefreshInfo {
                indexed: 99,
                skipped: 0,
                duration: Duration::ZERO,
            }),
            now,
        );
        let g = s.search_gen;
        let e = s.update(Action::Results(g, page(&["x", "d"])), now);
        assert_eq!(s.selected, 1);
        assert!(
            e.iter()
                .any(|x| matches!(x, Effect::LoadPreview { id, .. } if id == "d"))
        );
        s.update(
            Action::Refreshed(RefreshInfo {
                indexed: 1,
                skipped: 0,
                duration: Duration::ZERO,
            }),
            now,
        );
        let g = s.search_gen;
        s.update(Action::Results(g, page(&[])), now);
        assert!(s.results.is_empty() && s.selected_id.is_none() && s.preview_id.is_none());
    }

    #[test]
    fn movement_keys_stay_in_bounds() {
        let ids: Vec<String> = (0..30).map(|i| format!("e{i:02}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut s = loaded(&refs);
        let now = Instant::now();
        let k = |c, m| key(c, m);
        s.update(k(KeyCode::Down, KeyModifiers::NONE), now);
        s.update(ctrl('n'), now);
        s.update(ctrl('j'), now);
        assert_eq!(s.selected, 3);
        s.update(ctrl('p'), now);
        s.update(ctrl('k'), now);
        s.update(k(KeyCode::Up, KeyModifiers::NONE), now);
        assert_eq!(s.selected, 0);
        s.update(k(KeyCode::Up, KeyModifiers::NONE), now);
        assert_eq!(s.selected, 0);
        let rows = s.list_rows();
        assert_eq!(rows, 17);
        s.update(k(KeyCode::PageDown, KeyModifiers::NONE), now);
        assert_eq!(s.selected, rows);
        s.update(k(KeyCode::End, KeyModifiers::CONTROL), now);
        assert_eq!(s.selected, 29);
        assert!(s.selected >= s.list_offset && s.selected < s.list_offset + rows);
        s.update(k(KeyCode::PageDown, KeyModifiers::NONE), now);
        assert_eq!(s.selected, 29);
        s.update(k(KeyCode::Home, KeyModifiers::CONTROL), now);
        assert_eq!((s.selected, s.list_offset), (0, 0));
        s.update(k(KeyCode::PageUp, KeyModifiers::NONE), now);
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn more_results_are_requested_near_the_end_and_for_ctrl_end() {
        let ids: Vec<String> = (0..60).map(|i| format!("e{i:02}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut s = loaded(&refs);
        s.total = 500;
        let now = Instant::now();
        for _ in 0..40 {
            let e = s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
            assert!(search_of(&e).is_none(), "selected {}", s.selected);
        }
        let mut got = None;
        for _ in 0..12 {
            let e = s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
            if let Some(x) = search_of(&e) {
                got = Some(x);
                break;
            }
        }
        let (g, q, limit) = got.expect("a request near the end");
        assert_eq!((q.as_str(), limit), ("", 120));
        assert_eq!(g, s.search_gen);
        let e = s.update(key(KeyCode::End, KeyModifiers::CONTROL), now);
        assert_eq!(search_of(&e).map(|x| x.2), Some(0));
        let mut p = page(&[]);
        p.results = (0..500)
            .map(|i| res(&format!("f{i:03}"), EntryType::Sql))
            .collect();
        p.total = 500;
        let g = s.search_gen;
        s.update(Action::Results(g, p), now);
        assert_eq!(s.selected, 499);
    }

    #[test]
    fn enter_records_the_use_then_picks() {
        let mut s = loaded(&["a", "b"]);
        s.options.print = PrintMode::Id;
        let e = s.update(key(KeyCode::Enter, KeyModifiers::NONE), Instant::now());
        assert_eq!(
            e,
            vec![Effect::RecordUse("a".into()), Effect::Pick(PrintMode::Id)]
        );
    }

    #[test]
    fn enter_with_nothing_selected_says_so_for_two_seconds() {
        let mut s = loaded(&[]);
        let t0 = Instant::now();
        assert!(
            s.update(key(KeyCode::Enter, KeyModifiers::NONE), t0)
                .is_empty()
        );
        assert_eq!(
            s.message.as_ref().map(|m| m.text.as_str()),
            Some("Nothing to pick")
        );
        s.update(Action::Tick, t0 + Duration::from_millis(1900));
        assert!(s.message.is_some());
        s.update(Action::Tick, t0 + Duration::from_millis(2100));
        assert!(s.message.is_none());
    }

    #[test]
    fn escape_clears_then_quits_and_ctrl_c_always_quits() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        s.update(ch('f'), now);
        s.update(ch('o'), now);
        let e = s.update(key(KeyCode::Esc, KeyModifiers::NONE), now);
        assert!(search_of(&e).is_some() && s.input.is_blank());
        let e = s.update(key(KeyCode::Esc, KeyModifiers::NONE), now);
        assert_eq!(e, vec![Effect::Quit(130)]);
        s.update(ch('f'), now);
        assert_eq!(s.update(ctrl('c'), now), vec![Effect::Quit(130)]);
        s.size = (40, 10);
        assert_eq!(s.mode(), Mode::TooSmall);
        assert_eq!(s.update(ctrl('c'), now), vec![Effect::Quit(130)]);
    }

    #[test]
    fn resize_keeps_everything_and_keeps_the_selection_visible() {
        let ids: Vec<String> = (0..30).map(|i| format!("e{i:02}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut s = loaded(&refs);
        let now = Instant::now();
        s.update(ch('q'), now);
        let g = s.search_gen;
        s.update(Action::Results(g, page(&refs)), now);
        s.update(key(KeyCode::End, KeyModifiers::CONTROL), now);
        let text = s.input.editor.text().to_string();
        s.update(Action::Resize(80, 20), now);
        assert_eq!(s.input.editor.text(), text);
        assert_eq!(s.selected_id.as_deref(), Some("e29"));
        let rows = s.list_rows();
        assert!(s.selected >= s.list_offset && s.selected < s.list_offset + rows);
    }

    #[test]
    fn refreshing_follows_the_timer_and_backs_off_when_slow() {
        let mut s = state();
        let t0 = Instant::now();
        assert!(s.status.refreshing);
        assert_eq!(s.start(), vec![Effect::Refresh]);
        let e = s.update(
            Action::Refreshed(RefreshInfo {
                indexed: 5,
                skipped: 1,
                duration: Duration::from_millis(10),
            }),
            t0,
        );
        assert!(!s.status.refreshing && search_of(&e).is_some());
        assert_eq!(
            (s.status.indexed, s.status.skipped, s.empty_kb),
            (5, 1, false)
        );
        assert!(
            s.update(Action::Tick, t0 + Duration::from_millis(900))
                .is_empty()
        );
        assert_eq!(
            s.update(Action::Tick, t0 + Duration::from_millis(1100)),
            vec![Effect::Refresh]
        );
        assert!(s.status.refreshing);
        assert!(
            s.update(Action::Tick, t0 + Duration::from_secs(5))
                .is_empty(),
            "one at a time"
        );
        let t1 = t0 + Duration::from_secs(2);
        s.update(
            Action::Refreshed(RefreshInfo {
                indexed: 5,
                skipped: 0,
                duration: Duration::from_millis(300),
            }),
            t1,
        );
        assert!(
            s.update(Action::Tick, t1 + Duration::from_millis(1100))
                .is_empty()
        );
        assert_eq!(
            s.update(Action::Tick, t1 + Duration::from_millis(1250)),
            vec![Effect::Refresh]
        );
        let mut quiet = state();
        quiet.options.refresh_timer = false;
        quiet.update(
            Action::Refreshed(RefreshInfo {
                indexed: 1,
                skipped: 0,
                duration: Duration::ZERO,
            }),
            t0,
        );
        assert!(
            quiet
                .update(Action::Tick, t0 + Duration::from_secs(10))
                .is_empty()
        );
        let mut empty = state();
        empty.update(
            Action::Refreshed(RefreshInfo {
                indexed: 0,
                skipped: 0,
                duration: Duration::ZERO,
            }),
            t0,
        );
        assert!(empty.empty_kb);
    }

    #[test]
    fn a_worker_failure_is_a_notice_that_lasts_until_the_next_key() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        s.update(Action::WorkerFailed("disk gone".into()), now);
        let m = s.message.clone().unwrap();
        assert!(
            m.text.contains("disk gone") && m.kind == MessageKind::Warning && m.expires.is_none()
        );
        s.update(Action::Tick, now + Duration::from_secs(60));
        assert!(s.message.is_some());
        s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
        assert!(s.message.is_none());
    }

    #[test]
    fn paste_inserts_text_without_newlines_and_never_picks() {
        let mut s = loaded(&["a"]);
        let e = s.update(Action::Paste("some-id\n".into()), Instant::now());
        assert_eq!(s.input.editor.text(), "some-id");
        assert!(search_of(&e).is_some() && !e.iter().any(|x| matches!(x, Effect::Pick(_))));
    }

    #[test]
    fn copy_messages_expire() {
        let mut s = loaded(&["a"]);
        let t0 = Instant::now();
        assert_eq!(s.update(ctrl('y'), t0), vec![Effect::CopyPayload]);
        s.update(Action::CopyDone(CopyOutcome::Copied), t0);
        assert_eq!(
            s.message.as_ref().unwrap().text,
            "Copied (terminal clipboard)"
        );
        s.update(Action::Tick, t0 + Duration::from_millis(2100));
        assert!(s.message.is_none());
        s.update(Action::CopyDone(CopyOutcome::TooLarge), t0);
        assert!(s.message.as_ref().unwrap().text.contains("Too large"));
    }

    #[test]
    fn line_editing_keys_edit_the_text_and_search() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        for c in "foo bar".chars() {
            s.update(ch(c), now);
        }
        s.update(ctrl('w'), now);
        assert_eq!(s.input.editor.text(), "foo ");
        s.update(ctrl('a'), now);
        assert_eq!(s.input.editor.cursor(), 0);
        s.update(key(KeyCode::End, KeyModifiers::NONE), now);
        assert_eq!(s.input.editor.cursor(), 4);
        let e = s.update(ctrl('u'), now);
        assert!(s.input.editor.is_empty() && search_of(&e).is_some());
        let e = s.update(ctrl('u'), now);
        assert!(e.is_empty(), "nothing changed, nothing to search");
    }

    // ---- preview scrolling and loading (US2) ----

    fn long_model(id: &str, lines: usize) -> PreviewModel {
        use crate::tui::highlight::Span;
        use crate::tui::theme::Class;
        PreviewModel {
            id: id.to_string(),
            title: id.to_string(),
            path: format!("{id}.md"),
            payload: (0..lines)
                .map(|i| vec![Span::new(format!("line {i}"), Class::Plain)])
                .collect(),
            ..PreviewModel::default()
        }
    }

    fn with_preview(lines: usize) -> AppState {
        let mut s = loaded(&["a", "b", "c"]);
        let now = Instant::now();
        s.update(
            Action::PreviewReady(s.preview_gen, Box::new(long_model("a", lines))),
            now,
        );
        s
    }

    #[test]
    fn selecting_another_result_resets_the_scroll_and_loads_its_preview() {
        let mut s = with_preview(100);
        let now = Instant::now();
        s.update(key(KeyCode::Down, KeyModifiers::SHIFT), now);
        s.update(key(KeyCode::Down, KeyModifiers::SHIFT), now);
        assert_eq!(s.preview_scroll, 2);
        let before = s.preview_gen;
        let e = s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
        assert_eq!(s.preview_scroll, 0);
        assert_eq!(s.preview_gen, before + 1);
        assert!(e.iter().any(|x| matches!(x, Effect::LoadPreview { id, generation, .. } if id == "b" && *generation == before + 1)));
        // an answer for the old request is ignored, the new one is shown
        s.update(
            Action::PreviewReady(before, Box::new(long_model("a", 5))),
            now,
        );
        assert_eq!(
            s.preview.as_ref().map(|p| p.id.as_str()),
            Some("a"),
            "the old preview stays until the new one arrives"
        );
        s.update(
            Action::PreviewReady(before + 1, Box::new(long_model("b", 5))),
            now,
        );
        assert_eq!(s.preview.as_ref().map(|p| p.id.as_str()), Some("b"));
    }

    #[test]
    fn shift_arrows_and_pages_scroll_the_preview_within_bounds_without_moving_the_selection() {
        let mut s = with_preview(100);
        let now = Instant::now();
        let (rows, height) = crate::tui::view::preview_metrics(&s).unwrap();
        assert!(rows > height && height > 10, "{rows} {height}");
        let max = rows - height;
        s.update(key(KeyCode::Down, KeyModifiers::SHIFT), now);
        assert_eq!((s.preview_scroll, s.selected), (1, 0));
        s.update(key(KeyCode::Up, KeyModifiers::SHIFT), now);
        s.update(key(KeyCode::Up, KeyModifiers::SHIFT), now);
        assert_eq!(s.preview_scroll, 0);
        s.update(key(KeyCode::PageDown, KeyModifiers::SHIFT), now);
        assert_eq!(s.preview_scroll, height);
        for _ in 0..10 {
            s.update(key(KeyCode::PageDown, KeyModifiers::SHIFT), now);
        }
        assert_eq!(s.preview_scroll, max);
        s.update(key(KeyCode::PageUp, KeyModifiers::SHIFT), now);
        assert_eq!(s.preview_scroll, max - height);
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn a_short_preview_does_not_scroll_and_resizing_clamps() {
        let mut s = with_preview(3);
        let now = Instant::now();
        s.update(key(KeyCode::Down, KeyModifiers::SHIFT), now);
        assert_eq!(s.preview_scroll, 0);
        let mut s = with_preview(100);
        for _ in 0..10 {
            s.update(key(KeyCode::PageDown, KeyModifiers::SHIFT), now);
        }
        let big = s.preview_scroll;
        s.update(Action::Resize(200, 80), now);
        assert!(s.preview_scroll < big);
        let (rows, height) = crate::tui::view::preview_metrics(&s).unwrap();
        assert!(s.preview_scroll <= rows.saturating_sub(height));
    }

    // ---- chips and the type filter (US3) ----

    fn type_chips(s: &AppState) -> Vec<&'static str> {
        s.input.type_chips().iter().map(|t| t.name()).collect()
    }

    #[test]
    fn tab_cycles_through_the_types_and_back_to_all() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        let order = ["sql", "sumo", "cw", "cmd", "prompt", "link", "code"];
        for ty in order {
            let e = s.update(key(KeyCode::Tab, KeyModifiers::NONE), now);
            assert_eq!(type_chips(&s), vec![ty]);
            let (_, q, limit) = search_of(&e).unwrap();
            assert_eq!(q, format!("type:{ty}"));
            assert_eq!(limit, PAGE);
        }
        let e = s.update(key(KeyCode::Tab, KeyModifiers::NONE), now);
        assert!(type_chips(&s).is_empty());
        assert_eq!(search_of(&e).unwrap().1, "");
    }

    #[test]
    fn shift_tab_goes_the_other_way() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        s.update(key(KeyCode::BackTab, KeyModifiers::SHIFT), now);
        assert_eq!(type_chips(&s), vec!["code"]);
        s.update(key(KeyCode::BackTab, KeyModifiers::SHIFT), now);
        assert_eq!(type_chips(&s), vec!["link"]);
        for _ in 0..5 {
            s.update(key(KeyCode::BackTab, KeyModifiers::SHIFT), now);
        }
        assert_eq!(type_chips(&s), vec!["sql"]);
        s.update(key(KeyCode::BackTab, KeyModifiers::SHIFT), now);
        assert!(type_chips(&s).is_empty());
    }

    #[test]
    fn tab_replaces_typed_type_filters_by_a_single_one() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        for c in "type:sql type:cw ".chars() {
            s.update(ch(c), now);
        }
        assert_eq!(type_chips(&s), vec!["sql", "cw"]);
        s.update(key(KeyCode::Tab, KeyModifiers::NONE), now);
        assert_eq!(
            type_chips(&s),
            vec!["cmd"],
            "the next type after the last typed one"
        );
        for c in "type:sumo ".chars() {
            s.update(ch(c), now);
        }
        assert_eq!(type_chips(&s), vec!["cmd", "sumo"]);
        s.update(key(KeyCode::BackTab, KeyModifiers::SHIFT), now);
        assert_eq!(
            type_chips(&s),
            vec!["cw"],
            "the type before the first one (cmd)"
        );
    }

    #[test]
    fn typing_a_filter_and_a_space_makes_a_chip_and_searches() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        let mut last = Vec::new();
        for c in "tag:prod ".chars() {
            last = s.update(ch(c), now);
        }
        assert_eq!(s.input.chips.len(), 1);
        assert_eq!(search_of(&last).unwrap().1, "tag:prod");
        assert!(s.input.editor.is_empty());
        for c in "-tag:stage latency".chars() {
            last = s.update(ch(c), now);
        }
        assert_eq!(search_of(&last).unwrap().1, "tag:prod -tag:stage latency");
        // Backspace on a non-empty line deletes a character, on an empty line removes the last chip
        for _ in 0.."latency".len() {
            s.update(key(KeyCode::Backspace, KeyModifiers::NONE), now);
        }
        assert!(s.input.editor.is_empty() && s.input.chips.len() == 2);
        let e = s.update(key(KeyCode::Backspace, KeyModifiers::NONE), now);
        assert_eq!(s.input.chips.len(), 1);
        assert_eq!(search_of(&e).unwrap().1, "tag:prod");
        let e = s.update(key(KeyCode::Backspace, KeyModifiers::NONE), now);
        assert!(s.input.chips.is_empty() && search_of(&e).is_some());
        assert!(
            s.update(key(KeyCode::Backspace, KeyModifiers::NONE), now)
                .is_empty()
        );
    }

    #[test]
    fn escape_clears_chips_too_and_the_query_string_equals_the_cli() {
        use kb_core::query::Query;
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        for c in "type:cw tag:prod latency".chars() {
            s.update(ch(c), now);
        }
        let q = Query::parse(&s.query_string());
        assert_eq!(
            q,
            Query::from_parts("latency", &["cw".into()], &["prod".into()], &[], None).unwrap()
        );
        let e = s.update(key(KeyCode::Esc, KeyModifiers::NONE), now);
        assert!(s.input.is_blank());
        assert_eq!(search_of(&e).unwrap().1, "");
    }

    #[test]
    fn type_counts_come_from_the_page_whatever_the_filter() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        s.update(key(KeyCode::Tab, KeyModifiers::NONE), now);
        let mut p = page(&["x"]);
        p.type_counts = [4, 3, 2, 1, 0, 0, 0];
        let g = s.search_gen;
        s.update(Action::Results(g, p), now);
        assert_eq!(s.type_counts, [4, 3, 2, 1, 0, 0, 0]);
    }

    // ---- editing (US4) ----

    #[test]
    fn ctrl_e_edits_the_selected_entry_and_refreshes_when_it_returns() {
        let mut s = loaded(&["a", "b"]);
        let now = Instant::now();
        s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
        for c in "fo".chars() {
            s.update(ch(c), now);
        }
        let g = s.search_gen;
        s.update(Action::Results(g, page(&["a", "b"])), now);
        s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
        assert_eq!(
            s.update(ctrl('e'), now),
            vec![Effect::EditEntry("b".into())]
        );
        let e = s.update(Action::EditorReturned(None), now);
        assert!(s.status.refreshing && e == vec![Effect::Refresh]);
        // after the refresh the same query runs again and the selection stays on the entry
        let e = s.update(
            Action::Refreshed(RefreshInfo {
                indexed: 100,
                skipped: 0,
                duration: Duration::ZERO,
            }),
            now,
        );
        assert_eq!(search_of(&e).unwrap().1, "fo");
        let g = s.search_gen;
        s.update(Action::Results(g, page(&["a", "b"])), now);
        assert_eq!(s.selected_id.as_deref(), Some("b"));
        assert_eq!(s.input.editor.text(), "fo");
    }

    #[test]
    fn ctrl_e_with_nothing_selected_says_so() {
        let mut s = loaded(&[]);
        assert!(s.update(ctrl('e'), Instant::now()).is_empty());
        assert_eq!(
            s.message.as_ref().map(|m| m.text.as_str()),
            Some("Nothing to pick")
        );
    }

    #[test]
    fn an_error_in_the_edited_file_is_shown_until_the_next_key() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        s.update(
            Action::EditorReturned(Some("a.md:3 KB001-unknown-type unknown type".into())),
            now,
        );
        let m = s.message.clone().unwrap();
        assert!(m.text.contains("KB001") && m.kind == MessageKind::Warning && m.expires.is_none());
        s.update(Action::Tick, now + Duration::from_secs(30));
        assert!(s.message.is_some(), "it does not time out");
        s.update(ch('x'), now);
        assert!(s.message.is_none());
    }

    #[test]
    fn a_file_that_changed_on_disk_reloads_its_preview_but_keeps_the_scroll() {
        let mut s = with_preview(100);
        let now = Instant::now();
        s.update(key(KeyCode::Down, KeyModifiers::SHIFT), now);
        s.update(key(KeyCode::Down, KeyModifiers::SHIFT), now);
        assert_eq!(s.preview_scroll, 2);
        // the same entry again, same file time: nothing to load
        s.update(
            Action::Refreshed(RefreshInfo {
                indexed: 100,
                skipped: 0,
                duration: Duration::ZERO,
            }),
            now,
        );
        let g = s.search_gen;
        let e = s.update(Action::Results(g, page(&["a", "b", "c"])), now);
        assert!(!e.iter().any(|x| matches!(x, Effect::LoadPreview { .. })));
        // the file changed: load it again
        s.update(
            Action::Refreshed(RefreshInfo {
                indexed: 100,
                skipped: 0,
                duration: Duration::ZERO,
            }),
            now,
        );
        let g = s.search_gen;
        let mut p = page(&["a", "b", "c"]);
        p.results[0].modified = Some(1_790_000_123);
        let e = s.update(Action::Results(g, p), now);
        assert!(e.iter().any(|x| matches!(x, Effect::LoadPreview { id, modified: Some(1_790_000_123), .. } if id == "a")), "{e:?}");
        assert_eq!(s.preview_scroll, 2, "the reader keeps their place");
    }

    // ---- help overlay (US5) ----

    #[test]
    fn question_mark_opens_help_only_on_an_empty_text_line() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        assert!(s.update(ch('?'), now).is_empty());
        assert_eq!(s.overlay, Overlay::Help);
        assert_eq!(s.mode(), Mode::Help);
        s.update(ch('x'), now);
        assert_eq!(s.overlay, Overlay::None, "any key closes it");
        assert!(s.input.editor.is_empty(), "and the key is consumed");
        s.update(ch('f'), now);
        let e = s.update(ch('?'), now);
        assert_eq!(s.overlay, Overlay::None);
        assert_eq!(s.input.editor.text(), "f?");
        assert!(search_of(&e).is_some());
    }

    #[test]
    fn f1_opens_help_in_any_state_and_every_key_closes_it_without_side_effects() {
        let mut s = loaded(&["a", "b"]);
        let now = Instant::now();
        s.update(ch('f'), now);
        for k in [
            key(KeyCode::Enter, KeyModifiers::NONE),
            key(KeyCode::Esc, KeyModifiers::NONE),
            ch('q'),
            key(KeyCode::Down, KeyModifiers::NONE),
            ctrl('e'),
        ] {
            s.update(key(KeyCode::F(1), KeyModifiers::NONE), now);
            assert_eq!(s.overlay, Overlay::Help);
            let e = s.update(k, now);
            assert!(e.is_empty(), "{e:?}");
            assert_eq!(s.overlay, Overlay::None);
        }
        assert_eq!((s.selected, s.input.editor.text()), (0, "f"));
        s.update(key(KeyCode::F(1), KeyModifiers::NONE), now);
        assert_eq!(
            s.update(ctrl('c'), now),
            vec![Effect::Quit(130)],
            "Ctrl-C still quits"
        );
    }

    #[test]
    fn the_key_bar_follows_the_state() {
        use crate::tui::view::key_bar_text;
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        assert_eq!(
            key_bar_text(&s, 200),
            "↑↓ select · Enter pick · Tab type · ^Y copy · ^E edit · ? help · Esc quit"
        );
        s.update(ch('x'), now);
        assert!(key_bar_text(&s, 200).ends_with("Esc clear"));
        let g = s.search_gen;
        s.update(Action::Results(g, page(&[])), now);
        assert_eq!(key_bar_text(&s, 200), "Esc clear · ? help");
        s.update(key(KeyCode::F(1), KeyModifiers::NONE), now);
        assert_eq!(key_bar_text(&s, 200), "any key closes");
        let mut e = state();
        e.empty_kb = true;
        assert_eq!(key_bar_text(&e, 200), "Esc quit · ? help");
    }

    // ---- narrow terminals, full preview, too small ----

    fn alt(c: char) -> Action {
        key(KeyCode::Char(c), KeyModifiers::ALT)
    }

    #[test]
    fn alt_p_toggles_the_full_preview_and_esc_leaves_it() {
        let mut s = loaded(&["a", "b"]);
        s.size = (80, 24);
        let now = Instant::now();
        assert_eq!(s.mode(), Mode::Search);
        assert!(s.update(alt('p'), now).is_empty());
        assert_eq!(
            (s.layout_mode, s.mode()),
            (LayoutMode::FullPreview, Mode::FullPreview)
        );
        s.update(alt('p'), now);
        assert_eq!(s.mode(), Mode::Search);
        s.update(alt('p'), now);
        let e = s.update(key(KeyCode::Esc, KeyModifiers::NONE), now);
        assert!(e.is_empty(), "Esc only returns to the list");
        assert_eq!(s.mode(), Mode::Search);
        // the list keys do nothing while the preview is full screen; typing neither
        s.update(alt('p'), now);
        assert!(
            s.update(key(KeyCode::Down, KeyModifiers::NONE), now)
                .is_empty()
        );
        assert!(s.update(ch('x'), now).is_empty());
        assert_eq!((s.selected, s.input.editor.text()), (0, ""));
        // Enter, Ctrl-Y and Ctrl-E still work on the selected entry
        assert!(matches!(
            s.update(key(KeyCode::Enter, KeyModifiers::NONE), now)
                .as_slice(),
            [Effect::RecordUse(_), Effect::Pick(_)]
        ));
        assert_eq!(
            s.update(ctrl('e'), now),
            vec![Effect::EditEntry("a".into())]
        );
    }

    #[test]
    fn full_preview_scrolls_with_shift_arrows() {
        let mut s = with_preview(100);
        s.size = (80, 24);
        let now = Instant::now();
        s.update(alt('p'), now);
        s.update(key(KeyCode::Down, KeyModifiers::SHIFT), now);
        s.update(key(KeyCode::PageDown, KeyModifiers::SHIFT), now);
        assert!(s.preview_scroll > 1);
    }

    #[test]
    fn too_small_ignores_everything_but_ctrl_c_and_resize_restores_the_state() {
        let mut s = loaded(&["a", "b"]);
        let now = Instant::now();
        s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
        s.update(alt('p'), now);
        s.update(Action::Resize(59, 15), now);
        assert_eq!(s.mode(), Mode::TooSmall);
        for k in [
            ch('x'),
            key(KeyCode::Enter, KeyModifiers::NONE),
            key(KeyCode::Down, KeyModifiers::NONE),
            key(KeyCode::Esc, KeyModifiers::NONE),
            alt('p'),
        ] {
            assert!(s.update(k, now).is_empty());
        }
        assert_eq!(s.selected_id.as_deref(), Some("b"));
        s.update(Action::Resize(80, 14), now);
        assert_eq!(s.mode(), Mode::TooSmall);
        s.update(Action::Resize(60, 15), now);
        assert_eq!(s.mode(), Mode::FullPreview, "back to what it was");
        assert_eq!(s.selected_id.as_deref(), Some("b"));
        assert_eq!(s.update(ctrl('c'), now), vec![Effect::Quit(130)]);
    }

    // ---- mouse (optional, off by default) ----

    fn wheel(down: bool, column: u16, row: u16) -> Action {
        use ratatui::crossterm::event::{MouseEvent, MouseEventKind};
        Action::Mouse(MouseEvent {
            kind: if down {
                MouseEventKind::ScrollDown
            } else {
                MouseEventKind::ScrollUp
            },
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn click(column: u16, row: u16) -> Action {
        use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn mouse_state(ids: &[&str]) -> AppState {
        let mut s = loaded(ids);
        s.options.mouse = true;
        s
    }

    #[test]
    fn the_wheel_moves_the_list_and_scrolls_the_preview_by_three() {
        let ids: Vec<String> = (0..30).map(|i| format!("e{i:02}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut s = mouse_state(&refs);
        let now = Instant::now();
        s.update(
            Action::PreviewReady(s.preview_gen, Box::new(long_model("e00", 100))),
            now,
        );
        s.update(wheel(true, 10, 10), now);
        assert_eq!(s.selected, 3);
        s.update(wheel(false, 10, 10), now);
        s.update(wheel(false, 10, 10), now);
        assert_eq!(s.selected, 0);
        s.update(wheel(true, 100, 10), now);
        assert_eq!(
            (s.preview_scroll, s.selected),
            (3, 0),
            "over the preview it scrolls the preview"
        );
        s.update(wheel(true, 10, 0), now);
        assert_eq!(s.selected, 0, "the header is neither");
    }

    #[test]
    fn a_click_selects_the_row_under_it_and_nothing_else_is_handled() {
        let mut s = mouse_state(&["a", "b", "c", "d"]);
        let now = Instant::now();
        // the list box starts at row 3; its first row is row 4 and every result takes two rows
        let e = s.update(click(10, 8), now);
        assert_eq!(s.selected_id.as_deref(), Some("c"));
        assert!(
            e.iter()
                .any(|x| matches!(x, Effect::LoadPreview { id, .. } if id == "c"))
        );
        s.update(click(10, 5), now);
        assert_eq!(s.selected_id.as_deref(), Some("a"));
        s.update(click(10, 30), now);
        assert_eq!(s.selected_id.as_deref(), Some("a"), "below the last result");
        let right = ratatui::crossterm::event::MouseButton::Right;
        let before = s.selected;
        s.update(
            Action::Mouse(MouseEvent {
                kind: ratatui::crossterm::event::MouseEventKind::Down(right),
                column: 10,
                row: 8,
                modifiers: KeyModifiers::NONE,
            }),
            now,
        );
        assert_eq!(s.selected, before);
    }

    #[test]
    fn with_the_mouse_off_nothing_happens() {
        let mut s = loaded(&["a", "b", "c", "d"]);
        let now = Instant::now();
        assert!(
            s.update(click(10, 8), now).is_empty() && s.update(wheel(true, 10, 10), now).is_empty()
        );
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn a_query_given_on_the_command_line_starts_with_its_filters_as_chips() {
        let mut s = AppState::new(
            Options::default(),
            (120, 40),
            "type:cw tag:prod latency",
            "~/kb".into(),
        );
        assert_eq!(type_chips(&s), vec!["cw"]);
        assert_eq!(s.input.chips.len(), 2);
        assert_eq!(s.input.editor.text(), "latency");
        assert_eq!(s.query_string(), "type:cw tag:prod latency");
        // Tab then leaves exactly one type filter
        let e = s.update(key(KeyCode::Tab, KeyModifiers::NONE), Instant::now());
        assert_eq!(type_chips(&s), vec!["cmd"]);
        assert_eq!(search_of(&e).unwrap().1, "type:cmd tag:prod latency");
    }
}
