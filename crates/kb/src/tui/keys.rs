//! The TUI side of the key map: terminal events become `kb_core::keys` keys, and the effective
//! keymap (preset plus config overrides) decides what they do. The key bar and the help overlay
//! are generated from the same keymap, so they always show the keys in use.

pub use kb_core::keys::{ALT, CTRL, Code, Key, SHIFT};
use kb_core::keys::{Command as Cmd, Context, EffectiveKeymap, Keymap, is_reserved};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// The key an event stands for. Shift is dropped for characters (typing `A` is `Char('A')`)
/// and for Shift-Tab, which terminals report as `BackTab`. Releases are ignored.
pub fn from_event(e: KeyEvent) -> Option<Key> {
    if e.kind == KeyEventKind::Release {
        return None;
    }
    let mut mods = 0;
    if e.modifiers.contains(KeyModifiers::SHIFT) {
        mods |= SHIFT;
    }
    if e.modifiers.contains(KeyModifiers::CONTROL) {
        mods |= CTRL;
    }
    if e.modifiers.contains(KeyModifiers::ALT) {
        mods |= ALT;
    }
    let code = match e.code {
        KeyCode::Char(c) => {
            mods &= !SHIFT;
            Code::Char(if mods & CTRL != 0 {
                c.to_ascii_lowercase()
            } else {
                c
            })
        }
        KeyCode::Enter => Code::Enter,
        KeyCode::Esc => Code::Esc,
        KeyCode::Tab => Code::Tab,
        KeyCode::BackTab => {
            mods &= !SHIFT;
            Code::BackTab
        }
        KeyCode::Backspace => Code::Backspace,
        KeyCode::Delete => Code::Delete,
        KeyCode::Left => Code::Left,
        KeyCode::Right => Code::Right,
        KeyCode::Up => Code::Up,
        KeyCode::Down => Code::Down,
        KeyCode::Home => Code::Home,
        KeyCode::End => Code::End,
        KeyCode::PageUp => Code::PageUp,
        KeyCode::PageDown => Code::PageDown,
        KeyCode::F(n) => Code::F(n),
        _ => return None,
    };
    Some(Key { code, mods })
}

/// The event for a key, for views that still take terminal events.
pub fn to_event(k: Key) -> KeyEvent {
    let mut mods = KeyModifiers::NONE;
    if k.mods & SHIFT != 0 {
        mods |= KeyModifiers::SHIFT;
    }
    if k.mods & CTRL != 0 {
        mods |= KeyModifiers::CONTROL;
    }
    if k.mods & ALT != 0 {
        mods |= KeyModifiers::ALT;
    }
    let code = match k.code {
        Code::Char(c) => KeyCode::Char(c),
        Code::Enter => KeyCode::Enter,
        Code::Esc => KeyCode::Esc,
        Code::Tab => KeyCode::Tab,
        Code::BackTab => KeyCode::BackTab,
        Code::Backspace => KeyCode::Backspace,
        Code::Delete => KeyCode::Delete,
        Code::Left => KeyCode::Left,
        Code::Right => KeyCode::Right,
        Code::Up => KeyCode::Up,
        Code::Down => KeyCode::Down,
        Code::Home => KeyCode::Home,
        Code::End => KeyCode::End,
        Code::PageUp => KeyCode::PageUp,
        Code::PageDown => KeyCode::PageDown,
        Code::F(n) => KeyCode::F(n),
    };
    KeyEvent::new(code, mods)
}

/// What the UI is showing, for key resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The main list, typing edits the query.
    Search,
    /// The main list in vim's normal mode: letters are commands.
    Normal,
    FullPreview,
    Help,
    TooSmall,
    /// A form or view is open; it handles its own keys.
    Form,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Pick,
    Copy,
    /// Open the link, console deep link, or code view.
    Open,
    /// Run the command safely with confirmation.
    Run,
    ActionMenu,
    /// Copy the payload with its `{{variables}}` left in.
    CopyTemplate,
    Edit,
    AddEntry,
    RenameEntry,
    DeleteEntry,
    Down,
    Up,
    PageDown,
    PageUp,
    HalfPageDown,
    HalfPageUp,
    Last,
    First,
    TypeNext,
    TypePrev,
    ScrollDown,
    ScrollUp,
    ScrollPageDown,
    ScrollPageUp,
    TogglePreview,
    Left,
    Right,
    LineStart,
    LineEnd,
    Backspace,
    Delete,
    DeleteWord,
    ClearLine,
    Refresh,
    Sync,
    History,
    MarkEntry,
    NewRunbookFromMarks,
    /// `?`: help when the text line is empty, otherwise the character is typed.
    HelpKey,
    Help,
    /// Esc: clear the query, close the preview, or exit when there is nothing to clear.
    Esc,
    Quit,
    /// Vim: leave the query for normal mode.
    NormalMode,
    /// Vim: back to the query, cleared.
    SearchClear,
    /// Vim: back to the query, as it was.
    SearchKeep,
    /// Any key while the help overlay is open.
    CloseHelp,
    /// A printable character.
    Insert(char),
}

