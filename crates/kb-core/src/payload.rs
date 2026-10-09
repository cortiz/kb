//! The payload as text and `{{variable}}` pieces, for display.

use crate::entry::Entry;
use crate::vars::{Brace, scan_refs};

/// A piece of a payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadPiece {
    /// Literal text, with `\{{` already unescaped.
    Text(String),
    /// A reference to the variable `name`.
    Var(String),
}

/// Splits the payload into literal text and `{{name}}` references, using the scanner `kb lint`
/// uses, so what is marked as a variable is exactly what lint counts as one. Entries without a
/// payload (`link`, `code`) give an empty list.
pub fn payload_pieces(entry: &Entry) -> Vec<PayloadPiece> {
    let Some(payload) = entry.payload() else {
        return Vec::new();
    };
    let raw = &entry.source().text()[payload.raw_span.clone()];
    let raw = raw.replace("\r\n", "\n");
    let mut pieces = Vec::new();
    let mut at = 0;
    for brace in scan_refs(&raw) {
        if let Brace::Ref { name, span } = brace {
            if span.start > at {
                pieces.push(PayloadPiece::Text(crate::body::unescape(
                    &raw[at..span.start],
                )));
            }
            pieces.push(PayloadPiece::Var(name));
            at = span.end;
        }
    }
    if at < raw.len() {
        pieces.push(PayloadPiece::Text(crate::body::unescape(&raw[at..])));
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::parse_entry;

    fn entry(body: &str) -> Entry {
        let text = format!("---\nkb_format: 1\ntype: cmd\ntitle: T\n---\n\n```\n{body}\n```\n");
        parse_entry("x", "x.md", text.into_bytes()).entry.unwrap()
    }

    #[test]
    fn splits_text_and_variables() {
        let e = entry("echo {{env}} \\{{literal}} {{ not a var }}");
        assert_eq!(
            payload_pieces(&e),
            vec![
                PayloadPiece::Text("echo ".into()),
                PayloadPiece::Var("env".into()),
                PayloadPiece::Text(" {{literal}} {{ not a var }}\n".into()),
            ]
        );
    }

    #[test]
    fn joined_pieces_equal_the_payload_text_for_every_fixture() {
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        let mut checked = 0;
        for dir in ["valid", "search", "tui"] {
            let kb = fixtures.join(dir);
            for f in crate::walk::walk(&kb, std::slice::from_ref(&kb))
                .unwrap()
                .files
            {
                let bytes = std::fs::read(&f.abs_path).unwrap();
                let Some(e) = parse_entry(&f.id, &f.rel_path, bytes).entry else {
                    continue;
                };
                let joined: String = payload_pieces(&e)
                    .iter()
                    .map(|p| match p {
                        PayloadPiece::Text(t) => t.clone(),
                        PayloadPiece::Var(n) => format!("{{{{{n}}}}}"),
                    })
                    .collect();
                let want = e.payload().map(|p| p.text.clone()).unwrap_or_default();
                assert_eq!(joined, want, "{}", f.id);
                checked += 1;
            }
        }
        assert!(checked >= 40, "{checked}");
    }

    #[test]
    fn no_payload_no_pieces() {
        let text = "---\nkb_format: 1\ntype: link\ntitle: T\nurl: https://example.com\n---\n";
        let e = parse_entry("x", "x.md", text.as_bytes().to_vec())
            .entry
            .unwrap();
        assert!(payload_pieces(&e).is_empty());
    }
}
