//! Keeping text from files, remotes and agents away from the terminal.

pub mod sanitize;

use std::borrow::Cow;

pub use sanitize::{Mode, Sanitizer, sanitize};

/// `text` as it may be drawn: control characters and escape sequences are gone, and each escape
/// sequence leaves a visible `␛`. Borrowed when there is nothing to remove.
pub fn display(text: &str) -> Cow<'_, str> {
    sanitize(text, Mode::Display)
}

/// `text` for output that may be piped or redirected: escape sequences are removed without a
/// trace.
pub fn raw(text: &str) -> Cow<'_, str> {
    sanitize(text, Mode::Raw)
}

/// A link target kb itself may turn into a terminal hyperlink: `http`, `https` or `mailto`, with
/// nothing in it a terminal would act on.
pub fn is_safe_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let allowed = ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme));
    allowed && url.len() <= 2048 && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_http_https_and_mailto_links_are_safe() {
        for ok in [
            "https://example.com/a?b=c#d",
            "HTTP://example.com",
            "mailto:a@example.com",
        ] {
            assert!(is_safe_link(ok), "{ok}");
        }
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftp://x",
            "https://a\x1b]8;;\x07b",
            "https://a b",
            "//example.com",
            "",
            "data:text/html,x",
        ] {
            assert!(!is_safe_link(bad), "{bad:?}");
        }
    }
}