fn from_core(c: Cmd, key: Key) -> Option<Command> {
    Some(match c {
        Cmd::Pick => Command::Pick,
        Cmd::Copy => Command::Copy,
        Cmd::CopyTemplate => Command::CopyTemplate,
        Cmd::Open => Command::Open,
        Cmd::Run => Command::Run,
        Cmd::ActionMenu => Command::ActionMenu,
        Cmd::Edit => Command::Edit,
        Cmd::AddEntry => Command::AddEntry,
        Cmd::RenameEntry => Command::RenameEntry,
        Cmd::DeleteEntry => Command::DeleteEntry,
        Cmd::Down => Command::Down,
        Cmd::Up => Command::Up,
        Cmd::PageDown => Command::PageDown,
        Cmd::PageUp => Command::PageUp,
        Cmd::HalfPageDown => Command::HalfPageDown,
        Cmd::HalfPageUp => Command::HalfPageUp,
        Cmd::First => Command::First,
        Cmd::Last => Command::Last,
        Cmd::TypeNext => Command::TypeNext,
        Cmd::TypePrev => Command::TypePrev,
        Cmd::ScrollPreviewDown => Command::ScrollDown,
        Cmd::ScrollPreviewUp => Command::ScrollUp,
        Cmd::ScrollPreviewPageDown => Command::ScrollPageDown,
        Cmd::ScrollPreviewPageUp => Command::ScrollPageUp,
        Cmd::TogglePreview => Command::TogglePreview,
        Cmd::Left => Command::Left,
        Cmd::Right => Command::Right,
        Cmd::LineStart => Command::LineStart,
        Cmd::LineEnd => Command::LineEnd,
        Cmd::Backspace => Command::Backspace,
        Cmd::Delete => Command::Delete,
        Cmd::DeleteWord => Command::DeleteWord,
        Cmd::ClearLine => Command::ClearLine,
        Cmd::NormalMode => Command::NormalMode,
        Cmd::SearchClear => Command::SearchClear,
        Cmd::SearchKeep => Command::SearchKeep,
        Cmd::Refresh => Command::Refresh,
        Cmd::Sync => Command::Sync,
        Cmd::History => Command::History,
        Cmd::MarkEntry => Command::MarkEntry,
        Cmd::NewRunbookFromMarks => Command::NewRunbookFromMarks,
        Cmd::Help if key.is_printable() => Command::HelpKey,
        Cmd::Help => Command::Help,
        Cmd::Cancel => Command::Esc,
        Cmd::Quit => Command::Quit,
        _ => return None,
    })
}

/// The commands that also work while the preview fills the screen.
fn works_in_full_preview(c: Cmd) -> bool {
    matches!(
        c,
        Cmd::Pick
            | Cmd::Copy
            | Cmd::Open
            | Cmd::Run
            | Cmd::ActionMenu
            | Cmd::CopyTemplate
            | Cmd::Edit
            | Cmd::ScrollPreviewDown
            | Cmd::ScrollPreviewUp
            | Cmd::ScrollPreviewPageDown
            | Cmd::ScrollPreviewPageUp
            | Cmd::History
            | Cmd::Sync
            | Cmd::TogglePreview
            | Cmd::Refresh
            | Cmd::Help
            | Cmd::Cancel
            | Cmd::Quit
            | Cmd::NormalMode
    )
}

/// The command for a core command pressed as `key` (for sequences, which the keymap resolves).
pub fn resolve_core(c: Cmd, key: Key) -> Option<Command> {
    from_core(c, key)
}

/// The context of the main list.
pub fn list_context(normal: bool) -> Context {
    if normal {
        Context::Normal
    } else {
        Context::Search
    }
}

/// What a key does in `mode`. `normal` is whether the list is in vim's normal mode (the full
/// preview and the too-small screen keep the keys of the mode they came from).
pub fn resolve(map: &EffectiveKeymap, key: Key, mode: Mode, normal: bool) -> Option<Command> {
    let normal = normal || mode == Mode::Normal;
    let ctx = list_context(normal);
    match mode {
        Mode::Help => {
            if is_reserved(key) {
                return Some(Command::Quit);
            }
            // by default every key closes it; a `close` binding narrows that
            let narrowed = !map.keys(Context::Help, Cmd::Close).is_empty();
            (!narrowed || map.lookup(Context::Help, key) == Some(Cmd::Close))
                .then_some(Command::CloseHelp)
        }
        Mode::TooSmall => (map.lookup(ctx, key) == Some(Cmd::Quit)).then_some(Command::Quit),
        Mode::Form => None,
        Mode::Search | Mode::Normal => {
            if let Some(c) = map.lookup(ctx, key) {
                return from_core(c, key);
            }
            match key.code {
                Code::Char(c) if mode == Mode::Search && key.is_printable() => {
                    Some(Command::Insert(c))
                }
                _ => None,
            }
        }
        Mode::FullPreview => {
            let c = map.lookup(ctx, key)?;
            // in the query a plain character is typing, not a command
            if !normal && key.is_printable() {
                return None;
            }
            works_in_full_preview(c)
                .then(|| from_core(c, key))
                .flatten()
        }
    }
}

