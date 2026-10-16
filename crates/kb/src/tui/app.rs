//! The state of the UI and how it changes. `update` is a pure function of the state, the action
//! and the time; everything impure (terminal, threads, clock) lives in `tui/mod.rs`.

use std::time::{Duration, Instant, SystemTime};

use crate::platform::clipboard::{ClipboardSettings, ReadOutcome};
use kb_core::author::{Draft, DraftOptions, DraftSource};
use kb_core::config::{PathTemplate, ThemeChoice};
use kb_core::entry::EntryType;
use kb_core::search::{Notice, SearchResult};
use ratatui::crossterm::event::{KeyEvent, MouseEvent};

use super::add::{AddEvent, AddState};
use super::form::{FormEvent, FormSpec, FormState, Submission, Then};
use super::input::{Change, LineEditor, SearchInput};
use super::keys::{self, Command, Key, Mode};
use super::layout::{self, Screen};
use super::preview::PreviewModel;
use super::run_modal::RunModalState;
use kb_core::keys::{Context, EffectiveKeymap, Translated};

/// Results asked for at first; doubled when the selection nears the end.
pub const PAGE: usize = 60;
const NEAR_END: usize = 10;
const MESSAGE_FOR: Duration = Duration::from_secs(2);
const REFRESH_EVERY: Duration = Duration::from_secs(1);
const SLOW_REFRESH: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
pub struct Options {
    pub mouse: bool,
    pub theme: ThemeChoice,
    /// How Ctrl-Y copies (the clipboard settings).
    pub clipboard: ClipboardSettings,
    /// The `editor` setting; `None` falls back to `$VISUAL`, `$EDITOR`, `vi`.
    pub editor: Option<String>,
    /// Global loaded settings.
    pub settings: kb_core::config::Settings,
    /// Turned off in timing tests so the refresh timer does not add noise.
    pub refresh_timer: bool,
    /// Problems found in the config file or environment, shown as a warning in the header.
    pub config_problems: usize,
    /// Enter and Ctrl-Y open the variable form for entries that have variables. Off when what
    /// happens next does not need the payload (`--print id`, `kb show`).
    pub fill_variables: bool,
    /// The form remembers the values it was used with (`vars.remember` and `--no-remember`).
    pub remember_vars: bool,
    /// Where the add form puts new entries (`path_template`).
    pub path_template: PathTemplate,
    /// Color the body in the add form (false for `NO_COLOR`).
    pub highlight: bool,
    /// `kb add` opens the UI on the add form: closing it ends the UI.
    pub start_add: Option<Box<StartAdd>>,
    /// `kb runbook run`: open this runbook's runs at start (runbook id, start a new run).
    pub start_run: Option<(String, bool)>,
}

/// A one-line question on the key bar (Ctrl-R, Ctrl-X).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListPrompt {
    /// `Rename to: <id>`, filled in with the current ID.
    Rename { from: String, editor: LineEditor },
    /// `Delete <id>? (y/N)`.
    Delete { id: String },
    /// `New runbook title: <title>`.
    NewRunbook { editor: LineEditor },
}

impl ListPrompt {
    /// The key bar text, with a block where the cursor is.
    pub fn text(&self) -> String {
        match self {
            ListPrompt::Rename { editor, .. } => {
                let chars: Vec<char> = editor.text().chars().collect();
                let at = editor.cursor().min(chars.len());
                let head: String = chars[..at].iter().collect();
                let tail: String = chars[at..].iter().collect();
                format!("Rename to: {head}▌{tail}")
            }
            ListPrompt::Delete { id } => format!("Delete {id}? (y/N)"),
            ListPrompt::NewRunbook { editor } => {
                let chars: Vec<char> = editor.text().chars().collect();
                let at = editor.cursor().min(chars.len());
                let head: String = chars[..at].iter().collect();
                let tail: String = chars[at..].iter().collect();
                format!("New runbook title: {head}▌{tail}")
            }
        }
    }
}

/// The draft `kb add` hands to the UI, and why the clipboard could not be read, if it could not.
#[derive(Debug, Clone)]
pub struct StartAdd {
    pub draft: Draft,
    pub unavailable: Option<String>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            mouse: false,
            config_problems: 0,
            fill_variables: true,
            remember_vars: true,
            path_template: PathTemplate::default(),
            highlight: true,
            start_add: None,
            start_run: None,
            theme: ThemeChoice::Auto,
            clipboard: ClipboardSettings::default(),
            editor: None,
            settings: kb_core::config::Settings::default(),
            refresh_timer: true,
        }
    }
}

/// An active run, as the header and previews list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRun {
    pub runbook_id: String,
    pub name: String,
    pub progress: String,
}

