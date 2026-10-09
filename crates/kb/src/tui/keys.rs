//! The key map: one table that drives key handling, the key bar and the help overlay.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// A key, independent of the terminal library so the table can be checked at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

pub const SHIFT: u8 = 1;
pub const CTRL: u8 = 2;
pub const ALT: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub code: Code,
    pub mods: u8,
}

const fn code_eq(a: Code, b: Code) -> bool {
    match (a, b) {
        (Code::Char(x), Code::Char(y)) => x == y,
        (Code::F(x), Code::F(y)) => x == y,
        (Code::Enter, Code::Enter)
        | (Code::Esc, Code::Esc)
        | (Code::Tab, Code::Tab)
        | (Code::BackTab, Code::BackTab)
        | (Code::Backspace, Code::Backspace)
        | (Code::Delete, Code::Delete)
        | (Code::Left, Code::Left)
        | (Code::Right, Code::Right)
        | (Code::Up, Code::Up)
        | (Code::Down, Code::Down)
        | (Code::Home, Code::Home)
        | (Code::End, Code::End)
        | (Code::PageUp, Code::PageUp)
        | (Code::PageDown, Code::PageDown) => true,
        _ => false,
    }
}

impl Key {
    pub const fn new(code: Code, mods: u8) -> Key {
        Key { code, mods }
    }

    pub const fn same(self, other: Key) -> bool {
        code_eq(self.code, other.code) && self.mods == other.mods
    }

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
}

/// Screens a binding is active on.
pub const SEARCH: u8 = 1;
pub const FULL: u8 = 2;
pub const HELP: u8 = 4;
pub const SMALL: u8 = 8;
pub const FORM: u8 = 16;
pub const ALL: u8 = SEARCH | FULL | HELP | SMALL;

/// What the UI is showing, for key resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Search,
    FullPreview,
    Help,
    TooSmall,
    /// The variable form is open; it handles its own keys.
    Form,
}

