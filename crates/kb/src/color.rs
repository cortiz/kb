//! Terminal color: plain ANSI, on only when stdout is a terminal and `NO_COLOR` is unset or empty.

use std::io::IsTerminal;
use std::ops::Range;

const START: &str = "\x1b[1;33m";
const END: &str = "\x1b[0m";

/// Whether to color stdout (see no-color.org: any non-empty `NO_COLOR` turns it off).
pub fn enabled() -> bool {
    gate(
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").as_deref(),
    )
}

pub fn gate(is_tty: bool, no_color: Option<&std::ffi::OsStr>) -> bool {
    is_tty && no_color.is_none_or(std::ffi::OsStr::is_empty)
}

/// `text` with the character `ranges` in bold yellow when `on`, unchanged otherwise.
pub fn highlight(text: &str, ranges: &[Range<usize>], on: bool) -> String {
    if !on || ranges.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + ranges.len() * 12);
    for (i, c) in text.chars().enumerate() {
        if ranges.iter().any(|r| r.start == i) {
            out.push_str(START);
        }
        out.push(c);
        if ranges.iter().any(|r| r.end == i + 1) {
            out.push_str(END);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn gate_needs_a_terminal_and_no_no_color() {
        assert!(gate(true, None));
        assert!(gate(true, Some(OsStr::new(""))));
        assert!(!gate(true, Some(OsStr::new("1"))));
        assert!(!gate(false, None));
    }

    #[test]
    fn highlights_characters_not_bytes() {
        assert_eq!(highlight("año nuevo", &[0..3], false), "año nuevo");
        assert_eq!(
            highlight("año nuevo", &[0..3], true),
            format!("{START}año{END} nuevo")
        );
        assert_eq!(highlight("abc", &[], true), "abc");
        assert_eq!(
            highlight("ab cd", &[0..2, 3..5], true),
            format!("{START}ab{END} {START}cd{END}")
        );
    }
}
