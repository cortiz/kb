//! The lossless representation of one entry file: its text, BOM and line endings.

use std::ops::Range;
use std::sync::OnceLock;

/// A byte range into [`Source::text`].
pub type Span = Range<usize>;

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// The line-ending style of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
    Mixed,
}

impl LineEnding {
    /// The string to use when inserting a new line into a file with this style.
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::CrLf => "\r\n",
            LineEnding::Lf | LineEnding::Mixed => "\n",
        }
    }
}

/// The file is not valid UTF-8. `offset` is the first bad byte in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidUtf8 {
    pub offset: usize,
}

/// Converts character indexes into byte offsets for one string.
///
/// The YAML parser reports positions in characters (research R2). ASCII text needs no table.
#[derive(Debug, Clone)]
pub struct CharMap {
    table: Option<Vec<usize>>,
    len: usize,
}

impl CharMap {
    pub fn new(text: &str) -> CharMap {
        if text.is_ascii() {
            return CharMap {
                table: None,
                len: usize::MAX,
            };
        }
        let mut table: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let len = table.len();
        table.push(text.len());
        CharMap {
            table: Some(table),
            len,
        }
    }

    /// Byte offset of the character at `char_idx`; `char_idx` may equal the character count.
    pub fn byte(&self, char_idx: usize) -> usize {
        match &self.table {
            None => char_idx,
            Some(table) => table[char_idx.min(self.len)],
        }
    }

    /// True when the map is the identity (pure-ASCII text).
    pub fn is_identity(&self) -> bool {
        self.table.is_none() && self.len == usize::MAX
    }
}

/// The text of an entry file, with enough information to write it back byte for byte.
#[derive(Debug, Clone)]
pub struct Source {
    text: String,
    bom: bool,
    line_ending: LineEnding,
    line_starts: Vec<usize>,
    char_map: OnceLock<CharMap>,
}

impl Source {
    /// Decodes a file. A leading UTF-8 BOM is remembered and removed from the text.
    pub fn from_bytes(mut bytes: Vec<u8>) -> Result<Source, InvalidUtf8> {
        let bom = bytes.starts_with(BOM);
        let skip = if bom { BOM.len() } else { 0 };
        if let Err(e) = std::str::from_utf8(&bytes[skip..]) {
            return Err(InvalidUtf8 {
                offset: skip + e.valid_up_to(),
            });
        }
        bytes.drain(..skip);
        let text = String::from_utf8(bytes).expect("validated above");
        Ok(Source::from_text(text, bom))
    }

    /// Builds a source from text that is already decoded.
    pub fn from_text(text: String, bom: bool) -> Source {
        let mut line_starts = vec![0];
        let (mut lf, mut crlf) = (false, false);
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                if i > 0 && text.as_bytes()[i - 1] == b'\r' {
                    crlf = true;
                } else {
                    lf = true;
                }
                line_starts.push(i + 1);
            }
        }
        let line_ending = match (lf, crlf) {
            (true, true) => LineEnding::Mixed,
            (false, true) => LineEnding::CrLf,
            _ => LineEnding::Lf,
        };
        Source {
            text,
            bom,
            line_ending,
            line_starts,
            char_map: OnceLock::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn bom(&self) -> bool {
        self.bom
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Byte offset where each line starts.
    pub fn line_starts(&self) -> &[usize] {
        &self.line_starts
    }

    /// The exact bytes of the file: the BOM, if any, then the text.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.text.len() + 3);
        if self.bom {
            out.extend_from_slice(BOM);
        }
        out.extend_from_slice(self.text.as_bytes());
        out
    }

    /// 1-based line and column for a byte offset. Columns count Unicode characters.
    pub fn position(&self, byte: usize) -> (u32, u32) {
        let byte = byte.min(self.text.len());
        let line = self
            .line_starts
            .partition_point(|&start| start <= byte)
            .max(1);
        let start = self.line_starts[line - 1];
        let column = self.text[start..byte].chars().count() + 1;
        (to_u32(line), to_u32(column))
    }

    /// Byte offset of the character at `char_idx` in the text.
    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        self.char_map
            .get_or_init(|| CharMap::new(&self.text))
            .byte(char_idx)
    }
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_is_stripped_and_restored() {
        let bytes = b"\xEF\xBB\xBF---\nx: 1\n---\n".to_vec();
        let src = Source::from_bytes(bytes.clone()).unwrap();
        assert!(src.bom());
        assert_eq!(src.text(), "---\nx: 1\n---\n");
        assert_eq!(src.to_bytes(), bytes);
    }

    #[test]
    fn no_bom_round_trips() {
        let bytes = b"a\nb".to_vec();
        let src = Source::from_bytes(bytes.clone()).unwrap();
        assert!(!src.bom());
        assert_eq!(src.to_bytes(), bytes);
    }

    #[test]
    fn line_endings_are_classified() {
        let le = |s: &str| Source::from_text(s.to_string(), false).line_ending();
        assert_eq!(le("a\nb\n"), LineEnding::Lf);
        assert_eq!(le("a\r\nb\r\n"), LineEnding::CrLf);
        assert_eq!(le("a\r\nb\n"), LineEnding::Mixed);
        assert_eq!(le("no newline"), LineEnding::Lf);
    }

    #[test]
    fn line_starts_for_lf_and_crlf() {
        assert_eq!(
            Source::from_text("ab\ncd\n".into(), false).line_starts(),
            &[0, 3, 6]
        );
        assert_eq!(
            Source::from_text("ab\r\ncd".into(), false).line_starts(),
            &[0, 4]
        );
    }

    #[test]
    fn position_counts_characters() {
        let src = Source::from_text("héllo\nwörld x".into(), false);
        assert_eq!(src.position(0), (1, 1));
        // "x" is after "wörld " on line 2: byte 7 + "wörld ".len() (7 bytes) = 14.
        let x = src.text().find('x').unwrap();
        assert_eq!(x, 14);
        assert_eq!(src.position(x), (2, 7));
        assert_eq!(src.position(src.text().len()), (2, 8));
    }

    #[test]
    fn position_on_crlf_line() {
        let src = Source::from_text("a\r\nbc".into(), false);
        assert_eq!(src.position(4), (2, 2));
    }

    #[test]
    fn char_to_byte_for_non_ascii() {
        let src = Source::from_text("é-ü".into(), false);
        assert_eq!(src.char_to_byte(0), 0);
        assert_eq!(src.char_to_byte(1), 2);
        assert_eq!(src.char_to_byte(2), 3);
        assert_eq!(src.char_to_byte(3), 5);
    }

    #[test]
    fn ascii_needs_no_table() {
        let map = CharMap::new("plain ascii");
        assert!(map.is_identity());
        assert_eq!(map.byte(5), 5);
        assert!(!CharMap::new("é").is_identity());
    }

    #[test]
    fn invalid_utf8_reports_first_bad_byte() {
        let err = Source::from_bytes(b"ok\xFFrest".to_vec()).unwrap_err();
        assert_eq!(err, InvalidUtf8 { offset: 2 });
        let err = Source::from_bytes(b"\xEF\xBB\xBFab\xC3".to_vec()).unwrap_err();
        assert_eq!(err, InvalidUtf8 { offset: 5 });
    }
}