// ---- the key bar and the help overlay ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Pick,
    Move,
    Filter,
    Preview,
    Line,
    App,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Group::Pick,
        Group::Move,
        Group::Filter,
        Group::Preview,
        Group::Line,
        Group::App,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Pick => "Pick",
            Group::Move => "Move",
            Group::Filter => "Filter",
            Group::Preview => "Preview",
            Group::Line => "Search line",
            Group::App => "App",
        }
    }
}

/// Which of a command's keys a help row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    All,
    Printable,
    Other,
}

/// One row of the help overlay; the keys come from the keymap.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    pub command: Cmd,
    pub which: Which,
    pub group: Group,
    pub label: &'static str,
}

const fn row(command: Cmd, group: Group, label: &'static str) -> Row {
    Row {
        command,
        which: Which::All,
        group,
        label,
    }
}

pub const ROWS: &[Row] = &[
    row(Cmd::Pick, Group::Pick, "pick: copy the entry and exit"),
    row(Cmd::Copy, Group::Pick, "copy the payload and stay open"),
    row(
        Cmd::Open,
        Group::Pick,
        "open the link, console or code view",
    ),
    row(Cmd::Run, Group::Pick, "run the command safely"),
    row(Cmd::ActionMenu, Group::Pick, "open action menu"),
    row(
        Cmd::CopyTemplate,
        Group::Pick,
        "copy the payload with its {{variables}} left in",
    ),
    row(
        Cmd::Edit,
        Group::Pick,
        "edit the entry in $VISUAL / $EDITOR",
    ),
    row(
        Cmd::AddEntry,
        Group::Pick,
        "add an entry from the clipboard",
    ),
    row(Cmd::RenameEntry, Group::Pick, "rename the entry"),
    row(Cmd::DeleteEntry, Group::Pick, "delete the entry"),
    row(Cmd::Down, Group::Move, "select the next result"),
    row(Cmd::Up, Group::Move, "select the previous result"),
    row(Cmd::PageDown, Group::Move, "down one page"),
    row(Cmd::PageUp, Group::Move, "up one page"),
    row(Cmd::HalfPageDown, Group::Move, "down half a page"),
    row(Cmd::HalfPageUp, Group::Move, "up half a page"),
    row(Cmd::Last, Group::Move, "last result"),
    row(Cmd::First, Group::Move, "first result"),
    row(Cmd::TypeNext, Group::Filter, "next type filter"),
    row(Cmd::TypePrev, Group::Filter, "previous type filter"),
    row(
        Cmd::ScrollPreviewDown,
        Group::Preview,
        "scroll the preview one line down",
    ),
    row(
        Cmd::ScrollPreviewUp,
        Group::Preview,
        "scroll the preview one line up",
    ),
    row(
        Cmd::ScrollPreviewPageDown,
        Group::Preview,
        "scroll the preview one page down",
    ),
    row(
        Cmd::ScrollPreviewPageUp,
        Group::Preview,
        "scroll the preview one page up",
    ),
    row(Cmd::History, Group::App, "history of the selected entry"),
    row(Cmd::Sync, Group::App, "sync with the git remote"),
    row(
        Cmd::TogglePreview,
        Group::Preview,
        "full-screen preview (narrow terminals)",
    ),
    row(Cmd::Left, Group::Line, "cursor left"),
    row(Cmd::Right, Group::Line, "cursor right"),
    row(Cmd::LineStart, Group::Line, "start of the line"),
    row(Cmd::LineEnd, Group::Line, "end of the line"),
    row(
        Cmd::Backspace,
        Group::Line,
        "delete before the cursor; on an empty line remove the last filter chip",
    ),
    row(Cmd::Delete, Group::Line, "delete at the cursor"),
    row(Cmd::DeleteWord, Group::Line, "delete the previous word"),
    row(Cmd::ClearLine, Group::Line, "clear the line"),
    row(Cmd::Refresh, Group::App, "refresh from disk"),
    Row {
        command: Cmd::Help,
        which: Which::Printable,
        group: Group::App,
        label: "this help (when the text line is empty)",
    },
    Row {
        command: Cmd::Help,
        which: Which::Other,
        group: Group::App,
        label: "this help, always",
    },
    row(
        Cmd::Cancel,
        Group::App,
        "clear the query; exit when it is empty",
    ),
    row(
        Cmd::NormalMode,
        Group::App,
        "normal mode: letters become commands",
    ),
    row(
        Cmd::SearchClear,
        Group::App,
        "search again, from an empty query",
    ),
    row(
        Cmd::SearchKeep,
        Group::App,
        "search again, keeping the query",
    ),
    row(Cmd::Quit, Group::App, "exit immediately"),
];

/// The keys of a row as the help shows them, `None` when it has none here.
fn row_display(map: &EffectiveKeymap, ctx: Context, r: &Row) -> Option<String> {
    let keys: Vec<String> = map
        .keys(ctx, r.command)
        .iter()
        .filter(|k| match r.which {
            Which::All => true,
            Which::Printable => k.second.is_none() && k.first.is_printable(),
            Which::Other => !(k.second.is_none() && k.first.is_printable()),
        })
        .map(|k| k.display())
        .collect();
    (!keys.is_empty()).then(|| keys.join("  "))
}

