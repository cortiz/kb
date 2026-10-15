//! Key bindings: the key syntax, the commands, the two presets (`emacs`, `vim`) and the
//! effective keymap built from a preset plus the `[tui.keys.*]` overrides in config.

mod code;
mod command;
mod context;
mod effective;
mod parse;
mod preset;
#[cfg(test)]
mod tests;

pub use code::{ALT, CTRL, Code, Key, KeySeq, SHIFT};
pub use command::Command;
pub use context::Context;
pub use effective::{
    Binding, EffectiveKeymap, KeyOverride, KeyWarning, Origin, Translated, duplicates, is_reserved,
};
pub use parse::{ACCEPTED, parse_key, parse_seq};
pub use preset::{Keymap, Row, rows, valid};