#[derive(Debug, Clone, Default)]
pub struct Page {
    pub results: Vec<SearchResult>,
    pub total: usize,
    pub type_counts: [usize; EntryType::COUNT],
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

/// How a Ctrl-Y copy went, as the key bar says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyReport {
    pub message: String,
    /// The text was sent somewhere (otherwise the message is a warning).
    pub sent: bool,
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
    CopyDone(CopyReport),
    /// The editor returned; `Some` carries the first lint error of the file.
    EditorReturned(Option<String>),
    /// The form for an entry with variables was read (or could not be).
    FormLoaded(Result<Box<FormSpec>, String>, Then),
    /// The entry changed between opening the form and submitting it.
    FormChanged(Box<FormSpec>),
    /// The form was used (Ctrl-Y): it closes.
    FormDone,
    /// Ctrl-N read the clipboard.
    ClipboardRead {
        outcome: ReadOutcome,
        now: SystemTime,
    },
    /// Tags in use in the KB, most used first.
    TagCounts(Vec<(String, u32)>),
    /// A rename from the list was done.
    Renamed {
        from: String,
        to: String,
    },
    /// A delete from the list was done.
    Removed(String),
    /// A rename or delete from the list did not work.
    OpFailed(String),
    /// Show this entry: search for its title and select it.
    GoToEntry {
        id: String,
        title: String,
    },
    /// "Follow entry link" on a note with several links.
    OpenLinkPicker(Box<super::action_view::LinkPickerState>),
    /// Alt-S finished: the one-line summary, and whether it worked.
    SyncDone(String, bool),
    GitStatus(Box<kb_core::git::status::SyncStatus>),
    FetchFailed,
    HistoryLoaded(Result<Box<super::history_view::HistoryState>, String>),
    HistoryDiff(Result<String, String>),
    HistoryRestored(Result<String, String>),
    /// Ctrl-E in the add form returned: the new body (`None`: unchanged or cancelled) or why not.
    BodyEdited(Result<Option<String>, String>),
    /// The new entry was written.
    Saved(String),
    SaveFailed(String),
    /// Open the run confirmation modal.
    OpenRunModal {
        id: String,
        plan: kb_core::run::RunPlan,
    },
    /// Open the code view (Screen 3).
    OpenCodeView(Box<super::code_view::CodeViewState>),
    /// Open the action menu modal.
    OpenActionMenu(Box<super::action_view::ActionMenuState>),
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
        /// The columns the preview is drawn in now (0 when there is no preview pane).
        cols: usize,
    },
    Refresh,
    /// Alt-S: run `kb sync` in the foreground.
    Sync,
    /// Re-read the git status for the header badge.
    GitStatus,
    /// Alt-H: read the entry's history.
    LoadHistory(String),
    /// Enter in the history overlay.
    LoadHistoryDiff {
        id: String,
        rev: String,
    },
    /// `r` then `y` in the history overlay.
    RestoreRevision {
        id: String,
        rev: String,
    },
    /// `sync.fetch_on_start`: fetch in the background.
    Fetch,
    /// Enter on the selected entry: leave the UI and let the caller copy or print it.
    Pick,
    CopyPayload,
    /// Copy a note as plain text, the way it is drawn, without Markdown or styling.
    CopyRenderedText,
    /// "Follow entry link" in a note's action menu.
    FollowLink(String),
    /// Alt-C: copy the payload as it is in the file.
    CopyTemplate,
    /// The form passed validation: check the entry did not change, then use it.
    SubmitForm(Box<Submission>),
    /// Ctrl-O: open the selected entry's primary link or console.
    OpenEntry(String),
    /// Alt-A / F2: open the action menu modal.
    OpenActionMenu(String),
    /// Run a custom action.
    RunCustomAction {
        id: String,
        action_name: String,
    },
    /// Code view open target (web, vscode, idea).
    OpenCodeTarget {
        id: String,
        target: String,
    },
    CopyCodeLocation(String),
    CopyCodePermalink(String),
    RepinCodeEntry {
        id: String,
        report: Box<kb_core::code::DriftReport>,
    },
    FetchAndRecheckCode(String),
    /// Ctrl-G: run the selected cmd entry.
    RunEntry(String),
    ExecuteRun {
        id: String,
        plan: kb_core::run::RunPlan,
    },
    EditEntry(String),
    /// Ctrl-N: read the clipboard (and ask for the tags in use) to open the add form.
    ReadClipboard,
    LoadTags,
    /// Ctrl-E in the add form: edit this text in the editor.
    EditBody(String),
    /// Ctrl-S in the add form: write the entry.
    SaveDraft(Box<Draft>),
    /// `kb add`'s form saved this entry: leave the UI.
    Added(String),
    /// Ctrl-R confirmed: rename an entry.
    RenameEntry {
        from: String,
        to: String,
    },
    /// Ctrl-X confirmed: delete an entry.
    RemoveEntry {
        id: String,
    },
    /// Create a new runbook from marked entries.
    CreateRunbookFromMarks {
        title: String,
        entries: Vec<String>,
    },
    /// Run mode: open the runbook's runs (a picker, or a new run when `new`).
    StartRunbook {
        id: String,
        new: bool,
    },
    /// Run mode: a run picker choice.
    ResumeRun(String),
    NewRun(String),
    /// Run mode: write the run.
    SaveRun,
    /// Run mode: write the run, release it and go back to the list.
    CloseRun,
    /// Run mode: do the step's primary action, or copy it.
    RunStep {
        idx: usize,
        copy: bool,
    },
    /// Run mode: `edit-run-vars`.
    RunEditVars,
    Quit(i32),
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub root: String,
    /// The git badge's data; `None` until the first status arrives.
    pub sync_status: Option<kb_core::git::status::SyncStatus>,
    pub fetch_failed: bool,
    pub history: Option<Box<super::history_view::HistoryState>>,
    pub input: SearchInput,
    pub results: Vec<SearchResult>,
    pub total: usize,
    pub type_counts: [usize; EntryType::COUNT],
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
    /// The variable form, when it is open.
    pub form: Option<Box<FormState>>,
    /// Values typed in a form that was closed with Esc, by entry ID (this session only).
    pub form_session:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// The add form, when it is open.
    pub add: Option<Box<AddState>>,
    /// The run confirmation/pause modal, when it is open.
    pub run_modal: Option<Box<RunModalState>>,
    /// Screen 3: Code View, when it is open.
    pub code_view: Option<Box<super::code_view::CodeViewState>>,
    /// Action Menu modal, when it is open.
    pub action_menu: Option<Box<super::action_view::ActionMenuState>>,
    /// Run mode: the run screen, when a run is open.
    pub run: Option<Box<super::run_view::RunScreen>>,
    /// Run mode: every active run in the KB, for the header and runbook previews.
    pub active_runs: Vec<ActiveRun>,
    /// Run mode: the picker of a runbook's runs.
    pub run_picker: Option<Box<super::run_picker::RunPicker>>,
    /// Run mode: the step whose command waits for the run modal.
    pub run_exec: Option<usize>,
    /// The list of entries a note links to, when there is more than one.
    pub link_picker: Option<Box<super::action_view::LinkPickerState>>,
    /// Tags in use in the KB with their counts, for completion in the add form.
    pub tags: Vec<(String, u32)>,
    /// The UI was started by `kb add`: it ends with the form.
    pub started_by_add: bool,
    /// The question on the key bar, when Ctrl-R or Ctrl-X asked one.
    pub prompt: Option<ListPrompt>,
    /// Entries marked in the TUI list for runbook creation.
    pub marks: Vec<String>,
    /// The keys: the preset plus the user's overrides, built once at startup.
    pub keymap: std::sync::Arc<kb_core::keys::EffectiveKeymap>,
    /// Vim's normal mode: letters are commands (never true with the emacs keymap).
    pub normal: bool,
    /// The first key of a two-key sequence (`g`) and when it was pressed.
    pub pending: Option<(Key, Instant)>,
}

/// The key a view should see: the user's binding mapped to the view's own default key, or
/// nothing for a default key the user moved elsewhere.
fn translated(map: &EffectiveKeymap, ctx: Context, key: Key) -> Option<Key> {
    match map.translate(ctx, key) {
        Translated::Key(k) => Some(k),
        Translated::Swallow => None,
    }
}

/// How long the first key of a sequence waits for the second.
pub const SEQUENCE_TIMEOUT: Duration = Duration::from_secs(1);

impl AppState {
    pub fn new(options: Options, size: (u16, u16), initial_query: &str, root: String) -> AppState {
        let keymap = std::sync::Arc::new(kb_core::keys::EffectiveKeymap::build(
            options.settings.keymap(),
            options.settings.key_overrides(),
        ));
        AppState {
            keymap,
            normal: false,
            pending: None,
            root,
            sync_status: None,
            fetch_failed: false,
            history: None,
            input: SearchInput::from_text(initial_query),
            results: Vec::new(),
            total: 0,
            type_counts: [0; EntryType::COUNT],
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
            form: None,
            form_session: std::collections::BTreeMap::new(),
            add: None,
            run_modal: None,
            code_view: None,
            action_menu: None,
            run: None,
            run_picker: None,
            active_runs: Vec::new(),
            run_exec: None,
            link_picker: None,
            tags: Vec::new(),
            started_by_add: false,
            prompt: None,
            marks: Vec::new(),
        }
    }

    /// `kb add` starts on the form, with the list behind it only for the tags.
    pub fn open_start_add(&mut self, start: StartAdd, now: SystemTime) {
        self.started_by_add = true;
        self.open_add(start.draft, start.unavailable, now);
    }

    fn open_add(&mut self, draft: Draft, unavailable: Option<String>, now: SystemTime) {
        self.add = Some(Box::new(AddState::open(
            draft,
            self.tags.clone(),
            unavailable,
            now,
            self.options.highlight,
        )));
    }

