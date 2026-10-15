//! The key syntax of `[tui.keys.*]`: `ctrl-o`, `alt-a`, `shift-down`, `f2`, `G`, `g g`.

use super::code::{ALT, CTRL, Code, Key, KeySeq, SHIFT};

const NAMED: &[(&str, Code)] = &[
    ("enter", Code::Enter),
    ("esc", Code::Esc),
    ("tab", Code::Tab),
    ("backtab", Code::BackTab),
    ("space", Code::Char(' ')),
    ("backspace", Code::Backspace),
    ("delete", Code::Delete),
    ("up", Code::Up),
    ("down", Code::Down),
    ("left", Code::Left),
    ("right", Code::Right),
    ("home", Code::Home),
    ("end", Code::End),
    ("pageup", Code::PageUp),
    ("pagedown", Code::PageDown),
];

pub const ACCEPTED: &str = "modifiers ctrl- alt- shift-; keys enter esc tab backtab space backspace delete up down left right home end pageup pagedown f1-f12; or one character; two keys separated by a space";

pub fn parse_key(s: &str) -> Result<Key, String> {
    let mut rest = s;
    let mut mods = 0u8;
    loop {
        let lower = rest.to_ascii_lowercase();
        let (bit, len) = if lower.starts_with("ctrl-") {
            (CTRL, 5)
        } else if lower.starts_with("alt-") {
            (ALT, 4)
        } else if lower.starts_with("shift-") {
            (SHIFT, 6)
        } else {
            break;
        };
        if rest.len() == len {
            break; // `ctrl-` alone is not a modifier chain
        }
        if mods & bit != 0 {
            return Err(format!("{s:?}: modifier given twice"));
        }
        mods |= bit;
        rest = &rest[len..];
    }
    let lower = rest.to_ascii_lowercase();
    let code = if let Some((_, c)) = NAMED.iter().find(|(n, _)| *n == lower) {
        *c
    } else if let Some(n) = lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| (1..=12).contains(n))
    {
        Code::F(n)
    } else {
        let mut chars = rest.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if !c.is_control() => Code::Char(c),
            _ => return Err(format!("unknown key {s:?} (accepted: {ACCEPTED})")),
        }
    };
    normalise(code, mods, s)
}

fn normalise(code: Code, mut mods: u8, s: &str) -> Result<Key, String> {
    match code {
        Code::Char(c) => {
            if mods & SHIFT != 0 {
                if !c.is_alphabetic() {
                    return Err(format!("{s:?}: Shift cannot be combined with {c:?}"));
                }
                mods &= !SHIFT;
                let upper = c.to_uppercase().next().unwrap_or(c);
                return Ok(Key::new(
                    Code::Char(if mods & CTRL != 0 {
                        c.to_ascii_lowercase()
                    } else {
                        upper
                    }),
                    mods,
                ));
            }
            Ok(Key::new(
                Code::Char(if mods & CTRL != 0 {
                    c.to_ascii_lowercase()
                } else {
                    c
                }),
                mods,
            ))
        }
        Code::Tab if mods & SHIFT != 0 => Ok(Key::new(Code::BackTab, mods & !SHIFT)),
        Code::BackTab => Ok(Key::new(Code::BackTab, mods & !SHIFT)),
        c => Ok(Key::new(c, mods)),
    }
}

/// One key, or two keys separated by whitespace.
pub fn parse_seq(s: &str) -> Result<KeySeq, String> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    match parts.as_slice() {
        [a] => Ok(KeySeq::one(parse_key(a)?)),
        [a, b] => Ok(KeySeq {
            first: parse_key(a)?,
            second: Some(parse_key(b)?),
        }),
        [] => Err("empty key".into()),
        _ => Err(format!("{s:?}: a sequence has at most two keys")),
    }
}
