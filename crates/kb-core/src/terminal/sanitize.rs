//! A streaming scanner that removes control characters and escape sequences from text.
//!
//! One pass, no allocation when the text is clean. The scanner is a small state machine fed one
//! character at a time, so text may arrive in pieces. Everything a terminal could act on goes:
//! C0 and C1 controls (but not `\t` and `\n`), DEL, ESC sequences (CSI, OSC, DCS, APC, PM, SOS and
//! the two- and three-character forms like `ESC c`), their 8-bit C1 introducers, and the Unicode
//! bidirectional overrides that make text read in another order than it is stored.
//!
//! A string sequence (OSC, DCS, APC, PM, SOS) ends at BEL, at ST (`ESC \` or U+009C), or at the
//! end of the line, so a sequence that never ends cannot swallow the rest of a document.

use std::borrow::Cow;

/// What an escape sequence turns into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// One visible `␛` (U+241B) per sequence: for screens and `kb show`.
    Display,
    /// Nothing: for output that is piped or saved.
    Raw,
}

/// The character left where an escape sequence was.
pub const MARKER: char = '\u{241B}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Text,
    /// After ESC.
    Esc,
    /// Inside `ESC [` (or U+009B), up to the final byte.
    Csi,
    /// Inside a string sequence.
    Str,
    /// Inside a string sequence, after an ESC (maybe the start of ST).
    StrEsc,
    /// After ESC and an intermediate byte (`ESC ( B`): one more byte ends it.
    Intermediate,
}

/// The state machine: [`push`](Sanitizer::push) every character, then [`finish`](Sanitizer::finish).
#[derive(Debug, Clone)]
pub struct Sanitizer {
    mode: Mode,
    state: State,
}

/// Characters that are removed on their own.
fn is_dropped(c: char) -> bool {
    match c {
        '\t' | '\n' => false,
        c if c.is_control() => true,
        '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => true,
        _ => false,
    }
}

impl Sanitizer {
    pub fn new(mode: Mode) -> Sanitizer {
        Sanitizer {
            mode,
            state: State::Text,
        }
    }

    /// Whether the scanner is in the middle of a sequence.
    pub fn in_sequence(&self) -> bool {
        self.state != State::Text
    }

    fn marker(&self, out: &mut String) {
        if self.mode == Mode::Display {
            out.push(MARKER);
        }
    }

    /// Ends a sequence: one marker, back to text.
    fn end(&mut self, out: &mut String) {
        self.state = State::Text;
        self.marker(out);
    }

    /// Feeds one character; whatever is safe to show is appended to `out`.
    pub fn push(&mut self, c: char, out: &mut String) {
        match self.state {
            State::Text => self.text(c, out),
            State::Esc => match c {
                '[' => self.state = State::Csi,
                ']' | 'P' | '_' | '^' | 'X' => self.state = State::Str,
                ' '..='/' => self.state = State::Intermediate,
                '0'..='~' => self.end(out),
                // a lone ESC: the marker, then this character as ordinary text
                _ => {
                    self.end(out);
                    self.text(c, out);
                }
            },
            State::Csi => match c {
                '\u{20}'..='\u{3F}' => {}
                '\u{40}'..='\u{7E}' => self.end(out),
                // malformed: the sequence ends here, and the character is handled as text
                _ => {
                    self.end(out);
                    self.text(c, out);
                }
            },
            State::Str => match c {
                '\u{07}' | '\u{9C}' => self.end(out),
                '\u{1B}' => self.state = State::StrEsc,
                // an unterminated sequence must not eat the rest of the document
                '\n' => {
                    self.end(out);
                    out.push('\n');
                }
                _ => {}
            },
            State::StrEsc => match c {
                '\\' => self.end(out),
                // `ESC ESC`: still waiting for the `\`
                '\u{1B}' => {}
                // no ST: the string ended, and `ESC` plus this character start something new
                _ => {
                    self.end(out);
                    self.state = State::Esc;
                    self.push(c, out);
                }
            },
            State::Intermediate => match c {
                ' '..='/' => {}
                '0'..='~' => self.end(out),
                _ => {
                    self.end(out);
                    self.text(c, out);
                }
            },
        }
    }

    fn text(&mut self, c: char, out: &mut String) {
        match c {
            '\u{1B}' => self.state = State::Esc,
            // 8-bit introducers: CSI, then the string kinds DCS, SOS, OSC, PM, APC
            '\u{9B}' => self.state = State::Csi,
            '\u{90}' | '\u{98}' | '\u{9D}' | '\u{9E}' | '\u{9F}' => self.state = State::Str,
            c if is_dropped(c) => {}
            c => out.push(c),
        }
    }

    /// The text ended: a sequence still open leaves its marker.
    pub fn finish(&mut self, out: &mut String) {
        if self.state != State::Text {
            self.end(out);
        }
    }
}