    pub fn mode(&self) -> Mode {
        let area = ratatui::layout::Rect::new(0, 0, self.size.0, self.size.1);
        if layout::layout(area, false) == Screen::TooSmall {
            Mode::TooSmall
        } else if self.form.is_some()
            || self.add.is_some()
            || self.code_view.is_some()
            || self.run.is_some()
            || self.run_picker.is_some()
            || self.action_menu.is_some()
            || self.link_picker.is_some()
            || self.history.is_some()
        {
            Mode::Form
        } else if self.overlay == Overlay::Help {
            Mode::Help
        } else if self.layout_mode == LayoutMode::FullPreview {
            Mode::FullPreview
        } else if self.normal {
            Mode::Normal
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
        if self.started_by_add {
            vec![Effect::Refresh, Effect::LoadTags]
        } else {
            vec![Effect::Refresh]
        }
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

    pub(crate) fn say(&mut self, text: &str, kind: MessageKind, now: Instant, timed: bool) {
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
                    cols: super::view::preview_cols(self).unwrap_or(0),
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
                if self.add.is_some() {
                    return self.add_key(e);
                }
                if self.run_modal.is_some() {
                    return self.run_modal_key(e);
                }
                if self.run_picker.is_some() {
                    return self.run_picker_key(e);
                }
                if self.action_menu.is_some() {
                    return self.action_menu_key(e, now);
                }
                if self.link_picker.is_some() {
                    return self.link_picker_key(e, now);
                }
                if let Some(h) = self.history.as_mut() {
                    use super::history_view::HistoryOutcome as O;
                    let id = h.history.id.clone();
                    let e = if h.takes_raw_keys() {
                        e
                    } else {
                        match keys::from_event(e)
                            .and_then(|k| translated(&self.keymap, Context::History, k))
                        {
                            Some(k) => keys::to_event(k),
                            None => return Vec::new(),
                        }
                    };
                    return match h.key(e) {
                        O::Nothing => Vec::new(),
                        O::Close => {
                            self.history = None;
                            Vec::new()
                        }
                        O::Diff(rev) => vec![Effect::LoadHistoryDiff { id, rev }],
                        O::Restore(rev) => vec![Effect::RestoreRevision { id, rev }],
                    };
                }
                if self.prompt.is_some() {
                    return self.prompt_key(e, now);
                }
                if self.form.is_some() {
                    return self.form_key(e);
                }
                if self.code_view.is_some() {
                    return self.code_view_key(e, now);
                }
                if self.run.is_some() {
                    return self.run_key(e);
                }
                let Some(key) = keys::from_event(e) else {
                    return Vec::new();
                };
                self.list_key(key, now)
            }
            Action::Paste(text) => {
                if let Some(add) = &mut self.add {
                    add.paste(&text);
                    return Vec::new();
                }
                if let Some(form) = &mut self.form {
                    form.paste(&text);
                    return Vec::new();
                }
                if self.mode() != Mode::Search {
                    return Vec::new();
                }
                self.pending = None;
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
                    .pending
                    .is_some_and(|(_, at)| now.duration_since(at) > SEQUENCE_TIMEOUT)
                {
                    self.pending = None;
                }
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
                    self.mark_runs();
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
                let mut effects = vec![self.search_effect(limit)];
                if self.sync_status.as_ref().is_some_and(|s| s.git.is_git) {
                    effects.push(Effect::GitStatus);
                }
                effects
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
            Action::CopyDone(report) => {
                let kind = if report.sent {
                    MessageKind::Info
                } else {
                    MessageKind::Warning
                };
                self.say(&report.message, kind, now, true);
                Vec::new()
            }
            Action::FormLoaded(result, then) => {
                match result {
                    Ok(spec) => {
                        let typed = self.form_session.get(&spec.id).cloned();
                        self.form =
                            Some(Box::new(FormState::open(*spec, then, typed.as_ref(), None)));
                    }
                    Err(e) => self.say(
                        &format!("Cannot open the variables: {e}"),
                        MessageKind::Warning,
                        now,
                        false,
                    ),
                }
                Vec::new()
            }
            Action::FormChanged(spec) => {
                if let Some(form) = &mut self.form {
                    form.reopen(*spec, "The entry changed; review the fields");
                }
                Vec::new()
            }
            Action::FormDone => {
                self.form = None;
                Vec::new()
            }
            Action::ClipboardRead { outcome, now: at } => {
                let opts = DraftOptions {
                    path_template: self.options.path_template.clone(),
                    ..DraftOptions::default()
                };
                let (draft, unavailable) = match outcome {
                    ReadOutcome::Text(text) => {
                        (Draft::from_text(&text, DraftSource::Clipboard, &opts), None)
                    }
                    ReadOutcome::Empty => (Draft::empty(DraftSource::Empty, None, &opts), None),
                    ReadOutcome::Unavailable(why) => (
                        Draft::empty(DraftSource::Empty, None, &opts),
                        Some(why.explain()),
                    ),
                };
                self.open_add(draft, unavailable, at);
                Vec::new()
            }
            Action::TagCounts(tags) => {
                if let Some(add) = &mut self.add {
                    add.known_tags.clone_from(&tags);
                }
                self.tags = tags;
                Vec::new()
            }
            Action::BodyEdited(result) => {
                if let Some(add) = &mut self.add {
                    match result {
                        Ok(new) => add.body_edited(new),
                        Err(message) => add.message = Some(message),
                    }
                }
                Vec::new()
            }
            Action::Saved(id) => {
                let title = self
                    .add
                    .take()
                    .map(|a| a.draft.title.clone())
                    .unwrap_or_default();
                if self.started_by_add {
                    return vec![Effect::Added(id)];
                }
                // show the new entry: the search line holds its title, so it is found whatever
                // the list showed before, and it is the selection
                self.input = SearchInput::new(&title);
                self.selected_id = Some(id.clone());
                self.say(&format!("Saved {id}"), MessageKind::Info, now, true);
                self.status.refreshing = true;
                vec![Effect::Refresh]
            }
            Action::Renamed { from: _, to } => {
                self.prompt = None;
                self.selected_id = Some(to.clone());
                self.say(&format!("Renamed to {to}"), MessageKind::Info, now, true);
                self.status.refreshing = true;
                vec![Effect::Refresh]
            }
            Action::Removed(id) => {
                self.prompt = None;
                // the next row takes its place; at the end, the previous one
                let next = self
                    .results
                    .get(self.selected + 1)
                    .or_else(|| {
                        self.selected
                            .checked_sub(1)
                            .and_then(|i| self.results.get(i))
                    })
                    .map(|r| r.id.clone());
                self.selected_id = next;
                self.say(&format!("Deleted {id}"), MessageKind::Info, now, true);
                self.status.refreshing = true;
                vec![Effect::Refresh]
            }
            Action::SyncDone(message, ok) => {
                let kind = if ok {
                    MessageKind::Info
                } else {
                    MessageKind::Warning
                };
                self.say(&message, kind, now, ok);
                self.fetch_failed = false;
                self.status.refreshing = true;
                vec![Effect::Refresh, Effect::GitStatus]
            }
            Action::GitStatus(status) => {
                self.sync_status = Some(*status);
                Vec::new()
            }
            Action::HistoryLoaded(Ok(h)) => {
                self.history = Some(h);
                Vec::new()
            }
            Action::HistoryLoaded(Err(m)) => {
                self.say(&m, MessageKind::Warning, now, false);
                Vec::new()
            }
            Action::HistoryDiff(result) => {
                if let Some(h) = &mut self.history {
                    match result {
                        Ok(d) => h.diff = Some((d, 0)),
                        Err(m) => h.message = Some(m),
                    }
                }
                Vec::new()
            }
            Action::HistoryRestored(Ok(message)) => {
                self.history = None;
                self.say(&message, MessageKind::Info, now, true);
                self.status.refreshing = true;
                vec![Effect::Refresh, Effect::GitStatus]
            }
            Action::HistoryRestored(Err(m)) => {
                if let Some(h) = &mut self.history {
                    h.message = Some(m);
                }
                Vec::new()
            }
            Action::FetchFailed => {
                self.fetch_failed = true;
                Vec::new()
            }
            Action::OpFailed(message) => {
                // a rename keeps its question open so the name can be fixed
                if !matches!(self.prompt, Some(ListPrompt::Rename { .. })) {
                    self.prompt = None;
                }
                self.say(&message, MessageKind::Warning, now, false);
                Vec::new()
            }
            Action::SaveFailed(message) => {
                if let Some(add) = &mut self.add {
                    add.save_failed(message);
                }
                Vec::new()
            }
            Action::OpenRunModal { id, plan } => {
                self.run_modal = Some(Box::new(RunModalState::Confirming {
                    id,
                    plan,
                    input: String::new(),
                    error: None,
                }));
                Vec::new()
            }
            Action::OpenCodeView(cv) => {
                self.code_view = Some(cv);
                Vec::new()
            }
            Action::OpenActionMenu(menu) => {
                self.action_menu = Some(menu);
                Vec::new()
            }
            Action::OpenLinkPicker(picker) => {
                self.link_picker = Some(picker);
                Vec::new()
            }
            Action::GoToEntry { id, title } => self.go_to(id, &title),
            Action::EditorReturned(error) => {
                self.status.refreshing = true;
                if let Some(text) = error {
                    self.say(&text, MessageKind::Warning, now, false);
                }
                vec![Effect::Refresh]
            }
        }
    }

    /// A key while Ctrl-R or Ctrl-X is asking.
    fn prompt_key(&mut self, e: KeyEvent, _now: Instant) -> Vec<Effect> {
        let Some(key) = keys::from_event(e) else {
            return Vec::new();
        };
        if key == Key::new(keys::Code::Char('c'), keys::CTRL) {
            return vec![Effect::Quit(130)];
        }
        self.message = None;
        let Some(prompt) = self.prompt.as_mut() else {
            return Vec::new();
        };
        match prompt {
            ListPrompt::Delete { id } => {
                let confirmed = matches!(key.code, keys::Code::Char('y' | 'Y')) && key.mods == 0;
                let id = id.clone();
                if confirmed {
                    vec![Effect::RemoveEntry { id }]
                } else {
                    self.prompt = None;
                    Vec::new()
                }
            }
            ListPrompt::Rename { from, editor } => match (key.code, key.mods) {
                (keys::Code::Esc, 0) => {
                    self.prompt = None;
                    Vec::new()
                }
                (keys::Code::Enter, 0) => {
                    let to = editor.text().trim().to_string();
                    let from = from.clone();
                    if to.is_empty() || to == from {
                        self.prompt = None;
                        Vec::new()
                    } else {
                        vec![Effect::RenameEntry { from, to }]
                    }
                }
                _ => {
                    super::add::line_key(editor, key);
                    Vec::new()
                }
            },
            ListPrompt::NewRunbook { editor } => match (key.code, key.mods) {
                (keys::Code::Esc, 0) => {
                    self.prompt = None;
                    Vec::new()
                }
                (keys::Code::Enter, 0) => {
                    let title = editor.text().trim().to_string();
                    self.prompt = None;
                    if title.is_empty() {
                        Vec::new()
                    } else {
                        let entries = std::mem::take(&mut self.marks);
                        vec![Effect::CreateRunbookFromMarks { title, entries }]
                    }
                }
                _ => {
                    super::add::line_key(editor, key);
                    Vec::new()
                }
            },
        }
    }

    /// A key while the add form is open.
    fn add_key(&mut self, e: KeyEvent) -> Vec<Effect> {
        let Some(key) = keys::from_event(e) else {
            return Vec::new();
        };
        if key == Key::new(keys::Code::Char('c'), keys::CTRL) {
            return vec![Effect::Quit(130)];
        }
        let Some(add) = self.add.as_mut() else {
            return Vec::new();
        };
        let key = if add.takes_raw_keys() {
            key
        } else {
            let Some(k) = translated(&self.keymap, Context::Add, key) else {
                return Vec::new();
            };
            k
        };
        match add.handle_key(key) {
            AddEvent::None => Vec::new(),
            AddEvent::Close => {
                self.add = None;
                if self.started_by_add {
                    vec![Effect::Quit(130)]
                } else {
                    Vec::new()
                }
            }
            AddEvent::Save(draft) => vec![Effect::SaveDraft(draft)],
            AddEvent::EditBody(text) => vec![Effect::EditBody(text)],
        }
    }

    /// A key while the variable form is open: the form handles it, and may ask for something.
    fn form_key(&mut self, e: KeyEvent) -> Vec<Effect> {
        let Some(key) = keys::from_event(e) else {
            return Vec::new();
        };
        if key == Key::new(keys::Code::Char('c'), keys::CTRL) {
            return vec![Effect::Quit(130)];
        }
        let Some(key) = translated(&self.keymap, Context::Form, key) else {
            return Vec::new();
        };
        let Some(form) = self.form.as_mut() else {
            return Vec::new();
        };
        match form.handle_key(key) {
            FormEvent::None => Vec::new(),
            FormEvent::Close => {
                let kept = form.session_values();
                let id = form.id.clone();
                if form.then == Then::StepVars
                    && let Some(r) = self.run.as_mut()
                {
                    r.awaiting_vars = None;
                }
                self.form = None;
                if kept.is_empty() {
                    self.form_session.remove(&id);
                } else {
                    self.form_session.insert(id, kept);
                }
                Vec::new()
            }
            FormEvent::Submit { print } => {
                vec![Effect::SubmitForm(Box::new(form.submission(print)))]
            }
        }
    }

    /// A key while the run confirmation / pause modal is open.
    fn run_modal_key(&mut self, e: KeyEvent) -> Vec<Effect> {
        let Some(key) = keys::from_event(e) else {
            return Vec::new();
        };
        if key == Key::new(keys::Code::Char('c'), keys::CTRL) {
            return vec![Effect::Quit(130)];
        }
        let Some(modal) = self.run_modal.as_mut() else {
            return Vec::new();
        };
        match modal.as_mut() {
            RunModalState::Paused { .. } => {
                self.run_modal = None;
                Vec::new()
            }
            RunModalState::Confirming {
                id,
                plan,
                input,
                error,
            } => {
                if key == Key::new(keys::Code::Esc, 0) {
                    self.run_modal = None;
                    self.run_exec = None;
                    return Vec::new();
                }
                if plan.requires_typed_run {
                    match key.code {
                        keys::Code::Char(c) if key.mods == 0 => {
                            input.push(c);
                            *error = None;
                            Vec::new()
                        }
                        keys::Code::Backspace => {
                            input.pop();
                            *error = None;
                            Vec::new()
                        }
                        keys::Code::Enter => {
                            if input.trim() == "run" {
                                let id = id.clone();
                                let plan = plan.clone();
                                vec![Effect::ExecuteRun { id, plan }]
                            } else {
                                *error = Some("Type 'run' to execute".to_string());
                                Vec::new()
                            }
                        }
                        _ => Vec::new(),
                    }
                } else {
                    match key.code {
                        keys::Code::Enter => {
                            let id = id.clone();
                            let plan = plan.clone();
                            vec![Effect::ExecuteRun { id, plan }]
                        }
                        keys::Code::Char('y' | 'Y') if key.mods == 0 => {
                            let id = id.clone();
                            let plan = plan.clone();
                            vec![Effect::ExecuteRun { id, plan }]
                        }
                        keys::Code::Char('n' | 'N') if key.mods == 0 => {
                            self.run_modal = None;
                            Vec::new()
                        }
                        _ => Vec::new(),
                    }
                }
            }
        }
    }

    /// Replaces the active runs shown in the header and the selected runbook's preview.
    pub fn set_active_runs(&mut self, runs: Vec<ActiveRun>) {
        self.active_runs = runs;
        self.mark_runs();
    }

    fn mark_runs(&mut self) {
        if let Some(p) = self.preview.as_mut() {
            p.runs = self
                .active_runs
                .iter()
                .filter(|r| r.runbook_id == p.id)
                .map(|r| format!("{} · {}", r.name, r.progress))
                .collect();
        }
    }

    /// A key while the run screen is open (spec 015).
    fn run_key(&mut self, e: KeyEvent) -> Vec<Effect> {
        let Some(key) = keys::from_event(e) else {
            return Vec::new();
        };
        if key == Key::new(keys::Code::Char('c'), keys::CTRL) {
            return vec![Effect::Quit(130)];
        }
        let Some(screen) = self.run.as_mut() else {
            return Vec::new();
        };
        let ev = screen.key(key, &self.keymap);
        self.run_event(ev)
    }

    /// What the run screen asked for, as effects.
    pub fn run_event(&mut self, ev: super::run_view::RunEvent) -> Vec<Effect> {
        use super::run_view::RunEvent as E;
        match ev {
            E::None => Vec::new(),
            E::Close | E::SaveAndClose => vec![Effect::CloseRun],
            E::Save => vec![Effect::SaveRun],
            E::Primary(idx) => vec![Effect::RunStep { idx, copy: false }],
            E::Copy(idx) => vec![Effect::RunStep { idx, copy: true }],
            E::EditVars => vec![Effect::RunEditVars],
        }
    }

    /// A key while the run picker is open.
    fn run_picker_key(&mut self, e: KeyEvent) -> Vec<Effect> {
        use super::run_picker::PickerEvent as P;
        let Some(key) = keys::from_event(e) else {
            return Vec::new();
        };
        if key == Key::new(keys::Code::Char('c'), keys::CTRL) {
            return vec![Effect::Quit(130)];
        }
        let Some(p) = self.run_picker.as_mut() else {
            return Vec::new();
        };
        let id = p.runbook_id.clone();
        match p.key(key) {
            P::None => Vec::new(),
            P::Close => {
                self.run_picker = None;
                Vec::new()
            }
            P::Resume(run_id) => {
                self.run_picker = None;
                vec![Effect::ResumeRun(run_id)]
            }
            P::New => {
                self.run_picker = None;
                vec![Effect::NewRun(id)]
            }
        }
    }

    /// A key while Screen 3 (Code View) is open.
    fn code_view_key(&mut self, e: KeyEvent, now: Instant) -> Vec<Effect> {
        let Some(key) = keys::from_event(e) else {
            return Vec::new();
        };
        if key == Key::new(keys::Code::Char('c'), keys::CTRL) {
            return vec![Effect::Quit(130)];
        }
        let Some(cv) = self.code_view.as_mut() else {
            return Vec::new();
        };
        let key = if cv.takes_raw_keys() {
            key
        } else {
            let Some(k) = translated(&self.keymap, Context::CodeView, key) else {
                return Vec::new();
            };
            k
        };
        match cv.handle_key(key) {
            super::code_view::CodeViewEvent::None => Vec::new(),
            super::code_view::CodeViewEvent::Close => {
                self.code_view = None;
                Vec::new()
            }
            super::code_view::CodeViewEvent::OpenDefault => {
                let id = cv.id.clone();
                let target = cv.default_open.clone();
                vec![Effect::OpenCodeTarget { id, target }]
            }
            super::code_view::CodeViewEvent::OpenWeb => {
                let id = cv.id.clone();
                vec![Effect::OpenCodeTarget {
                    id,
                    target: "web".to_string(),
                }]
            }
            super::code_view::CodeViewEvent::OpenVsCode => {
                let id = cv.id.clone();
                vec![Effect::OpenCodeTarget {
                    id,
                    target: "vscode".to_string(),
                }]
            }
            super::code_view::CodeViewEvent::OpenIdea => {
                let id = cv.id.clone();
                vec![Effect::OpenCodeTarget {
                    id,
                    target: "idea".to_string(),
                }]
            }
            super::code_view::CodeViewEvent::CopyLocation => {
                let loc = if let Some(el) = cv.end_line {
                    format!("{}:{}-{}", cv.path, cv.line, el)
                } else {
                    format!("{}:{}", cv.path, cv.line)
                };
                vec![Effect::CopyCodeLocation(loc)]
            }
            super::code_view::CodeViewEvent::CopyPermalink => {
                let permalink = cv.permalink.clone().unwrap_or_default();
                vec![Effect::CopyCodePermalink(permalink)]
            }
            super::code_view::CodeViewEvent::RepinPrompt => {
                self.say("Press 'y' to confirm re-pin", MessageKind::Info, now, true);
                Vec::new()
            }
            super::code_view::CodeViewEvent::RepinConfirm(report) => {
                let id = cv.id.clone();
                vec![Effect::RepinCodeEntry { id, report }]
            }
            super::code_view::CodeViewEvent::FetchAndRecheck => {
                let id = cv.id.clone();
                vec![Effect::FetchAndRecheckCode(id)]
            }
            super::code_view::CodeViewEvent::EditEntry => {
                let id = cv.id.clone();
                vec![Effect::EditEntry(id)]
            }
        }
    }

    /// A key while Action Menu modal is open.
    fn action_menu_key(&mut self, e: KeyEvent, now: Instant) -> Vec<Effect> {
        let Some(key) = keys::from_event(e) else {
            return Vec::new();
        };
        if key == Key::new(keys::Code::Char('c'), keys::CTRL) {
            return vec![Effect::Quit(130)];
        }
        let Some(key) = translated(&self.keymap, Context::ActionMenu, key) else {
            return Vec::new();
        };
        if key.code == keys::Code::Esc {
            self.action_menu = None;
            return Vec::new();
        }
        if let keys::Code::Char(c) = key.code {
            if key.mods & (keys::CTRL | keys::ALT) != 0 {
                return Vec::new();
            }
            let Some(menu) = self.action_menu.as_ref() else {
                return Vec::new();
            };
            if let Some(action) = menu.actions.iter().find(|a| a.key == c) {
                if !action.is_available(menu.has_local_clone, menu.is_ssh_without_display) {
                    self.say("Action is disabled", MessageKind::Warning, now, true);
                    return Vec::new();
                }
                let id = menu.id.clone();
                let kind = action.kind;
                let action_id = action.id.clone();
                self.action_menu = None;
                return match kind {
                    kb_core::action::ActionKind::CopyPayload => vec![Effect::CopyPayload],
                    kb_core::action::ActionKind::CopyRenderedText => vec![Effect::CopyRenderedText],
                    kb_core::action::ActionKind::EditEntry => vec![Effect::EditEntry(id)],
                    kb_core::action::ActionKind::FollowLink => vec![Effect::FollowLink(id)],
                    kb_core::action::ActionKind::OpenUrl => vec![Effect::OpenEntry(id)],
                    kb_core::action::ActionKind::RunCommand => vec![Effect::RunEntry(id)],
                    kb_core::action::ActionKind::CodeView => vec![Effect::OpenEntry(id)],
                    kb_core::action::ActionKind::CopyCodeLocation => {
                        vec![Effect::CopyCodeLocation(id)]
                    }
                    kb_core::action::ActionKind::CopyWebPermalink => {
                        vec![Effect::CopyCodePermalink(id)]
                    }
                    kb_core::action::ActionKind::OpenEditor => {
                        if action_id == "open-vscode" {
                            vec![Effect::OpenCodeTarget {
                                id,
                                target: "vscode".into(),
                            }]
                        } else if action_id == "open-idea" {
                            vec![Effect::OpenCodeTarget {
                                id,
                                target: "idea".into(),
                            }]
                        } else {
                            vec![Effect::OpenEntry(id)]
                        }
                    }
                    kb_core::action::ActionKind::RepinCode => vec![Effect::OpenEntry(id)],
                    kb_core::action::ActionKind::FetchAndCheck => vec![Effect::OpenEntry(id)],
                    kb_core::action::ActionKind::CustomCommand => {
                        vec![Effect::RunCustomAction {
                            id,
                            action_name: action_id,
                        }]
                    }
                };
            }
        }
        Vec::new()
    }

    /// Show `id`: the search becomes its title (an exact title is ranked first) and `id` is the
    /// entry that gets selected when the results arrive.
    fn go_to(&mut self, id: String, title: &str) -> Vec<Effect> {
        self.input.clear_all();
        for c in title.chars().filter(|c| !c.is_control()) {
            self.input.insert(c);
        }
        self.normal = false;
        let effects = self.query_changed();
        self.selected_id = Some(id);
        effects
    }

    /// A key while the list of linked entries is open.
    fn link_picker_key(&mut self, e: KeyEvent, now: Instant) -> Vec<Effect> {
        let Some(key) = keys::from_event(e) else {
            return Vec::new();
        };
        if key == Key::new(keys::Code::Char('c'), keys::CTRL) {
            return vec![Effect::Quit(130)];
        }
        let Some(picker) = self.link_picker.as_mut() else {
            return Vec::new();
        };
        let last = picker.choices.len().saturating_sub(1);
        match (key.code, key.mods) {
            (keys::Code::Esc, 0) => self.link_picker = None,
            (keys::Code::Down, 0) | (keys::Code::Char('j'), 0) => {
                picker.selected = (picker.selected + 1).min(last);
            }
            (keys::Code::Up, 0) | (keys::Code::Char('k'), 0) => {
                picker.selected = picker.selected.saturating_sub(1);
            }
            (keys::Code::Char(c @ '1'..='9'), 0) => {
                let n = usize::from(c as u8 - b'1');
                if n <= last {
                    picker.selected = n;
                }
            }
            (keys::Code::Enter, 0) => {
                let choice = picker.choices.get(picker.selected).cloned();
                self.link_picker = None;
                return match choice {
                    Some(c) => match c.target {
                        Some((_, title)) => self.go_to(c.id, &title),
                        None => {
                            self.say(
                                &format!("No entry named {}", c.id),
                                MessageKind::Warning,
                                now,
                                true,
                            );
                            Vec::new()
                        }
                    },
                    None => Vec::new(),
                };
            }
            _ => {}
        }
        Vec::new()
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

    /// A key on the main list: a pending `g` first, then the keymap.
    fn list_key(&mut self, key: Key, now: Instant) -> Vec<Effect> {
        let ctx = keys::list_context(self.normal);
        if let Some((first, at)) = self.pending.take()
            && now.duration_since(at) <= SEQUENCE_TIMEOUT
        {
            if key.code == keys::Code::Esc && key.mods == 0 {
                return Vec::new();
            }
            if let Some(cmd) = self.keymap.lookup_seq(ctx, first, key) {
                return keys::resolve_core(cmd, key)
                    .map_or_else(Vec::new, |c| self.command(c, now));
            }
            // anything else ends the sequence and counts as a key of its own
        }
        if self.normal
            && self.mode() == Mode::Normal
            && self.keymap.lookup(ctx, key).is_none()
            && self.keymap.starts_sequence(ctx, key)
        {
            self.pending = Some((key, now));
            return Vec::new();
        }
        match keys::resolve(&self.keymap, key, self.mode(), self.normal) {
            Some(command) => self.command(command, now),
            None => Vec::new(),
        }
    }

    /// Back to the query line (vim): from an empty one, or as it was.
    fn enter_search(&mut self, clear: bool) -> Vec<Effect> {
        self.normal = false;
        self.pending = None;
        if clear {
            self.input.clear_all();
            return self.query_changed();
        }
        self.input.editor.end();
        Vec::new()
    }

    fn command(&mut self, command: Command, now: Instant) -> Vec<Effect> {
        match command {
            Command::NormalMode => {
                if self.layout_mode == LayoutMode::FullPreview {
                    self.layout_mode = LayoutMode::Split;
                    self.ensure_visible();
                }
                self.normal = true;
                Vec::new()
            }
            Command::SearchClear => self.enter_search(true),
            Command::SearchKeep => self.enter_search(false),
            Command::HalfPageDown => {
                let rows = (self.list_rows() / 2).max(1);
                self.move_to(self.selected + rows)
            }
            Command::HalfPageUp => {
                let rows = (self.list_rows() / 2).max(1);
                self.move_to(self.selected.saturating_sub(rows))
            }
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
                Some(_) => vec![Effect::Pick],
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
            Command::CopyTemplate => {
                if self.selected_result().is_some() {
                    vec![Effect::CopyTemplate]
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
            Command::Open => match self.selected_result() {
                Some(r) => vec![Effect::OpenEntry(r.id.clone())],
                None => {
                    self.say("Nothing to pick", MessageKind::Info, now, true);
                    Vec::new()
                }
            },
            Command::Run => match self.selected_result() {
                Some(r) => vec![Effect::RunEntry(r.id.clone())],
                None => {
                    self.say("Nothing to pick", MessageKind::Info, now, true);
                    Vec::new()
                }
            },
            Command::ActionMenu => match self.selected_result() {
                Some(r) => {
                    if r.entry_type == kb_core::entry::EntryType::Code {
                        vec![Effect::OpenEntry(r.id.clone())]
                    } else {
                        vec![Effect::OpenActionMenu(r.id.clone())]
                    }
                }
                None => {
                    self.say("Nothing to pick", MessageKind::Info, now, true);
                    Vec::new()
                }
            },
            Command::AddEntry => vec![Effect::ReadClipboard],
            Command::RenameEntry | Command::DeleteEntry => {
                let Some(id) = self.selected_result().map(|r| r.id.clone()) else {
                    self.say("Nothing to pick", MessageKind::Info, now, true);
                    return Vec::new();
                };
                self.prompt = Some(if command == Command::RenameEntry {
                    ListPrompt::Rename {
                        editor: LineEditor::new(&id),
                        from: id,
                    }
                } else {
                    ListPrompt::Delete { id }
                });
                Vec::new()
            }
            Command::Refresh => {
                self.status.refreshing = true;
                vec![Effect::Refresh]
            }
            Command::History => {
                let Some(id) = self.selected_result().map(|r| r.id.clone()) else {
                    self.say("Nothing to pick", MessageKind::Info, now, true);
                    return Vec::new();
                };
                vec![Effect::LoadHistory(id)]
            }
            Command::Sync => {
                self.say("Syncing…", MessageKind::Info, now, true);
                vec![Effect::Sync]
            }
            Command::MarkEntry => {
                if let Some(r) = self.selected_result() {
                    let id = r.id.clone();
                    if let Some(pos) = self.marks.iter().position(|m| m == &id) {
                        self.marks.remove(pos);
                    } else {
                        self.marks.push(id);
                    }
                } else {
                    self.say("Nothing to mark", MessageKind::Info, now, true);
                }
                Vec::new()
            }
            Command::NewRunbookFromMarks => {
                if self.marks.is_empty() {
                    self.say(
                        "mark entries first with Alt-M (or v)",
                        MessageKind::Info,
                        now,
                        true,
                    );
                    Vec::new()
                } else {
                    self.prompt = Some(ListPrompt::NewRunbook {
                        editor: LineEditor::new(""),
                    });
                    Vec::new()
                }
            }
            Command::Esc => {
                if self.layout_mode == LayoutMode::FullPreview {
                    self.layout_mode = LayoutMode::Split;
                    self.ensure_visible();
                    Vec::new()
                } else if self.input.is_blank() {
                    if self.normal && !self.marks.is_empty() {
                        self.marks.clear();
                        Vec::new()
                    } else {
                        vec![Effect::Quit(130)]
                    }
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
        if !self.options.mouse
            || !matches!(self.mode(), Mode::Search | Mode::Normal | Mode::FullPreview)
        {
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
    use crate::tui::form::Then;
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
            unreviewed: false,
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
        p.type_counts = [3, 0, 0, 0, 0, 0, 0, 0, 0];
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
        s.update(k(KeyCode::Down, KeyModifiers::NONE), now);
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
    fn enter_only_picks_and_the_caller_records_the_use() {
        // the use is recorded after the copy or print succeeds (FR-008), not when Enter is pressed
        let mut s = loaded(&["a", "b"]);
        let e = s.update(key(KeyCode::Enter, KeyModifiers::NONE), Instant::now());
        assert_eq!(e, vec![Effect::Pick]);
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
        assert!(search_of(&e).is_some() && !e.iter().any(|x| matches!(x, Effect::Pick)));
    }

    #[test]
    fn copy_messages_expire() {
        let mut s = loaded(&["a"]);
        let t0 = Instant::now();
        assert_eq!(s.update(ctrl('y'), t0), vec![Effect::CopyPayload]);
        let report = |message: &str, sent| CopyReport {
            message: message.to_string(),
            sent,
        };
        s.update(Action::CopyDone(report("Copied via pbcopy", true)), t0);
        let m = s.message.as_ref().unwrap();
        assert_eq!(
            (m.text.as_str(), m.kind),
            ("Copied via pbcopy", MessageKind::Info)
        );
        s.update(Action::Tick, t0 + Duration::from_millis(2100));
        assert!(s.message.is_none());
        s.update(
            Action::CopyDone(report(
                "Too large for the terminal: 99 B, limit 10 B",
                false,
            )),
            t0,
        );
        let m = s.message.as_ref().unwrap();
        assert!(m.text.contains("Too large") && m.kind == MessageKind::Warning);
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
        let order = [
            "sql", "sumo", "cw", "cmd", "prompt", "link", "code", "note", "runbook",
        ];
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
        assert_eq!(type_chips(&s), vec!["runbook"]);
        s.update(key(KeyCode::BackTab, KeyModifiers::SHIFT), now);
        assert_eq!(type_chips(&s), vec!["note"]);
        s.update(key(KeyCode::BackTab, KeyModifiers::SHIFT), now);
        assert_eq!(type_chips(&s), vec!["code"]);
        for _ in 0..6 {
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
        p.type_counts = [4, 3, 2, 1, 0, 0, 0, 0, 0];
        let g = s.search_gen;
        s.update(Action::Results(g, p), now);
        assert_eq!(s.type_counts, [4, 3, 2, 1, 0, 0, 0, 0, 0]);
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
            "↑↓ select · Enter pick · ^Y copy · ^E edit · Alt-A actions · ? help · Esc quit"
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
            [Effect::Pick]
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

    // ---- the variable form ----

    fn form_spec() -> crate::tui::form::FormSpec {
        crate::tui::form::tests::spec_of(
            "vars:\n  env: { kind: enum, options: [prod, stage], default: stage }\n  n: { kind: number, default: 5 }\n",
            "go {{env}} {{n}}",
        )
    }

    fn with_form(then: Then) -> AppState {
        let mut s = loaded(&["a"]);
        s.update(
            Action::FormLoaded(Ok(Box::new(form_spec())), then),
            Instant::now(),
        );
        s
    }

    #[test]
    fn enter_on_a_result_asks_the_caller_to_pick_and_the_form_opens_when_it_is_loaded() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        assert_eq!(
            s.update(key(KeyCode::Enter, KeyModifiers::NONE), now),
            vec![Effect::Pick]
        );
        assert!(s.form.is_none());
        s.update(
            Action::FormLoaded(Ok(Box::new(form_spec())), Then::Exit),
            now,
        );
        assert_eq!(s.mode(), Mode::Form);
        assert!(s.form.is_some());
    }

    #[test]
    fn a_form_that_cannot_be_read_is_reported_not_opened() {
        let mut s = loaded(&["a"]);
        s.update(
            Action::FormLoaded(Err("a/b: not readable".into()), Then::Exit),
            Instant::now(),
        );
        assert!(s.form.is_none());
        let m = s.message.as_ref().unwrap();
        assert!(m.text.contains("Cannot open the variables") && m.kind == MessageKind::Warning);
    }

    #[test]
    fn keys_go_to_the_form_while_it_is_open() {
        let mut s = with_form(Then::Exit);
        let now = Instant::now();
        // `?` and letters are typed into the field, not the search line or help
        s.update(key(KeyCode::Tab, KeyModifiers::NONE), now);
        s.update(ch('?'), now);
        assert_eq!(s.overlay, Overlay::None);
        assert_eq!(s.input.editor.text(), "");
        assert_eq!(s.form.as_ref().unwrap().fields[1].editor.text(), "5?");
        // Ctrl-C still quits
        assert_eq!(s.update(ctrl('c'), now), vec![Effect::Quit(130)]);
    }

    #[test]
    fn enter_submits_a_valid_form_as_an_effect_and_blocks_an_invalid_one() {
        let mut s = with_form(Then::Exit);
        let now = Instant::now();
        let e = s.update(key(KeyCode::Enter, KeyModifiers::NONE), now);
        let Some(Effect::SubmitForm(sub)) = e.first() else {
            panic!("{e:?}")
        };
        assert_eq!(
            (sub.id.as_str(), sub.print, sub.then),
            ("x", false, Then::Exit)
        );
        s.update(key(KeyCode::Tab, KeyModifiers::NONE), now);
        s.update(ch('x'), now);
        assert!(
            s.update(key(KeyCode::Enter, KeyModifiers::NONE), now)
                .is_empty()
        );
        assert!(s.form.as_ref().unwrap().fields[1].error.is_some());
        let e = s.update(key(KeyCode::Backspace, KeyModifiers::NONE), now);
        assert!(e.is_empty());
        let e = s.update(key(KeyCode::Enter, KeyModifiers::ALT), now);
        let Some(Effect::SubmitForm(sub)) = e.first() else {
            panic!("{e:?}")
        };
        assert!(sub.print);
    }

    #[test]
    fn esc_returns_to_the_list_with_the_same_selection_and_keeps_typed_values() {
        let mut s = loaded(&["a", "b"]);
        let now = Instant::now();
        s.update(key(KeyCode::Down, KeyModifiers::NONE), now);
        s.update(
            Action::FormLoaded(Ok(Box::new(form_spec())), Then::Exit),
            now,
        );
        s.update(key(KeyCode::Tab, KeyModifiers::NONE), now);
        s.update(ch('0'), now);
        assert!(
            s.update(key(KeyCode::Esc, KeyModifiers::NONE), now)
                .is_empty()
        );
        assert!(s.form.is_none());
        assert_eq!(s.selected_id.as_deref(), Some("b"));
        assert_eq!(s.form_session["x"]["n"], "50");
        // opening it again restores what was typed
        s.update(
            Action::FormLoaded(Ok(Box::new(form_spec())), Then::Exit),
            now,
        );
        assert_eq!(s.form.as_ref().unwrap().fields[1].editor.text(), "50");
    }

    #[test]
    fn a_changed_entry_reopens_the_form_with_a_notice_and_a_done_copy_closes_it() {
        let mut s = with_form(Then::Stay);
        let now = Instant::now();
        s.update(key(KeyCode::Tab, KeyModifiers::NONE), now);
        s.update(ch('0'), now);
        s.update(Action::FormChanged(Box::new(form_spec())), now);
        let f = s.form.as_ref().unwrap();
        assert!(f.notice.as_deref().unwrap().contains("changed"));
        assert_eq!(f.fields[1].editor.text(), "50", "typed values survive");
        s.update(Action::FormDone, now);
        assert!(s.form.is_none());
    }

    #[test]
    fn alt_c_copies_the_template_and_ctrl_y_copies_the_payload() {
        let mut s = loaded(&["a"]);
        let now = Instant::now();
        assert_eq!(
            s.update(key(KeyCode::Char('c'), KeyModifiers::ALT), now),
            vec![Effect::CopyTemplate]
        );
        assert_eq!(s.update(ctrl('y'), now), vec![Effect::CopyPayload]);
        let mut empty = state();
        assert!(
            empty
                .update(key(KeyCode::Char('c'), KeyModifiers::ALT), now)
                .is_empty()
        );
    }

    #[test]
    fn pasted_text_goes_to_the_form_not_the_search_line() {
        let mut s = with_form(Then::Exit);
        let now = Instant::now();
        s.update(key(KeyCode::Tab, KeyModifiers::NONE), now);
        s.update(Action::Paste("1\n2".into()), now);
        assert_eq!(s.input.editor.text(), "");
        assert_eq!(s.form.as_ref().unwrap().fields[1].editor.text(), "51");
    }

    #[test]
    fn the_debug_output_of_the_whole_state_never_shows_a_secret() {
        let env = |n: &str| (n == "KB_TOK").then(|| "S3CR3T-MARKER".to_string());
        let spec = crate::tui::form::tests::spec_with(
            "vars:\n  t: { kind: secret, env: KB_TOK }\n",
            "curl {{t}}",
            &[],
            &env,
        );
        let mut s = loaded(&["a"]);
        s.update(
            Action::FormLoaded(Ok(Box::new(spec)), Then::Exit),
            Instant::now(),
        );
        assert!(!format!("{s:?}").contains("S3CR3T"));
        let e = s.update(key(KeyCode::Enter, KeyModifiers::NONE), Instant::now());
        assert!(!format!("{e:?}").contains("S3CR3T"));
    }

    fn clipboard(text: &str) -> Action {
        Action::ClipboardRead {
            outcome: ReadOutcome::Text(text.into()),
            now: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn ctrl_n_reads_the_clipboard_and_opens_the_add_form() {
        let mut s = loaded(&["a", "b"]);
        let now = Instant::now();
        let e = s.update(ctrl('n'), now);
        assert_eq!(e, [Effect::ReadClipboard]);
        assert!(s.add.is_none());
        s.update(clipboard("select 1 from t;"), now);
        let add = s.add.as_ref().unwrap();
        assert_eq!(add.draft.entry_type, EntryType::Sql);
        assert_eq!(s.mode(), Mode::Form);
        // the unreadable clipboard opens an empty form that says why
        let mut s = loaded(&["a"]);
        s.update(
            Action::ClipboardRead {
                outcome: ReadOutcome::Unavailable(crate::platform::clipboard::Unavailable::OverSsh),
                now: SystemTime::UNIX_EPOCH,
            },
            now,
        );
        assert!(s.add.as_ref().unwrap().header().contains("over SSH"));
    }

    #[test]
    fn saving_closes_the_form_and_selects_the_new_entry() {
        let mut s = loaded(&["a", "b"]);
        let now = Instant::now();
        s.update(clipboard("select 1 from t;"), now);
        for c in "My query".chars() {
            s.update(ch(c), now);
        }
        let e = s.update(ctrl('s'), now);
        let [Effect::SaveDraft(draft)] = e.as_slice() else {
            panic!("{e:?}")
        };
        assert_eq!(draft.title, "My query");
        // a failed save keeps the form open with the message
        s.update(
            Action::SaveFailed("sql/my-query already exists".into()),
            now,
        );
        assert!(s.add.is_some());
        assert_eq!(
            s.add.as_ref().unwrap().message.as_deref(),
            Some("sql/my-query already exists")
        );
        let e = s.update(Action::Saved("sql/my-query".into()), now);
        assert!(s.add.is_none());
        assert_eq!(s.selected_id.as_deref(), Some("sql/my-query"));
        assert_eq!(s.message.as_ref().unwrap().text, "Saved sql/my-query");
        assert!(e.contains(&Effect::Refresh));
        // the next results put the selection on it
        s.update(
            Action::Refreshed(RefreshInfo {
                indexed: 3,
                skipped: 0,
                duration: Duration::from_millis(5),
            }),
            now,
        );
        let g = s.search_gen;
        s.update(Action::Results(g, page(&["a", "sql/my-query", "b"])), now);
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn started_by_add_ends_the_ui_with_the_form() {
        let mut s = state();
        s.started_by_add = true;
        let now = Instant::now();
        s.update(clipboard("select 1 from t;"), now);
        assert_eq!(
            s.update(Action::Saved("sql/x".into()), now),
            [Effect::Added("sql/x".into())]
        );
        s.update(clipboard("select 1 from t;"), now);
        assert_eq!(
            s.update(key(KeyCode::Esc, KeyModifiers::NONE), now),
            [Effect::Quit(130)]
        );
    }

    #[test]
    fn ctrl_c_quits_from_the_add_form() {
        let mut s = state();
        let now = Instant::now();
        s.update(clipboard("select 1 from t;"), now);
        assert_eq!(s.update(ctrl('c'), now), [Effect::Quit(130)]);
    }
}