impl Mode {
    pub const fn bit(self) -> u8 {
        match self {
            Mode::Search => SEARCH,
            Mode::FullPreview => FULL,
            Mode::Help => HELP,
            Mode::TooSmall => SMALL,
            Mode::Form => FORM,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Pick,
    Copy,
    /// Alt-C: copy the payload with its `{{variables}}` left in.
    CopyTemplate,
    Edit,
    Down,
    Up,
    PageDown,
    PageUp,
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
    /// `?`: help when the text line is empty, otherwise the character is typed.
    HelpKey,
    Help,
    Esc,
    Quit,
    /// Any key while the help overlay is open.
    CloseHelp,
    /// A printable character.
    Insert(char),
}

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

/// A key bar item: where it goes, how long it survives on a narrow terminal, and its text.
#[derive(Debug, Clone, Copy)]
pub struct Bar {
    pub order: u8,
    pub keep: u8,
    pub text: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub keys: &'static [Key],
    pub modes: u8,
    pub command: Command,
    pub group: Group,
    /// The keys as shown in the help overlay.
    pub display: &'static str,
    pub label: &'static str,
    pub bar: Option<Bar>,
}

const fn k(code: Code) -> Key {
    Key::new(code, 0)
}
const fn ctrl(c: char) -> Key {
    Key::new(Code::Char(c), CTRL)
}
const fn shift(code: Code) -> Key {
    Key::new(code, SHIFT)
}
const fn bar(order: u8, keep: u8, text: &'static str) -> Option<Bar> {
    Some(Bar { order, keep, text })
}

const SF: u8 = SEARCH | FULL;

pub const BINDINGS: &[Binding] = &[
    Binding {
        keys: &[k(Code::Enter)],
        modes: SF,
        command: Command::Pick,
        group: Group::Pick,
        display: "Enter",
        label: "pick: copy the entry and exit",
        bar: bar(2, 10, "Enter pick"),
    },
    Binding {
        keys: &[ctrl('y')],
        modes: SF,
        command: Command::Copy,
        group: Group::Pick,
        display: "Ctrl-Y",
        label: "copy the payload and stay open",
        bar: bar(4, 4, "^Y copy"),
    },
    Binding {
        keys: &[Key::new(Code::Char('c'), ALT)],
        modes: SF,
        command: Command::CopyTemplate,
        group: Group::Pick,
        display: "Alt-C",
        label: "copy the payload with its {{variables}} left in",
        bar: None,
    },
    Binding {
        keys: &[ctrl('e')],
        modes: SF,
        command: Command::Edit,
        group: Group::Pick,
        display: "Ctrl-E",
        label: "edit the entry in $VISUAL / $EDITOR",
        bar: bar(5, 5, "^E edit"),
    },
    Binding {
        keys: &[k(Code::Down), ctrl('n'), ctrl('j')],
        modes: SEARCH,
        command: Command::Down,
        group: Group::Move,
        display: "↓  Ctrl-N  Ctrl-J",
        label: "select the next result",
        bar: bar(1, 8, "↑↓ select"),
    },
    Binding {
        keys: &[k(Code::Up), ctrl('p'), ctrl('k')],
        modes: SEARCH,
        command: Command::Up,
        group: Group::Move,
        display: "↑  Ctrl-P  Ctrl-K",
        label: "select the previous result",
        bar: None,
    },
    Binding {
        keys: &[k(Code::PageDown)],
        modes: SEARCH,
        command: Command::PageDown,
        group: Group::Move,
        display: "PgDn",
        label: "down one page",
        bar: None,
    },
    Binding {
        keys: &[k(Code::PageUp)],
        modes: SEARCH,
        command: Command::PageUp,
        group: Group::Move,
        display: "PgUp",
        label: "up one page",
        bar: None,
    },
    Binding {
        keys: &[Key::new(Code::End, CTRL)],
        modes: SEARCH,
        command: Command::Last,
        group: Group::Move,
        display: "Ctrl-End",
        label: "last result",
        bar: None,
    },
    Binding {
        keys: &[Key::new(Code::Home, CTRL)],
        modes: SEARCH,
        command: Command::First,
        group: Group::Move,
        display: "Ctrl-Home",
        label: "first result",
        bar: None,
    },
    Binding {
        keys: &[k(Code::Tab)],
        modes: SEARCH,
        command: Command::TypeNext,
        group: Group::Filter,
        display: "Tab",
        label: "next type filter",
        bar: bar(3, 6, "Tab type"),
    },
    Binding {
        keys: &[k(Code::BackTab)],
        modes: SEARCH,
        command: Command::TypePrev,
        group: Group::Filter,
        display: "Shift-Tab",
        label: "previous type filter",
        bar: None,
    },
    Binding {
        keys: &[shift(Code::Down)],
        modes: SF,
        command: Command::ScrollDown,
        group: Group::Preview,
        display: "Shift-↓",
        label: "scroll the preview one line down",
        bar: bar(6, 3, "S-↑↓ scroll"),
    },
    Binding {
        keys: &[shift(Code::Up)],
        modes: SF,
        command: Command::ScrollUp,
        group: Group::Preview,
        display: "Shift-↑",
        label: "scroll the preview one line up",
        bar: None,
    },
    Binding {
        keys: &[shift(Code::PageDown)],
        modes: SF,
        command: Command::ScrollPageDown,
        group: Group::Preview,
        display: "Shift-PgDn",
        label: "scroll the preview one page down",
        bar: None,
    },
    Binding {
        keys: &[shift(Code::PageUp)],
        modes: SF,
        command: Command::ScrollPageUp,
        group: Group::Preview,
        display: "Shift-PgUp",
        label: "scroll the preview one page up",
        bar: None,
    },
    Binding {
        keys: &[Key::new(Code::Char('p'), ALT)],
        modes: SF,
        command: Command::TogglePreview,
        group: Group::Preview,
        display: "Alt-P",
        label: "full-screen preview (narrow terminals)",
        bar: bar(9, 2, "Alt-P zoom"),
    },
    Binding {
        keys: &[k(Code::Left)],
        modes: SEARCH,
        command: Command::Left,
        group: Group::Line,
        display: "←",
        label: "cursor left",
        bar: None,
    },
    Binding {
        keys: &[k(Code::Right)],
        modes: SEARCH,
        command: Command::Right,
        group: Group::Line,
        display: "→",
        label: "cursor right",
        bar: None,
    },
    Binding {
        keys: &[k(Code::Home), ctrl('a')],
        modes: SEARCH,
        command: Command::LineStart,
        group: Group::Line,
        display: "Home  Ctrl-A",
        label: "start of the line",
        bar: None,
    },
    Binding {
        keys: &[k(Code::End)],
        modes: SEARCH,
        command: Command::LineEnd,
        group: Group::Line,
        display: "End",
        label: "end of the line",
        bar: None,
    },
    Binding {
        keys: &[k(Code::Backspace)],
        modes: SEARCH,
        command: Command::Backspace,
        group: Group::Line,
        display: "Backspace",
        label: "delete before the cursor; on an empty line remove the last filter chip",
        bar: None,
    },
    Binding {
        keys: &[k(Code::Delete)],
        modes: SEARCH,
        command: Command::Delete,
        group: Group::Line,
        display: "Delete",
        label: "delete at the cursor",
        bar: None,
    },
    Binding {
        keys: &[ctrl('w')],
        modes: SEARCH,
        command: Command::DeleteWord,
        group: Group::Line,
        display: "Ctrl-W",
        label: "delete the previous word",
        bar: None,
    },
    Binding {
        keys: &[ctrl('u')],
        modes: SEARCH,
        command: Command::ClearLine,
        group: Group::Line,
        display: "Ctrl-U",
        label: "clear the line",
        bar: None,
    },
    Binding {
        keys: &[ctrl('l')],
        modes: SF,
        command: Command::Refresh,
        group: Group::App,
        display: "Ctrl-L",
        label: "refresh from disk",
        bar: None,
    },
    Binding {
        keys: &[k(Code::Char('?'))],
        modes: SEARCH,
        command: Command::HelpKey,
        group: Group::App,
        display: "?",
        label: "this help (when the text line is empty)",
        bar: bar(7, 7, "? help"),
    },
    Binding {
        keys: &[k(Code::F(1))],
        modes: SF,
        command: Command::Help,
        group: Group::App,
        display: "F1",
        label: "this help, always",
        bar: None,
    },
    Binding {
        keys: &[k(Code::Esc)],
        modes: SF,
        command: Command::Esc,
        group: Group::App,
        display: "Esc",
        label: "clear the query; exit when it is empty",
        bar: bar(8, 9, "Esc"),
    },
    Binding {
        keys: &[ctrl('c')],
        modes: ALL,
        command: Command::Quit,
        group: Group::App,
        display: "Ctrl-C",
        label: "exit immediately",
        bar: None,
    },
];

/// True when two bindings that can be active together share a key.
pub const fn has_duplicates(table: &[Binding]) -> bool {
    let mut a = 0;
    while a < table.len() {
        let mut ka = 0;
        while ka < table[a].keys.len() {
            let mut b = a;
            while b < table.len() {
                if table[a].modes & table[b].modes != 0 {
                    let mut kb = if a == b { ka + 1 } else { 0 };
                    while kb < table[b].keys.len() {
                        if table[a].keys[ka].same(table[b].keys[kb]) {
                            return true;
                        }
                        kb += 1;
                    }
                }
                b += 1;
            }
            ka += 1;
        }
        a += 1;
    }
    false
}

const _: () = assert!(
    !has_duplicates(BINDINGS),
    "a key is bound twice in the same mode"
);

/// What a key does in `mode`.
pub fn resolve(key: Key, mode: Mode) -> Option<Command> {
    if mode == Mode::Help && !(key.same(ctrl('c'))) {
        return Some(Command::CloseHelp);
    }
    for b in BINDINGS {
        if b.modes & mode.bit() != 0 && b.keys.iter().any(|x| x.same(key)) {
            return Some(b.command);
        }
    }
    if mode == Mode::Search
        && let Code::Char(c) = key.code
        && key.mods & (CTRL | ALT) == 0
        && !c.is_control()
    {
        return Some(Command::Insert(c));
    }
    None
}

/// What the key bar is describing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    Results {
        text_empty: bool,
        preview_scrolls: bool,
    },
    NoResults {
        text_empty: bool,
    },
    EmptyKb,
    FullPreview,
    Help,
    TooSmall,
}

