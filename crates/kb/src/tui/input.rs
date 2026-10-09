//! The search line: free text with a cursor, and the filter chips next to it.

use kb_core::entry::EntryType;
use kb_core::fold::fold;
use kb_core::query::Field;

/// A filter shown as a chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chip {
    Type(EntryType),
    Tag(String),
    NotTag(String),
    Var(String),
    Repo(String),
    In(Field),
}

impl Chip {
    /// The query syntax of the chip (`type:cw`, `-tag:stage`, ...); also its label.
    pub fn syntax(&self) -> String {
        match self {
            Chip::Type(t) => format!("type:{}", t.name()),
            Chip::Tag(t) => format!("tag:{t}"),
            Chip::NotTag(t) => format!("-tag:{t}"),
            Chip::Var(v) => format!("var:{v}"),
            Chip::Repo(r) => format!("repo:{r}"),
            Chip::In(f) => format!("in:{}", f.name()),
        }
    }

    /// The chip a typed token stands for, if it is a valid filter.
    pub fn parse(token: &str) -> Option<Chip> {
        let (key, value) = token.split_once(':')?;
        if value.is_empty() {
            return None;
        }
        let tag = |v: &str| v.trim_start_matches('#').to_lowercase();
        match key.to_lowercase().as_str() {
            "type" => EntryType::from_name(&fold(value)).map(Chip::Type),
            "tag" => Some(Chip::Tag(tag(value))),
            "-tag" => Some(Chip::NotTag(tag(value))),
            "var" => Some(Chip::Var(value.to_lowercase())),
            "repo" => Some(Chip::Repo(fold(value))),
            "in" => match fold(value).as_str() {
                "title" => Some(Chip::In(Field::Title)),
                "payload" => Some(Chip::In(Field::Payload)),
                "notes" => Some(Chip::In(Field::Notes)),
                _ => None,
            },
            _ => None,
        }
    }
}

/// A single line of text with a cursor counted in characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineEditor {
    text: String,
    cursor: usize,
}

impl LineEditor {
    pub fn new(text: &str) -> LineEditor {
        LineEditor {
            text: text.to_string(),
            cursor: text.chars().count(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn byte(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map_or(self.text.len(), |(i, _)| i)
    }

    pub fn insert(&mut self, c: char) {
        let at = self.byte(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
    }

    /// Deletes the character before the cursor; returns whether anything changed.
    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let at = self.byte(self.cursor - 1);
        self.text.remove(at);
        self.cursor -= 1;
        true
    }

    pub fn delete(&mut self) -> bool {
        if self.cursor >= self.text.chars().count() {
            return false;
        }
        let at = self.byte(self.cursor);
        self.text.remove(at);
        true
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.text.chars().count());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.chars().count();
    }

    /// Ctrl-W: delete back over spaces, then over the word before them.
    pub fn delete_word(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let mut start = self.cursor;
        while start > 0 && chars[start - 1] == ' ' {
            start -= 1;
        }
        while start > 0 && chars[start - 1] != ' ' {
            start -= 1;
        }
        let from = self.byte(start);
        let to = self.byte(self.cursor);
        self.text.replace_range(from..to, "");
        self.cursor = start;
        true
    }

    pub fn clear(&mut self) -> bool {
        let had = !self.text.is_empty();
        self.text.clear();
        self.cursor = 0;
        had
    }

    /// Removes the characters `from..to` (character indexes) and puts the cursor at `from`.
    fn remove_range(&mut self, from: usize, to: usize) {
        let (a, b) = (self.byte(from), self.byte(to));
        self.text.replace_range(a..b, "");
        self.cursor = from;
    }
}

/// What a key did to the search input, so the caller knows whether to search again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    None,
    Text,
    Chips,
}

/// The text line and the chips together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchInput {
    pub editor: LineEditor,
    pub chips: Vec<Chip>,
}

impl SearchInput {
    pub fn new(initial: &str) -> SearchInput {
        SearchInput {
            editor: LineEditor::new(initial),
            chips: Vec::new(),
        }
    }

    /// Input holding `initial`, with filter tokens (`type:cw`, `-tag:stage`) as chips, as if the
    /// user had typed it.
    pub fn from_text(initial: &str) -> SearchInput {
        let mut input = SearchInput::new("");
        for c in initial.chars() {
            input.insert(c);
        }
        // a trailing space turns a final filter token into a chip; keep it only if it stayed text
        if input.insert(' ') == Change::Text {
            input.editor.backspace();
        }
        input
    }