/// A few words for the compact help.
pub fn short_label(c: Cmd) -> &'static str {
    match c {
        Cmd::Pick => "pick and exit",
        Cmd::Copy => "copy payload",
        Cmd::CopyTemplate => "copy template",
        Cmd::Open => "open",
        Cmd::Run => "run",
        Cmd::ActionMenu => "action menu",
        Cmd::Edit => "edit entry",
        Cmd::AddEntry => "add entry",
        Cmd::RenameEntry => "rename entry",
        Cmd::DeleteEntry => "delete entry",
        Cmd::Down => "next result",
        Cmd::Up => "previous result",
        Cmd::PageDown => "page down",
        Cmd::PageUp => "page up",
        Cmd::HalfPageDown => "half page down",
        Cmd::HalfPageUp => "half page up",
        Cmd::Last => "last result",
        Cmd::First => "first result",
        Cmd::TypeNext => "next type",
        Cmd::TypePrev => "previous type",
        Cmd::ScrollPreviewDown | Cmd::ScrollPreviewUp => "scroll preview",
        Cmd::ScrollPreviewPageDown | Cmd::ScrollPreviewPageUp => "preview page",
        Cmd::TogglePreview => "zoom preview",
        Cmd::Left => "cursor left",
        Cmd::Right => "cursor right",
        Cmd::LineStart => "line start",
        Cmd::LineEnd => "line end",
        Cmd::Backspace => "delete, drop chip",
        Cmd::Delete => "delete",
        Cmd::DeleteWord => "delete word",
        Cmd::ClearLine => "clear line",
        Cmd::Refresh => "refresh",
        Cmd::Sync => "sync",
        Cmd::History => "history",
        Cmd::Help => "help",
        Cmd::Cancel => "clear / quit",
        Cmd::Quit => "exit",
        Cmd::NormalMode => "normal mode",
        Cmd::SearchClear => "search, clear",
        Cmd::SearchKeep => "search, keep",
        _ => "",
    }
}

/// The titles of the vim sections of the help (the emacs keymap has none).
pub const MODE_TITLES: [&str; 2] = ["NORMAL MODE", "SEARCH MODE"];

/// The modes whose keys are listed: the current one first, and the other too when `both`.
fn listed(map: &EffectiveKeymap, normal: bool, both: bool) -> Vec<(Context, &'static str)> {
    if map.keymap == Keymap::Emacs {
        return vec![(Context::Search, "")];
    }
    let (n, s) = (
        (Context::Normal, MODE_TITLES[0]),
        (Context::Search, MODE_TITLES[1]),
    );
    match (normal, both) {
        (true, true) => vec![n, s],
        (false, true) => vec![s, n],
        (true, false) => vec![n],
        (false, false) => vec![s],
    }
}

/// One short line per binding of the current mode, for terminals too short for `help_lines`.
pub fn help_compact(map: &EffectiveKeymap, normal: bool) -> Vec<String> {
    let mut out = Vec::new();
    for (ctx, title) in listed(map, normal, false) {
        if !title.is_empty() {
            out.push(title.to_string());
        }
        for r in ROWS {
            if let Some(d) = row_display(map, ctx, r) {
                out.push(format!("{d:<19}{}", short_label(r.command)));
            }
        }
    }
    out
}

/// The sentence about keys some terminals keep for themselves.
pub fn help_note() -> [&'static str; 2] {
    [
        "Terminals may keep Shift-PgUp/PgDn and Ctrl-Home/End for themselves;",
        "use PgUp/PgDn, Shift-↑/↓ and Alt-P instead.",
    ]
}

/// The lines of the help overlay: each group title, then one line per binding. The vim keymap
/// lists its two modes one after the other (the current mode first) when `both`.
pub fn help_lines(map: &EffectiveKeymap, normal: bool, both: bool) -> Vec<String> {
    let mut out = Vec::new();
    for (ctx, title) in listed(map, normal, both) {
        if !title.is_empty() {
            out.push(title.to_string());
        }
        for g in Group::ALL {
            let rows: Vec<String> = ROWS
                .iter()
                .filter(|r| r.group == g)
                .filter_map(|r| row_display(map, ctx, r).map(|d| format!("  {d:<20} {}", r.label)))
                .collect();
            if rows.is_empty() {
                continue;
            }
            out.push(g.title().to_string());
            out.extend(rows);
        }
        if ctx == Context::Search {
            out.push(
                "  typing                edit the query; `tag:x ` + space makes a filter chip"
                    .to_string(),
            );
        }
        out.push(String::new());
    }
    out.extend(help_note().map(str::to_string));
    out
}

/// What the key bar is describing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    Results {
        text_empty: bool,
        preview_scrolls: bool,
        entry_type: Option<kb_core::entry::EntryType>,
    },
    NoResults {
        text_empty: bool,
    },
    EmptyKb,
    FullPreview,
    Help,
    TooSmall,
}

type Item = (u8, u8, String);