/// The key bar text for `kind`, dropping the least important items until it fits `width`.
pub fn key_bar(kind: BarKind, width: usize) -> String {
    let from_table = |mode: Mode, want: &dyn Fn(Command) -> bool| -> Vec<(u8, u8, String)> {
        BINDINGS
            .iter()
            .filter(|b| b.modes & mode.bit() != 0 && want(b.command))
            .filter_map(|b| b.bar.map(|x| (x.order, x.keep, x.text.to_string())))
            .collect()
    };
    let esc = |text_empty: bool, order: u8| {
        (
            order,
            9,
            format!("Esc {}", if text_empty { "quit" } else { "clear" }),
        )
    };
    let mut items = match kind {
        BarKind::Results {
            text_empty,
            preview_scrolls,
        } => {
            let mut items = from_table(Mode::Search, &|c| {
                matches!(
                    c,
                    Command::Down
                        | Command::Pick
                        | Command::TypeNext
                        | Command::Copy
                        | Command::Edit
                        | Command::HelpKey
                ) || (preview_scrolls && c == Command::ScrollDown)
            });
            items.push(esc(text_empty, 8));
            items
        }
        BarKind::NoResults { text_empty } => {
            let mut items = from_table(Mode::Search, &|c| c == Command::HelpKey);
            items.push(esc(text_empty, 1));
            items
        }
        BarKind::EmptyKb => {
            let mut items = from_table(Mode::Search, &|c| c == Command::HelpKey);
            items.push(esc(true, 1));
            items
        }
        BarKind::FullPreview => {
            let mut items = from_table(Mode::FullPreview, &|c| {
                matches!(
                    c,
                    Command::ScrollDown | Command::Copy | Command::Edit | Command::Pick
                )
            });
            items.push((9, 9, "Alt-P back".to_string()));
            items
        }
        BarKind::Help => return "any key closes".to_string(),
        BarKind::TooSmall => return String::new(),
    };
    items.sort_by_key(|i| i.0);
    let join = |items: &[(u8, u8, String)]| {
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

/// A few words for the compact help.
pub fn short_label(c: Command) -> &'static str {
    match c {
        Command::Pick => "pick and exit",
        Command::Copy => "copy payload",
        Command::CopyTemplate => "copy template",
        Command::Edit => "edit entry",
        Command::Down => "next result",
        Command::Up => "previous result",
        Command::PageDown => "page down",
        Command::PageUp => "page up",
        Command::Last => "last result",
        Command::First => "first result",
        Command::TypeNext => "next type",
        Command::TypePrev => "previous type",
        Command::ScrollDown => "scroll preview",
        Command::ScrollUp => "scroll preview",
        Command::ScrollPageDown => "preview page",
        Command::ScrollPageUp => "preview page",
        Command::TogglePreview => "zoom preview",
        Command::Left => "cursor left",
        Command::Right => "cursor right",
        Command::LineStart => "line start",
        Command::LineEnd => "line end",
        Command::Backspace => "delete, drop chip",
        Command::Delete => "delete",
        Command::DeleteWord => "delete word",
        Command::ClearLine => "clear line",
        Command::Refresh => "refresh",
        Command::HelpKey | Command::Help => "help",
        Command::Esc => "clear / quit",
        Command::Quit => "exit",
        Command::CloseHelp | Command::Insert(_) => "",
    }
}

/// One short line per binding, for terminals too short for `help_lines`.
pub fn help_compact() -> Vec<String> {
    BINDINGS
        .iter()
        .map(|b| format!("{:<19}{}", b.display, short_label(b.command)))
        .collect()
}

/// The sentence about keys some terminals keep for themselves.
pub fn help_note() -> [&'static str; 2] {
    [
        "Terminals may keep Shift-PgUp/PgDn and Ctrl-Home/End for themselves;",
        "use PgUp/PgDn, Shift-↑/↓ and Alt-P instead.",
    ]
}

/// The lines of the help overlay: each group title, then one line per binding.
pub fn help_lines() -> Vec<String> {
    let mut out = Vec::new();
    for g in Group::ALL {
        out.push(g.title().to_string());
        for b in BINDINGS.iter().filter(|b| b.group == g) {
            out.push(format!("  {:<20} {}", b.display, b.label));
        }
    }
    out.push(
        "  typing                edit the query; `tag:x ` + space makes a filter chip".to_string(),
    );
    out.push(String::new());
    out.extend(help_note().map(str::to_string));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn press(code: Code, mods: u8, mode: Mode) -> Option<Command> {
        resolve(Key::new(code, mods), mode)
    }

    #[test]
    fn the_real_table_has_no_duplicates_and_the_check_works() {
        assert!(!has_duplicates(BINDINGS));
        const DUP: &[Binding] = &[
            Binding {
                keys: &[ctrl('x')],
                modes: SEARCH,
                command: Command::Pick,
                group: Group::Pick,
                display: "",
                label: "",
                bar: None,
            },
            Binding {
                keys: &[k(Code::Up), ctrl('x')],
                modes: SF,
                command: Command::Up,
                group: Group::Move,
                display: "",
                label: "",
                bar: None,
            },
        ];
        assert!(has_duplicates(DUP));
        const OK: &[Binding] = &[
            Binding {
                keys: &[ctrl('x')],
                modes: SEARCH,
                command: Command::Pick,
                group: Group::Pick,
                display: "",
                label: "",
                bar: None,
            },
            Binding {
                keys: &[ctrl('x')],
                modes: HELP,
                command: Command::Up,
                group: Group::Move,
                display: "",
                label: "",
                bar: None,
            },
        ];
        assert!(!has_duplicates(OK));
        const SELF_DUP: &[Binding] = &[Binding {
            keys: &[ctrl('x'), ctrl('x')],
            modes: SEARCH,
            command: Command::Pick,
            group: Group::Pick,
            display: "",
            label: "",
            bar: None,
        }];
        assert!(has_duplicates(SELF_DUP));
    }

    #[test]
    fn every_documented_key_resolves() {
        let s = Mode::Search;
        let cases: Vec<(Code, u8, Command)> = vec![
            (Code::Enter, 0, Command::Pick),
            (Code::Char('y'), CTRL, Command::Copy),
            (Code::Char('e'), CTRL, Command::Edit),
            (Code::Down, 0, Command::Down),
            (Code::Char('n'), CTRL, Command::Down),
            (Code::Char('j'), CTRL, Command::Down),
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
        assert_eq!(press(Code::Char('x'), CTRL, s), None);
    }

    #[test]
    fn modes_limit_keys() {
        assert_eq!(press(Code::Down, 0, Mode::FullPreview), None);
        assert_eq!(
            press(Code::Down, SHIFT, Mode::FullPreview),
            Some(Command::ScrollDown)
        );
        assert_eq!(
            press(Code::Char('p'), ALT, Mode::FullPreview),
            Some(Command::TogglePreview)
        );
        assert_eq!(press(Code::Esc, 0, Mode::FullPreview), Some(Command::Esc));
        assert_eq!(press(Code::Char('x'), 0, Mode::FullPreview), None);
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
    fn terminal_events_become_keys() {
        let f = |c, m| Key::from_event(ev(c, m));
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
        assert_eq!(Key::from_event(release), None);
        assert_eq!(f(KeyCode::Null, KeyModifiers::NONE), None);
    }

    #[test]
    fn key_bar_matches_the_contract() {
        let r = |text_empty, preview_scrolls| BarKind::Results {
            text_empty,
            preview_scrolls,
        };
        assert_eq!(
            key_bar(r(false, false), 200),
            "↑↓ select · Enter pick · Tab type · ^Y copy · ^E edit · ? help · Esc clear"
        );
        assert_eq!(
            key_bar(r(true, false), 200),
            "↑↓ select · Enter pick · Tab type · ^Y copy · ^E edit · ? help · Esc quit"
        );
        assert_eq!(
            key_bar(r(true, true), 200),
            "↑↓ select · Enter pick · Tab type · ^Y copy · ^E edit · S-↑↓ scroll · ? help · Esc quit"
        );
        assert_eq!(
            key_bar(BarKind::NoResults { text_empty: false }, 200),
            "Esc clear · ? help"
        );
        assert_eq!(
            key_bar(BarKind::NoResults { text_empty: true }, 200),
            "Esc quit · ? help"
        );
        assert_eq!(key_bar(BarKind::EmptyKb, 200), "Esc quit · ? help");
        assert_eq!(
            key_bar(BarKind::FullPreview, 200),
            "Enter pick · ^Y copy · ^E edit · S-↑↓ scroll · Alt-P back"
        );
        assert_eq!(key_bar(BarKind::Help, 200), "any key closes");
        assert_eq!(key_bar(BarKind::TooSmall, 200), "");
    }

    #[test]
    fn key_bar_never_offers_what_does_not_work_and_fits() {
        for w in [120usize, 80, 60, 40, 20] {
            let bar = key_bar(
                BarKind::Results {
                    text_empty: false,
                    preview_scrolls: true,
                },
                w,
            );
            assert!(bar.chars().count() <= w.max(8), "{w}: {bar}");
        }
        let narrow = key_bar(
            BarKind::Results {
                text_empty: false,
                preview_scrolls: false,
            },
            40,
        );
        assert!(
            narrow.contains("Enter pick") && narrow.contains("Esc"),
            "{narrow}"
        );
        for kind in [BarKind::NoResults { text_empty: false }, BarKind::EmptyKb] {
            let bar = key_bar(kind, 200);
            assert!(
                !bar.contains("Enter") && !bar.contains("^E") && !bar.contains("^Y"),
                "{bar}"
            );
        }
    }

    #[test]
    fn help_lists_every_binding_once() {
        let lines = help_lines();
        let text = lines.join("\n");
        for b in BINDINGS {
            assert_eq!(
                lines
                    .iter()
                    .filter(|l| l.contains(b.label)
                        && l.contains(b.display.split("  ").next().unwrap()))
                    .count(),
                1,
                "{}",
                b.label
            );
        }
        for g in Group::ALL {
            assert!(lines.iter().any(|l| l == g.title()), "{g:?}");
        }
        assert!(text.contains("Shift-PgUp/PgDn") && text.contains("Ctrl-Home/End"));
    }

    #[test]
    fn the_compact_help_has_one_short_line_per_binding() {
        let lines = help_compact();
        assert_eq!(lines.len(), BINDINGS.len());
        for (line, b) in lines.iter().zip(BINDINGS) {
            assert!(line.starts_with(b.display), "{line}");
            assert!(line.chars().count() <= 19 + 18, "{line}");
        }
        for b in BINDINGS {
            assert!(!short_label(b.command).is_empty());
        }
    }

    #[test]
    fn the_readme_lists_every_bound_key() {
        let readme = include_str!("../../../../README.md");
        for b in BINDINGS {
            assert!(
                readme.contains(b.display),
                "README.md does not mention {:?}",
                b.display
            );
        }
    }
}
