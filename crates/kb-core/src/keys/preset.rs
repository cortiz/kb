//! The two keymaps as data. Duplicate keys inside one context are caught by
//! `tests::presets_have_no_duplicates` (strings are not const-parsable).

use super::command::Command::{self, *};
use super::context::Context;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keymap {
    Emacs,
    Vim,
}

impl Keymap {
    pub fn name(self) -> &'static str {
        match self {
            Keymap::Emacs => "emacs",
            Keymap::Vim => "vim",
        }
    }

    pub fn from_name(s: &str) -> Option<Keymap> {
        match s {
            "emacs" => Some(Keymap::Emacs),
            "vim" => Some(Keymap::Vim),
            _ => None,
        }
    }
}

pub type Row = (Command, &'static [&'static str]);

/// Every key of the emacs keymap in the main list (the keymap that existed before the vim one).
const EMACS_SEARCH: &[Row] = &[
    (Pick, &["enter"]),
    (Copy, &["ctrl-y"]),
    (Open, &["ctrl-o"]),
    (Run, &["ctrl-g"]),
    (ActionMenu, &["alt-a", "f2"]),
    (CopyTemplate, &["alt-c"]),
    (Edit, &["ctrl-e"]),
    (AddEntry, &["ctrl-n"]),
    (RenameEntry, &["ctrl-r"]),
    (DeleteEntry, &["ctrl-x"]),
    (Down, &["down", "ctrl-j"]),
    (Up, &["up", "ctrl-p", "ctrl-k"]),
    (PageDown, &["pagedown"]),
    (PageUp, &["pageup"]),
    (Last, &["ctrl-end"]),
    (First, &["ctrl-home"]),
    (TypeNext, &["tab"]),
    (TypePrev, &["backtab"]),
    (ScrollPreviewDown, &["shift-down"]),
    (ScrollPreviewUp, &["shift-up"]),
    (ScrollPreviewPageDown, &["shift-pagedown"]),
    (ScrollPreviewPageUp, &["shift-pageup"]),
    (History, &["alt-h"]),
    (Sync, &["alt-s"]),
    (TogglePreview, &["alt-p"]),
    (Left, &["left"]),
    (Right, &["right"]),
    (LineStart, &["home", "ctrl-a"]),
    (LineEnd, &["end"]),
    (Backspace, &["backspace"]),
    (Delete, &["delete"]),
    (DeleteWord, &["ctrl-w"]),
    (ClearLine, &["ctrl-u"]),
    (Refresh, &["ctrl-l"]),
    (Help, &["?", "f1"]),
    (Cancel, &["esc"]),
    (Quit, &["ctrl-c"]),
];

const VIM_NORMAL: &[Row] = &[
    (Pick, &["enter"]),
    (Copy, &["y", "ctrl-y"]),
    (CopyTemplate, &["Y", "alt-c"]),
    (Open, &["o", "ctrl-o"]),
    (Run, &["!", "ctrl-g"]),
    (ActionMenu, &["m", ".", "alt-a", "f2"]),
    (Edit, &["e", "ctrl-e"]),
    (AddEntry, &["n", "ctrl-n"]),
    (RenameEntry, &["R", "ctrl-r"]),
    (DeleteEntry, &["D", "ctrl-x"]),
    (Down, &["j", "down", "ctrl-j"]),
    (Up, &["k", "up", "ctrl-p", "ctrl-k"]),
    (PageDown, &["pagedown"]),
    (PageUp, &["pageup"]),
    (HalfPageDown, &["ctrl-d"]),
    (HalfPageUp, &["ctrl-u"]),
    (First, &["g g", "home", "ctrl-home"]),
    (Last, &["G", "end", "ctrl-end"]),
    (TypeNext, &["l", "right", "tab"]),
    (TypePrev, &["h", "left", "backtab"]),
    (ScrollPreviewDown, &["J", "shift-down"]),
    (ScrollPreviewUp, &["K", "shift-up"]),
    (ScrollPreviewPageDown, &["shift-pagedown"]),
    (ScrollPreviewPageUp, &["shift-pageup"]),
    (TogglePreview, &["p", "alt-p"]),
    (SearchClear, &["/"]),
    (SearchKeep, &["i", "a"]),
    (Refresh, &["ctrl-l"]),
    (Sync, &["s", "alt-s"]),
    (History, &["H", "alt-h"]),
    (Help, &["?", "f1"]),
    (Cancel, &["esc"]),
    (Quit, &["q", "ctrl-c"]),
];