/// The key bar text for `kind`, dropping the least important items until it fits `width`.
pub fn key_bar(map: &EffectiveKeymap, normal: bool, kind: BarKind, width: usize) -> String {
    let ctx = list_context(normal);
    let vim = map.keymap == Keymap::Vim;
    let hint = |cmd: Cmd, word: &str| map.hint(ctx, cmd).map(|k| format!("{k} {word}"));
    let first = |cmd: Cmd| map.hint(ctx, cmd);
    // `↑↓ select`, or the two keys of a pair (`j/k select`)
    let pair = |a: Cmd, b: Cmd, arrows: (&str, &str), word: &str, short: &str| {
        let (x, y) = (first(a)?, first(b));
        Some(match y {
            Some(y) if x == arrows.0 && y == arrows.1 => format!("{short} {word}"),
            Some(y) => format!("{x}/{y} {word}"),
            None => format!("{x} {word}"),
        })
    };
    let select = || pair(Cmd::Down, Cmd::Up, ("↓", "↑"), "select", "↑↓");
    let scroll = || {
        pair(
            Cmd::ScrollPreviewDown,
            Cmd::ScrollPreviewUp,
            ("Shift-↓", "Shift-↑"),
            "scroll",
            "S-↑↓",
        )
    };
    let kinds = || {
        pair(
            Cmd::TypeNext,
            Cmd::TypePrev,
            ("Tab", "Shift-Tab"),
            "type",
            "Tab",
        )
    };
    let esc = |text_empty: bool, order: u8| -> Option<Item> {
        let text = if vim && !normal {
            hint(Cmd::NormalMode, "normal")?
        } else if vim && text_empty {
            hint(Cmd::Quit, "quit")?
        } else {
            hint(Cmd::Cancel, if text_empty { "quit" } else { "clear" })?
        };
        Some((order, 9, text))
    };
    let search = || {
        (vim && normal)
            .then(|| hint(Cmd::SearchClear, "search").map(|t| (9, 8, t)))
            .flatten()
    };
    let it = |order: u8, keep: u8, t: Option<String>| t.map(|t| (order, keep, t));
    let mut items: Vec<Item> = match kind {
        BarKind::Results {
            text_empty,
            preview_scrolls,
            entry_type,
        } => {
            use kb_core::entry::EntryType;
            let mut items: Vec<Option<Item>> = vec![
                it(1, 8, select()),
                it(2, 10, hint(Cmd::Pick, "pick")),
                it(7, 7, hint(Cmd::Help, "help")),
                esc(text_empty, 8),
                search(),
            ];
            match entry_type {
                None => {
                    items.push(it(3, 6, kinds()));
                    items.push(it(4, 4, hint(Cmd::Copy, "copy")));
                    items.push(it(5, 5, hint(Cmd::Edit, "edit")));
                    if preview_scrolls {
                        items.push(it(6, 3, scroll()));
                    }
                }
                Some(t) => {
                    items.push(match t {
                        EntryType::Link => it(3, 6, hint(Cmd::Open, "open")),
                        EntryType::Sumo | EntryType::Cw => it(3, 6, hint(Cmd::Open, "console")),
                        EntryType::Cmd => it(3, 6, hint(Cmd::Run, "run")),
                        EntryType::Code => it(3, 6, hint(Cmd::Open, "code view")),
                        EntryType::Sql | EntryType::Prompt | EntryType::Note | EntryType::Runbook => {
                            it(3, 4, hint(Cmd::Copy, "copy"))
                        }
                    });
                    items.push(it(4, 5, hint(Cmd::Edit, "edit")));
                    items.push(it(5, 7, hint(Cmd::ActionMenu, "actions")));
                    if preview_scrolls {
                        items.push(it(6, 7, scroll()));
                    }
                }
            }
            items.into_iter().flatten().collect()
        }
        BarKind::NoResults { text_empty } => [
            it(7, 7, hint(Cmd::Help, "help")),
            esc(text_empty, 1),
            search(),
        ]
        .into_iter()
        .flatten()
        .collect(),
        BarKind::EmptyKb => [it(7, 7, hint(Cmd::Help, "help")), esc(true, 1), search()]
            .into_iter()
            .flatten()
            .collect(),
        BarKind::FullPreview => [
            it(2, 10, hint(Cmd::Pick, "pick")),
            it(4, 4, hint(Cmd::Copy, "copy")),
            it(5, 5, hint(Cmd::Edit, "edit")),
            it(6, 3, scroll()),
            it(9, 9, hint(Cmd::TogglePreview, "back")),
        ]
        .into_iter()
        .flatten()
        .collect(),
        BarKind::Help => return "any key closes".to_string(),
        BarKind::TooSmall => return String::new(),
    };
    items.sort_by_key(|i| i.0);
    let join = |items: &[Item]| {
        items
            .iter()
            .map(|i| i.2.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    };
    while join(&items).chars().count() > width && items.len() > 1 {
        let Some(lowest) = items
            .iter()
            .enumerate()
            .min_by_key(|(_, i)| i.1)
            .map(|(n, _)| n)
        else {
            break;
        };
        items.remove(lowest);
    }
    join(&items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn emacs() -> EffectiveKeymap {
        EffectiveKeymap::preset(Keymap::Emacs)
    }

    fn vim() -> EffectiveKeymap {
        EffectiveKeymap::preset(Keymap::Vim)
    }

    fn press(code: Code, mods: u8, mode: Mode) -> Option<Command> {
        resolve(&emacs(), Key::new(code, mods), mode, false)
    }

    #[test]
    fn every_documented_key_resolves() {
        let s = Mode::Search;
        let cases: Vec<(Code, u8, Command)> = vec![
            (Code::Enter, 0, Command::Pick),
            (Code::Char('y'), CTRL, Command::Copy),
            (Code::Char('e'), CTRL, Command::Edit),
            (Code::Down, 0, Command::Down),
            (Code::Char('j'), CTRL, Command::Down),
            (Code::Char('n'), CTRL, Command::AddEntry),
            (Code::Char('r'), CTRL, Command::RenameEntry),
            (Code::Char('x'), CTRL, Command::DeleteEntry),
            (Code::Up, 0, Command::Up),
            (Code::Char('p'), CTRL, Command::Up),
            (Code::Char('k'), CTRL, Command::Up),
            (Code::PageDown, 0, Command::PageDown),
            (Code::PageUp, 0, Command::PageUp),
            (Code::End, CTRL, Command::Last),
            (Code::Home, CTRL, Command::First),
            (Code::Tab, 0, Command::TypeNext),
            (Code::BackTab, 0, Command::TypePrev),
            (Code::Down, SHIFT, Command::ScrollDown),
            (Code::Up, SHIFT, Command::ScrollUp),
            (Code::PageDown, SHIFT, Command::ScrollPageDown),
            (Code::PageUp, SHIFT, Command::ScrollPageUp),
            (Code::Char('p'), ALT, Command::TogglePreview),
            (Code::Left, 0, Command::Left),
            (Code::Right, 0, Command::Right),
            (Code::Home, 0, Command::LineStart),
            (Code::Char('a'), CTRL, Command::LineStart),
            (Code::End, 0, Command::LineEnd),
            (Code::Backspace, 0, Command::Backspace),
            (Code::Delete, 0, Command::Delete),
            (Code::Char('w'), CTRL, Command::DeleteWord),
            (Code::Char('u'), CTRL, Command::ClearLine),
            (Code::Char('l'), CTRL, Command::Refresh),
            (Code::Char('?'), 0, Command::HelpKey),
            (Code::F(1), 0, Command::Help),
            (Code::Esc, 0, Command::Esc),
            (Code::Char('c'), CTRL, Command::Quit),
        ];
        for (code, mods, want) in cases {
            assert_eq!(press(code, mods, s), Some(want), "{code:?} {mods}");
        }
        assert_eq!(press(Code::Char('x'), 0, s), Some(Command::Insert('x')));
        assert_eq!(press(Code::Char('X'), 0, s), Some(Command::Insert('X')));
        assert_eq!(press(Code::Char('é'), 0, s), Some(Command::Insert('é')));
        assert_eq!(press(Code::Char('z'), CTRL, s), None);
    }

    #[test]
    fn emacs_has_no_normal_mode_keys() {
        let e = emacs();
        for c in [Cmd::NormalMode, Cmd::HalfPageDown, Cmd::SearchClear] {
            assert!(e.keys(Context::Search, c).is_empty(), "{c:?}");
        }
    }

    #[test]
    fn ctrl_n_adds_and_no_longer_selects_the_next_result() {
        let e = emacs();
        for (code, mods) in [(Code::Down, 0), (Code::Char('j'), CTRL)] {
            assert_eq!(press(code, mods, Mode::Search), Some(Command::Down));
        }
        for (code, mods) in [
            (Code::Up, 0),
            (Code::Char('p'), CTRL),
            (Code::Char('k'), CTRL),
        ] {
            assert_eq!(press(code, mods, Mode::Search), Some(Command::Up));
        }
        let down: Vec<String> = e
            .keys(Context::Search, Cmd::Down)
            .iter()
            .map(|k| k.display())
            .collect();
        assert!(!down.join(" ").contains("Ctrl-N"), "{down:?}");
    }

    #[test]
    fn modes_limit_keys() {
        let e = emacs();
        let full = |code, mods| resolve(&e, Key::new(code, mods), Mode::FullPreview, false);
        assert_eq!(full(Code::Down, 0), None);
        assert_eq!(full(Code::Down, SHIFT), Some(Command::ScrollDown));
        assert_eq!(full(Code::Char('p'), ALT), Some(Command::TogglePreview));
        assert_eq!(full(Code::Esc, 0), Some(Command::Esc));
        assert_eq!(full(Code::Char('x'), 0), None);
        assert_eq!(full(Code::Char('?'), 0), None, "`?` is typing, not help");
        assert_eq!(full(Code::F(1), 0), Some(Command::Help));
        assert_eq!(
            press(Code::Char('c'), CTRL, Mode::TooSmall),
            Some(Command::Quit)
        );
        assert_eq!(press(Code::Enter, 0, Mode::TooSmall), None);
        assert_eq!(
            press(Code::Char('a'), 0, Mode::Help),
            Some(Command::CloseHelp)
        );
        assert_eq!(press(Code::Esc, 0, Mode::Help), Some(Command::CloseHelp));
        assert_eq!(
            press(Code::Char('c'), CTRL, Mode::Help),
            Some(Command::Quit)
        );
    }

    #[test]
    fn vim_normal_mode_letters() {
        let v = vim();
        let n = |c: char| resolve(&v, Key::plain(c), Mode::Normal, true);
        assert_eq!(n('o'), Some(Command::Open));
        assert_eq!(n('e'), Some(Command::Edit));
        assert_eq!(n('y'), Some(Command::Copy));
        assert_eq!(n('!'), Some(Command::Run));
        assert_eq!(n('j'), Some(Command::Down));
        assert_eq!(n('/'), Some(Command::SearchClear));
        assert_eq!(n('i'), Some(Command::SearchKeep));
        assert_eq!(n('q'), Some(Command::Quit));
        assert_eq!(n('?'), Some(Command::HelpKey));
        assert_eq!(n('x'), None, "normal mode never types");
        let s = |k: Key| resolve(&v, k, Mode::Search, false);
        assert_eq!(s(Key::plain('o')), Some(Command::Insert('o')));
        assert_eq!(s(Key::new(Code::Esc, 0)), Some(Command::NormalMode));
        assert_eq!(s(Key::ctrl('o')), Some(Command::Open));
        assert_eq!(s(Key::ctrl('u')), Some(Command::ClearLine));
        assert_eq!(
            resolve(&v, Key::ctrl('d'), Mode::Normal, true),
            Some(Command::HalfPageDown)
        );
        // the full preview keeps the keys of the mode it came from
        assert_eq!(
            resolve(&v, Key::plain('p'), Mode::FullPreview, true),
            Some(Command::TogglePreview)
        );
        assert_eq!(resolve(&v, Key::plain('p'), Mode::FullPreview, false), None);
    }

    /// SC-006 at the keymap level: every list command can be moved to an unused key in every
    /// mode where it exists, and that key then triggers it.
    #[test]
    fn every_list_command_can_be_rebound() {
        use kb_core::keys::{KeyOverride, valid};
        for keymap in [Keymap::Emacs, Keymap::Vim] {
            for (ctx, mode, normal) in [
                (Context::Search, Mode::Search, false),
                (Context::Normal, Mode::Normal, true),
            ] {
                for cmd in Cmd::ALL {
                    let Some(want) = from_core(*cmd, Key::new(Code::F(9), 0)) else {
                        continue;
                    };
                    if !valid(keymap, ctx, *cmd) {
                        continue;
                    }
                    let o = KeyOverride {
                        context: ctx.name().into(),
                        command: cmd.name().into(),
                        keys: vec!["f9".into(), "ctrl-c".into()],
                        file: None,
                        line: Some(1),
                        error: None,
                    };
                    // ctrl-c belongs to `quit` alone
                    let keys = if *cmd == Cmd::Quit {
                        o.keys.clone()
                    } else {
                        vec!["f9".into()]
                    };
                    let map = EffectiveKeymap::build(keymap, &[KeyOverride { keys, ..o }]);
                    assert_eq!(map.warnings, [], "{keymap:?} {ctx:?} {cmd:?}");
                    assert_eq!(
                        resolve(&map, Key::new(Code::F(9), 0), mode, normal),
                        Some(want),
                        "{keymap:?} {ctx:?} {cmd:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn terminal_events_become_keys() {
        let f = |c, m| from_event(ev(c, m));
        assert_eq!(
            f(KeyCode::Char('A'), KeyModifiers::SHIFT),
            Some(Key::new(Code::Char('A'), 0))
        );
        assert_eq!(
            f(KeyCode::BackTab, KeyModifiers::SHIFT),
            Some(Key::new(Code::BackTab, 0))
        );
        assert_eq!(
            f(KeyCode::Char('J'), KeyModifiers::CONTROL),
            Some(Key::new(Code::Char('j'), CTRL))
        );
        assert_eq!(
            f(KeyCode::Down, KeyModifiers::SHIFT),
            Some(Key::new(Code::Down, SHIFT))
        );
        assert_eq!(
            f(KeyCode::Home, KeyModifiers::CONTROL),
            Some(Key::new(Code::Home, CTRL))
        );
        assert_eq!(
            f(KeyCode::Char('p'), KeyModifiers::ALT),
            Some(Key::new(Code::Char('p'), ALT))
        );
        let mut release = ev(KeyCode::Char('a'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(from_event(release), None);
        assert_eq!(f(KeyCode::Null, KeyModifiers::NONE), None);
        for k in [Key::ctrl('o'), Key::new(Code::Down, SHIFT), Key::plain('G')] {
            assert_eq!(from_event(to_event(k)), Some(k));
        }
    }

    #[test]
    fn key_bar_matches_the_contract() {
        let e = emacs();
        let r = |text_empty, preview_scrolls| BarKind::Results {
            text_empty,
            preview_scrolls,
            entry_type: None,
        };
        let bar = |k, w| key_bar(&e, false, k, w);
        assert_eq!(
            bar(r(false, false), 200),
            "↑↓ select · Enter pick · Tab type · ^Y copy · ^E edit · ? help · Esc clear"
        );
        assert_eq!(
            bar(r(true, false), 200),
            "↑↓ select · Enter pick · Tab type · ^Y copy · ^E edit · ? help · Esc quit"
        );
        assert_eq!(
            bar(r(true, true), 200),
            "↑↓ select · Enter pick · Tab type · ^Y copy · ^E edit · S-↑↓ scroll · ? help · Esc quit"
        );
        assert_eq!(
            bar(BarKind::NoResults { text_empty: false }, 200),
            "Esc clear · ? help"
        );
        assert_eq!(
            bar(BarKind::NoResults { text_empty: true }, 200),
            "Esc quit · ? help"
        );
        assert_eq!(bar(BarKind::EmptyKb, 200), "Esc quit · ? help");
        assert_eq!(
            bar(BarKind::FullPreview, 200),
            "Enter pick · ^Y copy · ^E edit · S-↑↓ scroll · Alt-P back"
        );
        assert_eq!(bar(BarKind::Help, 200), "any key closes");
        assert_eq!(bar(BarKind::TooSmall, 200), "");
    }

    #[test]
    fn the_vim_key_bar_shows_the_keys_of_the_mode() {
        let v = vim();
        let r = BarKind::Results {
            text_empty: true,
            preview_scrolls: false,
            entry_type: None,
        };
        assert_eq!(
            key_bar(&v, true, r, 200),
            "j/k select · Enter pick · l/h type · y copy · e edit · ? help · q quit · / search"
        );
        let search = key_bar(&v, false, r, 200);
        assert!(
            search.contains("^Y copy") && search.ends_with("Esc normal"),
            "{search}"
        );
    }

    #[test]
    fn the_key_bar_follows_the_user_keys() {
        use kb_core::keys::KeyOverride;
        let o = KeyOverride {
            context: "normal".into(),
            command: "copy".into(),
            keys: vec!["x".into()],
            file: None,
            line: None,
            error: None,
        };
        let v = EffectiveKeymap::build(Keymap::Vim, &[o]);
        let r = BarKind::Results {
            text_empty: true,
            preview_scrolls: false,
            entry_type: None,
        };
        assert!(key_bar(&v, true, r, 200).contains("x copy"));
    }

    #[test]
    fn key_bar_never_offers_what_does_not_work_and_fits() {
        let e = emacs();
        for w in [120usize, 80, 60, 40, 20] {
            let bar = key_bar(
                &e,
                false,
                BarKind::Results {
                    text_empty: false,
                    preview_scrolls: true,
                    entry_type: None,
                },
                w,
            );
            assert!(bar.chars().count() <= w.max(8), "{w}: {bar}");
        }
        let narrow = key_bar(
            &e,
            false,
            BarKind::Results {
                text_empty: false,
                preview_scrolls: false,
                entry_type: None,
            },
            40,
        );
        assert!(
            narrow.contains("Enter pick") && narrow.contains("Esc"),
            "{narrow}"
        );
        for kind in [BarKind::NoResults { text_empty: false }, BarKind::EmptyKb] {
            let bar = key_bar(&e, false, kind, 200);
            assert!(
                !bar.contains("Enter") && !bar.contains("^E") && !bar.contains("^Y"),
                "{bar}"
            );
        }
    }

    #[test]
    fn help_lists_every_binding_once() {
        let e = emacs();
        let lines = help_lines(&e, false, true);
        let text = lines.join("\n");
        for r in ROWS {
            let Some(d) = row_display(&e, Context::Search, r) else {
                continue;
            };
            assert_eq!(
                lines
                    .iter()
                    .filter(|l| l.contains(r.label) && l.contains(d.split("  ").next().unwrap()))
                    .count(),
                1,
                "{}",
                r.label
            );
        }
        for g in Group::ALL {
            assert!(lines.iter().any(|l| l == g.title()), "{g:?}");
        }
        assert!(text.contains("Shift-PgUp/PgDn") && text.contains("Ctrl-Home/End"));
    }

    #[test]
    fn the_vim_help_lists_both_modes() {
        let v = vim();
        let lines = help_lines(&v, true, true);
        let n = lines.iter().position(|l| l == "NORMAL MODE").unwrap();
        let s = lines.iter().position(|l| l == "SEARCH MODE").unwrap();
        assert!(n < s, "the current mode comes first");
        let text = lines.join("\n");
        assert!(
            text.contains("gg") && text.contains("half a page"),
            "{text}"
        );
        assert_eq!(help_lines(&v, false, true)[0], "SEARCH MODE");
    }

    #[test]
    fn the_compact_help_has_one_short_line_per_binding() {
        let e = emacs();
        let lines = help_compact(&e, false);
        let shown = ROWS
            .iter()
            .filter(|r| row_display(&e, Context::Search, r).is_some())
            .count();
        assert_eq!(lines.len(), shown);
        for line in &lines {
            assert!(line.chars().count() <= 19 + 18, "{line}");
        }
        for r in ROWS {
            assert!(!short_label(r.command).is_empty(), "{:?}", r.command);
        }
    }

    #[test]
    fn the_readme_lists_every_bound_key() {
        let readme = include_str!("../../../../README.md");
        for keymap in [Keymap::Emacs, Keymap::Vim] {
            let map = EffectiveKeymap::preset(keymap);
            for ctx in [Context::Search, Context::Normal] {
                for r in ROWS {
                    for k in map.keys(ctx, r.command) {
                        let shown = k.display();
                        assert!(
                            readme.contains(&shown),
                            "README.md does not mention {shown:?} ({keymap:?} {ctx:?} {:?})",
                            r.command
                        );
                    }
                }
            }
        }
    }
}
