//! Splits an entry into its YAML frontmatter and Markdown body.

use crate::lint::RuleCode;
use crate::source::Span;

/// Where the frontmatter and body are in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    /// The YAML between the delimiter lines.
    pub frontmatter: Span,
    /// Byte offset where the closing `---` line starts.
    pub close_line: usize,
    /// Everything after the closing delimiter line.
    pub body: Span,
}

/// Why a file has no usable frontmatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitError {
    /// The first line is not `---`.
    NoFrontmatter,
    /// An opening `---` with no closing `---` line.
    Unclosed,
    /// The file is empty or only whitespace.
    Empty,
}

impl SplitError {
    pub fn code(self) -> RuleCode {
        match self {
            SplitError::NoFrontmatter => RuleCode::NoFrontmatter,
            SplitError::Unclosed => RuleCode::UnclosedFrontmatter,
            SplitError::Empty => RuleCode::EmptyFile,
        }
    }
}

/// Splits `text` (without BOM) at its `---` delimiter lines.
pub fn split(text: &str) -> Result<Split, SplitError> {
    if text.trim().is_empty() {
        return Err(SplitError::Empty);
    }
    let mut lines = Lines { text, pos: 0 };
    match lines.next() {
        Some((_, line)) if is_delimiter(line) => {}
        _ => return Err(SplitError::NoFrontmatter),
    }
    let start = lines.pos;
    for (line_start, line) in lines.by_ref() {
        if is_delimiter(line) {
            return Ok(Split {
                frontmatter: start..line_start,
                close_line: line_start,
                body: lines.pos..text.len(),
            });
        }
    }
    Err(SplitError::Unclosed)
}

fn is_delimiter(line: &str) -> bool {
    line.trim_end_matches([' ', '\t']) == "---"
}

/// Iterates over lines as (start offset, content without the line ending); `pos` is the start
/// of the next line.
struct Lines<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Iterator for Lines<'a> {
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.text.len() {
            return None;
        }
        let start = self.pos;
        let rest = &self.text[start..];
        let (line, advance) = match rest.find('\n') {
            Some(i) => (&rest[..i], i + 1),
            None => (rest, rest.len()),
        };
        self.pos = start + advance;
        Some((start, line.strip_suffix('\r').unwrap_or(line)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(text: &str) -> (&str, &str) {
        let s = split(text).unwrap();
        (&text[s.frontmatter], &text[s.body])
    }

    #[test]
    fn basic_split() {
        let text = "---\na: 1\n---\nbody\n";
        assert_eq!(parts(text), ("a: 1\n", "body\n"));
        assert_eq!(split(text).unwrap().close_line, 9);
    }

    #[test]
    fn crlf_delimiters() {
        assert_eq!(parts("---\r\na: 1\r\n---\r\nbody"), ("a: 1\r\n", "body"));
    }

    #[test]
    fn trailing_whitespace_after_delimiters() {
        assert_eq!(parts("--- \t\na: 1\n---  \nb"), ("a: 1\n", "b"));
    }

    #[test]
    fn empty_body_and_no_final_newline() {
        assert_eq!(parts("---\na: 1\n---"), ("a: 1\n", ""));
        assert_eq!(parts("---\na: 1\n---\n"), ("a: 1\n", ""));
    }

    #[test]
    fn empty_frontmatter() {
        assert_eq!(parts("---\n---\nx"), ("", "x"));
    }

    #[test]
    fn missing_opening() {
        assert_eq!(split("title: x\n---\n"), Err(SplitError::NoFrontmatter));
        assert_eq!(split("----\na\n---\n"), Err(SplitError::NoFrontmatter));
    }

    #[test]
    fn unclosed() {
        assert_eq!(split("---\na: 1\nbody\n"), Err(SplitError::Unclosed));
        assert_eq!(split("---"), Err(SplitError::Unclosed));
    }

    #[test]
    fn empty_and_whitespace() {
        assert_eq!(split(""), Err(SplitError::Empty));
        assert_eq!(split("  \n\t\r\n"), Err(SplitError::Empty));
    }

    #[test]
    fn codes() {
        assert_eq!(
            SplitError::NoFrontmatter.code().to_string(),
            "KB002-no-frontmatter"
        );
        assert_eq!(
            SplitError::Unclosed.code().to_string(),
            "KB003-unclosed-frontmatter"
        );
        assert_eq!(SplitError::Empty.code().to_string(), "KB004-empty-file");
    }
}
