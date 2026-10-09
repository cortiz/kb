//! Case and accent folding, and word splitting that matches SQLite's `unicode61` tokenizer.

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Lowercases `s` and removes accents (`Configuración` → `configuracion`).
pub fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// One folded word and where it sits in the original text, in characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || is_combining_mark(c)
}

/// Splits `s` at every character that is not a letter or digit.
pub fn words(s: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut buf = String::new();
    let mut count = 0;
    for (i, c) in s.chars().enumerate() {
        count = i + 1;
        if is_word_char(c) {
            start.get_or_insert(i);
            buf.push(c);
        } else if let Some(st) = start.take() {
            out.push(Word {
                text: fold(&buf),
                start: st,
                end: i,
            });
            buf.clear();
        }
    }
    if let Some(st) = start {
        out.push(Word {
            text: fold(&buf),
            start: st,
            end: count,
        });
    }
    out.retain(|w| !w.text.is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case_and_accents() {
        assert_eq!(fold("Configuración"), "configuracion");
        assert_eq!(fold("ÀÉÎõü"), "aeiou");
        assert_eq!(fold("ß"), "ß");
        assert_eq!(fold(&fold("Ñandú")), fold("Ñandú"));
    }

    #[test]
    fn splits_words_with_character_offsets() {
        let w = words("5xx-by-authorizer");
        let got: Vec<_> = w
            .iter()
            .map(|w| (w.text.as_str(), w.start, w.end))
            .collect();
        assert_eq!(
            got,
            vec![("5xx", 0, 3), ("by", 4, 6), ("authorizer", 7, 17)]
        );
        let w = words("año nuevo");
        let got: Vec<_> = w
            .iter()
            .map(|w| (w.text.as_str(), w.start, w.end))
            .collect();
        assert_eq!(got, vec![("ano", 0, 3), ("nuevo", 4, 9)]);
        assert!(words("???").is_empty());
    }
}