    /// Types a character. A space after a complete filter token (`tag:prod `) turns the token
    /// into a chip, unless the token is inside an open quote.
    pub fn insert(&mut self, c: char) -> Change {
        self.editor.insert(c);
        if c != ' ' {
            return Change::Text;
        }
        let chars: Vec<char> = self.editor.text().chars().collect();
        let space = self.editor.cursor() - 1;
        let mut start = space;
        while start > 0 && chars[start - 1] != ' ' {
            start -= 1;
        }
        let token: String = chars[start..space].iter().collect();
        let quotes = chars[..start].iter().filter(|c| **c == '"').count();
        if token.is_empty() || quotes % 2 == 1 {
            return Change::Text;
        }
        match Chip::parse(&token) {
            Some(chip) => {
                self.editor.remove_range(start, space + 1);
                self.chips.push(chip);
                Change::Chips
            }
            None => Change::Text,
        }
    }

    /// Backspace: removes the last chip when the text is empty, otherwise one character.
    pub fn backspace(&mut self) -> Change {
        if self.editor.is_empty() {
            return if self.chips.pop().is_some() {
                Change::Chips
            } else {
                Change::None
            };
        }
        if self.editor.backspace() {
            Change::Text
        } else {
            Change::None
        }
    }

    pub fn clear_all(&mut self) -> bool {
        let had = !self.editor.is_empty() || !self.chips.is_empty();
        self.editor.clear();
        self.chips.clear();
        had
    }

    pub fn is_blank(&self) -> bool {
        self.editor.is_empty() && self.chips.is_empty()
    }

    pub fn type_chips(&self) -> Vec<EntryType> {
        self.chips
            .iter()
            .filter_map(|c| match c {
                Chip::Type(t) => Some(*t),
                _ => None,
            })
            .collect()
    }

    /// Replaces the whole type group by one type (or none, for "all").
    pub fn set_type(&mut self, ty: Option<EntryType>) {
        self.chips.retain(|c| !matches!(c, Chip::Type(_)));
        if let Some(t) = ty {
            self.chips.insert(0, Chip::Type(t));
        }
    }