/// `text` without control characters and escape sequences. Borrowed when `text` is clean.
pub fn sanitize(text: &str, mode: Mode) -> Cow<'_, str> {
    if !text.chars().any(|c| c == '\u{1B}' || is_dropped(c)) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut s = Sanitizer::new(mode);
    for c in text.chars() {
        s.push(c, &mut out);
    }
    s.finish(&mut out);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disp(s: &str) -> String {
        sanitize(s, Mode::Display).into_owned()
    }

    fn raw(s: &str) -> String {
        sanitize(s, Mode::Raw).into_owned()
    }

    #[test]
    fn clean_text_is_borrowed() {
        for s in ["", "plain", "tab\tand\nnewline", "ünï©ödé 日本語 ␛ ✓"] {
            assert!(
                matches!(sanitize(s, Mode::Display), Cow::Borrowed(_)),
                "{s:?}"
            );
        }
    }

    #[test]
    fn c0_c1_and_del_are_removed_but_tab_and_newline_stay() {
        assert_eq!(
            disp("a\u{0}b\u{7}c\u{8}d\re\u{7F}f\u{85}g\u{81}h"),
            "abcdefgh"
        );
        assert_eq!(disp("a\tb\nc"), "a\tb\nc");
        assert_eq!(raw("a\u{7}b"), "ab");
    }

    #[test]
    fn csi_is_one_marker() {
        assert_eq!(disp("a\x1b[2Jb"), "a␛b");
        assert_eq!(disp("\x1b[1;31mred\x1b[0m"), "␛red␛");
        assert_eq!(disp("\x1b[?25l"), "␛");
        assert_eq!(raw("a\x1b[2Jb"), "ab");
        assert_eq!(disp("a\u{9B}31mb"), "a␛b", "8-bit CSI");
    }

    #[test]
    fn osc_ends_at_bel_or_st() {
        assert_eq!(disp("a\x1b]52;c;aGk=\x07b"), "a␛b");
        assert_eq!(disp("a\x1b]0;title\x1b\\b"), "a␛b");
        assert_eq!(disp("a\u{9D}0;title\u{9C}b"), "a␛b");
        assert_eq!(raw("a\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\z"), "alinkz");
    }

    #[test]
    fn dcs_apc_pm_and_sos_are_strings_too() {
        for intro in ['P', '_', '^', 'X'] {
            assert_eq!(
                disp(&format!("a\x1b{intro}payload\x1b\\b")),
                "a␛b",
                "{intro}"
            );
        }
        assert_eq!(
            disp("a\u{90}q\u{9C}b\u{9F}x\u{9C}c\u{9E}y\u{9C}d"),
            "a␛b␛c␛d"
        );
    }

    #[test]
    fn two_and_three_character_escapes() {
        assert_eq!(disp("a\x1bcb"), "a␛b", "ESC c resets the terminal");
        assert_eq!(disp("a\x1b7b\x1b8c"), "a␛b␛c");
        assert_eq!(disp("a\x1b(Bb"), "a␛b", "charset select");
        assert_eq!(disp("a\x1b#8b"), "a␛b");
    }

    #[test]
    fn unterminated_sequences_do_not_eat_the_document() {
        assert_eq!(disp("a\x1b]52;c;x\nnext line"), "a␛\nnext line");
        assert_eq!(disp("a\x1b[1;2"), "a␛");
        assert_eq!(disp("a\x1b"), "a␛");
        assert_eq!(disp("a\x1b[1;2\nb"), "a␛\nb", "a newline is not a CSI byte");
        assert_eq!(disp("a\x1b\x07b"), "a␛b");
    }

    #[test]
    fn a_new_escape_ends_the_old_string() {
        assert_eq!(disp("\x1b]0;t\x1b[31mx"), "␛␛x");
        assert_eq!(disp("\x1b]0;t\x1b\x1b\\x"), "␛x");
    }

    #[test]
    fn bidi_overrides_are_removed() {
        assert_eq!(disp("a\u{202E}b\u{2066}c\u{200F}d"), "abcd");
    }

    #[test]
    fn text_in_pieces_gives_the_same_result() {
        let text = "x\x1b[31mred\x1b]52;c;aGk=\x07\x1b(By";
        let whole = disp(text);
        let mut s = Sanitizer::new(Mode::Display);
        let mut out = String::new();
        for c in text.chars() {
            s.push(c, &mut out);
        }
        s.finish(&mut out);
        assert_eq!(out, whole);
        assert_eq!(whole, "x␛red␛␛y");
    }

    #[test]
    fn nothing_that_reaches_the_terminal_is_a_control() {
        // every character below 0x20 and in 0x7F..=0x9F, alone and inside sequences
        for code in (0u32..0x20).chain(0x7F..0xA0) {
            let c = char::from_u32(code).unwrap();
            for text in [
                format!("a{c}b"),
                format!("\x1b[{c}b"),
                format!("\x1b]{c}\x07b"),
            ] {
                for mode in [Mode::Display, Mode::Raw] {
                    let out = sanitize(&text, mode);
                    assert!(
                        out.chars()
                            .all(|ch| ch == '\t' || ch == '\n' || !ch.is_control()),
                        "{text:?} -> {out:?}"
                    );
                }
            }
        }
    }
}
