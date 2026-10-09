//! File-level rules: KB050 (mixed line endings) and KB051 (file too large).

use crate::source::{LineEnding, Source};

/// Files larger than this get KB051.
pub const MAX_FILE_BYTES: usize = 1_048_576;

/// The byte offset of the first line whose ending differs from the first line's, when the file
/// mixes LF and CRLF.
pub fn mixed_line_ending_at(source: &Source) -> Option<usize> {
    if source.line_ending() != LineEnding::Mixed {
        return None;
    }
    let text = source.text().as_bytes();
    let is_crlf = |start: usize| -> Option<bool> {
        let nl = text[start..].iter().position(|&b| b == b'\n')? + start;
        Some(nl > 0 && text[nl - 1] == b'\r')
    };
    let starts = source.line_starts();
    let first = is_crlf(starts[0])?;
    starts
        .iter()
        .copied()
        .find(|&s| is_crlf(s).is_some_and(|c| c != first))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_first_differing_line() {
        let s = Source::from_text("a\r\nb\r\nc\nd\r\n".into(), false);
        assert_eq!(mixed_line_ending_at(&s), Some(6));
        let s = Source::from_text("a\nb\r\n".into(), false);
        assert_eq!(mixed_line_ending_at(&s), Some(2));
        assert_eq!(
            mixed_line_ending_at(&Source::from_text("a\nb\n".into(), false)),
            None
        );
    }
}
