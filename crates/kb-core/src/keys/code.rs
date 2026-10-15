//! Keys, independent of the terminal library: the TUI converts terminal events into these.

/// A key, without modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

/// A key with its modifiers. Characters are normalised: Shift is folded into the character
/// (`Shift-a` is `Char('A')`) and Ctrl keys are lowercase (`Ctrl-A` is `Ctrl-a`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub code: Code,
    pub mods: u8,
}

impl Key {
    pub const fn new(code: Code, mods: u8) -> Key {
        Key { code, mods }
    }

    pub const fn plain(c: char) -> Key {
        Key::new(Code::Char(c), 0)
    }

    pub const fn ctrl(c: char) -> Key {
        Key::new(Code::Char(c), CTRL)
    }

    /// A printable character typed without Ctrl or Alt: what a text field would insert.
    pub fn is_printable(self) -> bool {
        matches!(self.code, Code::Char(c) if !c.is_control()) && self.mods & (CTRL | ALT) == 0
    }

    /// The config syntax: `ctrl-o`, `alt-a`, `shift-down`, `f2`, `G`, `space`.
    pub fn name(self) -> String {
        let mut s = String::new();
        for (bit, name) in [(CTRL, "ctrl-"), (ALT, "alt-"), (SHIFT, "shift-")] {
            if self.mods & bit != 0 {
                s.push_str(name);
            }
        }
        match self.code {
            Code::Char(' ') => s.push_str("space"),
            Code::Char(c) => s.push(c),
            Code::F(n) => s.push_str(&format!("f{n}")),
            other => s.push_str(match other {
                Code::Enter => "enter",
                Code::Esc => "esc",
                Code::Tab => "tab",
                Code::BackTab => "backtab",
                Code::Backspace => "backspace",
                Code::Delete => "delete",
                Code::Left => "left",
                Code::Right => "right",
                Code::Up => "up",
                Code::Down => "down",
                Code::Home => "home",
                Code::End => "end",
                Code::PageUp => "pageup",
                Code::PageDown => "pagedown",
                Code::Char(_) | Code::F(_) => "",
            }),
        }
        s
    }

    /// How the help overlay and the key bar show it: `Ctrl-O`, `Alt-A`, `Shift-↓`, `F2`, `G`.
    pub fn display(self) -> String {
        let mut s = String::new();
        for (bit, name) in [(CTRL, "Ctrl-"), (ALT, "Alt-"), (SHIFT, "Shift-")] {
            if self.mods & bit != 0 {
                s.push_str(name);
            }
        }
        match self.code {
            Code::Char(' ') => s.push_str("Space"),
            Code::Char(c) if self.mods & CTRL != 0 => s.extend(c.to_uppercase()),
            Code::Char(c) if self.mods & ALT != 0 => s.extend(c.to_uppercase()),
            Code::Char(c) => s.push(c),
            Code::F(n) => s.push_str(&format!("F{n}")),
            Code::Enter => s.push_str("Enter"),
            Code::Esc => s.push_str("Esc"),
            Code::Tab => s.push_str("Tab"),
            Code::BackTab => s.push_str("Shift-Tab"),
            Code::Backspace => s.push_str("Backspace"),
            Code::Delete => s.push_str("Delete"),
            Code::Left => s.push('←'),
            Code::Right => s.push('→'),
            Code::Up => s.push('↑'),
            Code::Down => s.push('↓'),
            Code::Home => s.push_str("Home"),
            Code::End => s.push_str("End"),
            Code::PageUp => s.push_str("PgUp"),
            Code::PageDown => s.push_str("PgDn"),
        }
        s
    }

    /// The short form for the key bar: `^O` for Ctrl letters, otherwise as `display`.
    pub fn short(self) -> String {
        match self {
            Key {
                code: Code::Char(c),
                mods: CTRL,
            } => format!("^{}", c.to_ascii_uppercase()),
            k => k.display(),
        }
    }
}

/// One key, or two typed one after the other (`g g`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeySeq {
    pub first: Key,
    pub second: Option<Key>,
}

impl KeySeq {
    pub const fn one(k: Key) -> KeySeq {
        KeySeq {
            first: k,
            second: None,
        }
    }

    /// Config syntax; the two keys are separated by a space.
    pub fn name(self) -> String {
        match self.second {
            Some(s) => format!("{} {}", self.first.name(), s.name()),
            None => self.first.name(),
        }
    }

    /// Help syntax: `gg`, `Ctrl-O`.
    pub fn display(self) -> String {
        match self.second {
            Some(s) => format!("{}{}", self.first.display(), s.display()),
            None => self.first.display(),
        }
    }

    pub fn short(self) -> String {
        match self.second {
            Some(s) => format!("{}{}", self.first.short(), s.short()),
            None => self.first.short(),
        }
    }
}