    /// The text `Query::parse` should see: chips as filter syntax, then the free text.
    pub fn query_string(&self) -> String {
        let mut parts: Vec<String> = self.chips.iter().map(Chip::syntax).collect();
        let text = self.editor.text().trim();
        if !text.is_empty() {
            parts.push(text.to_string());
        }
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kb_core::query::Query;

    fn typed(input: &mut SearchInput, s: &str) {
        for c in s.chars() {
            input.insert(c);
        }
    }

    #[test]
    fn editing_keeps_the_cursor_on_character_boundaries() {
        for s in ["héllo", "日本語", "a🚀b", "e\u{301}x"] {
            let n = s.chars().count();
            let mut e = LineEditor::new(s);
            assert_eq!(e.cursor(), n);
            e.left();
            e.left();
            e.insert('Z');
            assert_eq!(e.text().chars().count(), n + 1, "{s}");
            assert!(e.backspace());
            assert_eq!(e.text(), s);
            e.home();
            assert!(!e.backspace());
            assert!(e.delete());
            assert_eq!(e.text().chars().count(), n - 1, "{s}");
            e.end();
            assert_eq!(e.cursor(), n - 1);
            e.right();
            assert_eq!(e.cursor(), n - 1);
        }
    }

    #[test]
    fn word_deletion_and_clearing() {
        let mut e = LineEditor::new("foo bar  ");
        assert!(e.delete_word());
        assert_eq!(e.text(), "foo ");
        assert!(e.delete_word());
        assert_eq!(e.text(), "");
        assert!(!e.delete_word());
        let mut e = LineEditor::new("日本 語彙");
        e.delete_word();
        assert_eq!(e.text(), "日本 ");
        let mut e = LineEditor::new("abc def");
        e.left();
        e.left();
        e.left();
        e.delete_word();
        assert_eq!(e.text(), "def");
        assert_eq!(e.cursor(), 0);
        assert!(e.clear() && e.text().is_empty() && !e.clear());
    }

    #[test]
    fn a_space_after_a_filter_makes_a_chip() {
        for (typed_text, chip) in [
            ("tag:prod ", Chip::Tag("prod".into())),
            ("-tag:stage ", Chip::NotTag("stage".into())),
            ("type:sql ", Chip::Type(EntryType::Sql)),
            ("var:region ", Chip::Var("region".into())),
            ("repo:acme/api ", Chip::Repo("acme/api".into())),
            ("in:title ", Chip::In(Field::Title)),
            ("TAG:#Prod ", Chip::Tag("prod".into())),
        ] {
            let mut i = SearchInput::new("");
            typed(&mut i, typed_text);
            assert_eq!(i.chips, vec![chip], "{typed_text}");
            assert_eq!(i.editor.text(), "", "{typed_text}");
        }
        let mut i = SearchInput::new("");
        typed(&mut i, "type:sql type:cw ");
        assert_eq!(
            i.chips,
            vec![Chip::Type(EntryType::Sql), Chip::Type(EntryType::Cw)]
        );
        let mut i = SearchInput::new("");
        typed(&mut i, "foo tag:prod bar");
        assert_eq!(i.chips, vec![Chip::Tag("prod".into())]);
        assert_eq!(i.editor.text(), "foo bar");
        typed(&mut i, " ");
        assert_eq!(
            i.chips.len(),
            1,
            "only the token before the new space is considered"
        );
    }

    #[test]
    fn invalid_or_quoted_tokens_stay_text() {
        for s in [
            "type:nonsense ",
            "tag: ",
            "in:other ",
            "foo:bar ",
            "\"say tag:prod ",
        ] {
            let mut i = SearchInput::new("");
            typed(&mut i, s);
            assert!(i.chips.is_empty(), "{s}");
            assert_eq!(i.editor.text(), s);
        }
        let mut i = SearchInput::new("foo ");
        typed(&mut i, "tag:prod ");
        assert_eq!(i.chips, vec![Chip::Tag("prod".into())]);
        assert_eq!(i.editor.text(), "foo ");
    }

    #[test]
    fn backspace_removes_chips_only_on_an_empty_line() {
        let mut i = SearchInput::new("");
        typed(&mut i, "tag:a tag:b x");
        assert_eq!(i.backspace(), Change::Text);
        assert_eq!(i.chips.len(), 2);
        assert_eq!(i.backspace(), Change::Chips);
        assert_eq!(i.chips, vec![Chip::Tag("a".into())]);
        assert_eq!(i.backspace(), Change::Chips);
        assert_eq!(i.backspace(), Change::None);
        typed(&mut i, "tag:z ");
        assert!(i.clear_all() && i.is_blank() && !i.clear_all());
    }

    #[test]
    fn the_query_string_equals_the_cli_arguments() {
        let mut i = SearchInput::new("");
        typed(&mut i, "type:cw tag:prod latency");
        assert_eq!(i.query_string(), "type:cw tag:prod latency");
        let q = Query::parse(&i.query_string());
        let want =
            Query::from_parts("latency", &["cw".into()], &["prod".into()], &[], None).unwrap();
        assert_eq!(q, want);
        let mut j = SearchInput::new("role cache");
        j.chips.push(Chip::NotTag("stage".into()));
        assert_eq!(j.query_string(), "-tag:stage role cache");
        assert_eq!(SearchInput::new("  ").query_string(), "");
    }

    #[test]
    fn the_type_group_is_replaced_as_one() {
        let mut i = SearchInput::new("");
        typed(&mut i, "tag:x type:sql type:cw ");
        assert_eq!(i.type_chips(), vec![EntryType::Sql, EntryType::Cw]);
        i.set_type(Some(EntryType::Link));
        assert_eq!(i.type_chips(), vec![EntryType::Link]);
        assert_eq!(i.chips.len(), 2);
        i.set_type(None);
        assert!(i.type_chips().is_empty() && i.chips == vec![Chip::Tag("x".into())]);
    }

    #[test]
    fn initial_text_gets_its_filters_as_chips() {
        let i = SearchInput::from_text("type:cw tag:prod latency");
        assert_eq!(
            i.chips,
            vec![Chip::Type(EntryType::Cw), Chip::Tag("prod".into())]
        );
        assert_eq!(i.editor.text(), "latency");
        assert_eq!(i.editor.cursor(), 7);
        let i = SearchInput::from_text("latency -tag:stage");
        assert_eq!(i.chips, vec![Chip::NotTag("stage".into())]);
        assert_eq!(i.editor.text(), "latency ");
        let i = SearchInput::from_text("5xx auth");
        assert!(i.chips.is_empty());
        assert_eq!(i.editor.text(), "5xx auth");
        let i = SearchInput::from_text("");
        assert!(i.is_blank());
        let i = SearchInput::from_text("\"say tag:prod");
        assert!(i.chips.is_empty() && i.editor.text() == "\"say tag:prod");
        assert_eq!(
            SearchInput::from_text("type:cw latency").query_string(),
            "type:cw latency"
        );
    }
}