// the action menu picks by the action's own letter; Esc closes it
const ACTION_MENU: &[Row] = &[(Quit, &["esc"])];

const CODE_VIEW: &[Row] = &[
    (Pick, &["enter"]),
    (OpenWeb, &["g"]),
    (OpenVscode, &["v"]),
    (OpenIdea, &["i"]),
    (CopyPathLine, &["y"]),
    (CopyPermalink, &["Y"]),
    (Repin, &["u"]),
    (Fetch, &["F"]),
    (Edit, &["ctrl-e"]),
    (Quit, &["esc"]),
];

const HISTORY: &[Row] = &[
    (Down, &["down", "j"]),
    (Up, &["up", "k"]),
    (PageDown, &["pagedown"]),
    (PageUp, &["pageup"]),
    (Pick, &["enter"]),
    (Restore, &["r"]),
    (Quit, &["esc", "q"]),
];

const FORM: &[Row] = &[
    (Pick, &["enter"]),
    (NextField, &["tab", "down"]),
    (PrevField, &["backtab", "up"]),
    (Quit, &["esc"]),
];

const ADD: &[Row] = &[
    (Save, &["ctrl-s"]),
    (EditBody, &["ctrl-e"]),
    (TogglePreview, &["ctrl-p"]),
    (NextField, &["tab"]),
    (PrevField, &["backtab"]),
    (Quit, &["esc"]),
];

// "close" has no keys by default: every key closes the help overlay.
const HELP: &[Row] = &[(Close, &[])];

/// The default keys of `ctx`. Global has none of its own (it overrides both list modes), and the
/// conflict screen's keys are not configurable yet.
pub fn rows(keymap: Keymap, ctx: Context) -> Vec<Row> {
    match (keymap, ctx) {
        (_, Context::Global | Context::Conflict) => Vec::new(),
        (_, Context::ActionMenu) => ACTION_MENU.to_vec(),
        (_, Context::CodeView) => CODE_VIEW.to_vec(),
        (_, Context::History) => HISTORY.to_vec(),
        (_, Context::Form) => FORM.to_vec(),
        (_, Context::Add) => ADD.to_vec(),
        (_, Context::Help) => HELP.to_vec(),
        (Keymap::Emacs, Context::Search) => EMACS_SEARCH.to_vec(),
        (Keymap::Emacs, Context::Normal) => Vec::new(),
        (Keymap::Vim, Context::Normal) => VIM_NORMAL.to_vec(),
        (Keymap::Vim, Context::Search) => {
            let mut v: Vec<Row> = EMACS_SEARCH
                .iter()
                .filter(|(c, _)| *c != Cancel)
                .copied()
                .collect();
            v.push((NormalMode, &["esc"]));
            v
        }
    }
}

fn is_list_command(c: Command) -> bool {
    (c as usize) <= (Quit as usize)
}

/// Whether `cmd` can be bound in `ctx` (a preset row, or any list command in a list mode).
pub fn valid(keymap: Keymap, ctx: Context, cmd: Command) -> bool {
    match ctx {
        Context::Global => is_list_command(cmd) && valid(keymap, Context::Search, cmd),
        Context::Search => {
            is_list_command(cmd)
                && !matches!(cmd, SearchClear | SearchKeep)
                && (cmd != NormalMode || keymap == Keymap::Vim)
        }
        Context::Normal => {
            keymap == Keymap::Vim
                && is_list_command(cmd)
                && !matches!(
                    cmd,
                    Left | Right
                        | LineStart
                        | LineEnd
                        | Backspace
                        | Delete
                        | DeleteWord
                        | ClearLine
                        | NormalMode
                )
        }
        _ => rows(keymap, ctx).iter().any(|(c, _)| *c == cmd),
    }
}
