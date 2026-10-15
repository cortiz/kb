//! The add form (screen 4): its state and key handling. No drawing and no I/O. What the entry is,
//! how it is named and whether it may be written is decided by `kb_core::author`; this file
//! collects what the user types and says what is wrong.

use std::ops::Range;
use std::time::SystemTime;

use kb_core::author::draft::clean_tag;
use kb_core::author::{Confidence, Draft, DraftFields, DraftSource, FixedClock, PathChoice};
use kb_core::entry::{EntryType, parse_entry};
use kb_core::lint::{Finding, RuleCode, Severity, rules::MAX_FILE_BYTES};
use kb_core::payload::payload_pieces;

use super::highlight::{self, Span as HlSpan, highlight as highlight_pieces, lang_for};
use super::input::LineEditor;
use super::keys::{ALT, CTRL, Code, Key, SHIFT};
use super::preview::{PreviewModel, build_preview};

/// How many tag completions are offered.
pub const COMPLETIONS: usize = 5;

/// The fields the focus moves through (the order of `AddState::order`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Type,
    Title,
    Tags,
    Variables,
    /// A `link`'s URL or a `code` entry's location: several inputs, one stop.
    Fields,
    Path,
    Notes,
    /// Not a stop of Tab: focused when the body holds a secret.
    Body,
}

/// What a key did to the form.
#[derive(Debug, Clone, PartialEq)]
pub enum AddEvent {
    None,
    /// The form is closed without saving.
    Close,
    /// The draft passed validation: write it.
    Save(Box<Draft>),
    /// Open this text in the editor.
    EditBody(String),
}

/// A small multi-line editor for the notes. The cursor is counted in characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextArea {
    text: String,
    cursor: usize,
}

impl TextArea {
    pub fn new(text: &str) -> TextArea {
        TextArea {
            text: text.to_string(),
            cursor: text.chars().count(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
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

    /// (row, column) of the cursor, counted in lines and characters.
    pub fn position(&self) -> (usize, usize) {
        let before: String = self.text.chars().take(self.cursor).collect();
        let row = before.matches('\n').count();
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count());
        (row, col)
    }

    fn line_starts(&self) -> Vec<usize> {
        let mut starts = vec![0];
        for (i, c) in self.text.chars().enumerate() {
            if c == '\n' {
                starts.push(i + 1);
            }
        }
        starts
    }

    pub fn home(&mut self) {
        let (row, _) = self.position();
        self.cursor = self.line_starts()[row];
    }

    pub fn end(&mut self) {
        let (row, _) = self.position();
        let starts = self.line_starts();
        self.cursor = match starts.get(row + 1) {
            Some(next) => next - 1,
            None => self.text.chars().count(),
        };
    }

    pub fn up(&mut self) {
        let (row, col) = self.position();
        if row == 0 {
            return;
        }
        let starts = self.line_starts();
        let len = starts[row] - starts[row - 1] - 1;
        self.cursor = starts[row - 1] + col.min(len);
    }

    pub fn down(&mut self) {
        let (row, col) = self.position();
        let starts = self.line_starts();
        let Some(&start) = starts.get(row + 1) else {
            return;
        };
        let len = match starts.get(row + 2) {
            Some(next) => next - start - 1,
            None => self.text.chars().count() - start,
        };
        self.cursor = start + col.min(len);
    }

    pub fn lines(&self) -> Vec<&str> {
        self.text.split('\n').collect()
    }
}

/// The result of a key on a one-line input.
pub(crate) fn line_key(editor: &mut LineEditor, key: Key) -> Option<bool> {
    let plain = key.mods & (CTRL | ALT) == 0;
    match (key.code, key.mods) {
        (Code::Char(c), _) if plain && !c.is_control() => {
            editor.insert(c);
            Some(true)
        }
        (Code::Backspace, 0) => Some(editor.backspace()),
        (Code::Delete, 0) => Some(editor.delete()),
        (Code::Left, 0) => {
            editor.left();
            Some(false)
        }
        (Code::Right, 0) => {
            editor.right();
            Some(false)
        }
        (Code::Home, 0) | (Code::Char('a'), CTRL) => {
            editor.home();
            Some(false)
        }
        (Code::End, 0) => {
            editor.end();
            Some(false)
        }
        (Code::Char('w'), CTRL) => Some(editor.delete_word()),
        (Code::Char('u'), CTRL) => Some(editor.clear()),
        _ => None,
    }
}

/// Characters a tag may hold (after lowercasing). The format allows anything; the form keeps tags
/// to what is easy to type and to search for.
fn tag_ok(tag: &str) -> bool {
    tag.chars().count() <= 40
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':'))
}

/// The tags as typed: separated by commas or spaces, cleaned, without repeats.
pub fn parse_tags(text: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for part in text.split([',', ' ', '\t']) {
        if let Some(tag) = clean_tag(part)
            && !tags.contains(&tag)
        {
            tags.push(tag);
        }
    }
    tags
}

/// The labels and starting text of the inputs a `link` or `code` entry has instead of a body.
fn type_field_rows(fields: &DraftFields) -> Vec<(&'static str, String)> {
    match fields {
        DraftFields::Link { url } => vec![("url", url.clone())],
        DraftFields::Code {
            repo,
            git_ref,
            path,
            line,
            end_line,
            symbol,
        } => vec![
            ("repo", repo.clone()),
            ("ref", git_ref.clone()),
            ("path", path.clone()),
            ("line", line.to_string()),
            (
                "end line",
                end_line.map(|n| n.to_string()).unwrap_or_default(),
            ),
            ("symbol", symbol.clone().unwrap_or_default()),
        ],
        _ => Vec::new(),
    }
}

/// Which setting of a variable the small editor under the Variables list is changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarSetting {
    Default,
    Options,
}

/// A variable setting being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarEdit {
    pub name: String,
    pub what: VarSetting,
    pub editor: LineEditor,
}

#[derive(Debug, Clone)]
pub struct AddState {
    pub draft: Draft,
    pub focus: Field,
    pub title: LineEditor,
    pub tags: LineEditor,
    pub path: LineEditor,
    pub notes: TextArea,
    /// The inputs of a `link` or `code` entry: label and editor.
    pub type_fields: Vec<(&'static str, LineEditor)>,
    pub type_row: usize,
    /// The selected row of the Variables list: declared variables, then suggestions.
    pub var_row: usize,
    pub var_edit: Option<VarEdit>,
    /// Tags in use in the KB with their counts, most used first (for completion).
    pub known_tags: Vec<(String, u32)>,
    /// Anything was edited, so Esc asks first.
    pub changed: bool,
    /// The key bar asks `Discard this entry? (y/N)`.
    pub confirm_discard: bool,
    /// The message on the key bar until the next key, and the field it is about.
    pub message: Option<String>,
    pub error: Option<(Field, String)>,
    /// Said once in the header: the clipboard was not readable, or the type changed.
    pub note: Option<String>,
    /// Byte ranges of likely secrets in the body.
    pub secret: Vec<Range<usize>>,
    pub preview: Option<PreviewModel>,
    pub body_scroll: usize,
    pub body_lines: Vec<Vec<HlSpan>>,
    now: SystemTime,
    color: bool,
}

impl AddState {
    /// Opens the form on `draft`. `unavailable` is why the clipboard could not be read, if it
    /// could not; `now` and `color` only matter for checking and for the highlighted body.
    pub fn open(
        draft: Draft,
        known_tags: Vec<(String, u32)>,
        unavailable: Option<String>,
        now: SystemTime,
        color: bool,
    ) -> AddState {
        let has_text = draft.source != DraftSource::Empty;
        let focus = if !has_text
            || unavailable.is_some()
            || draft.detection.confidence == Confidence::Low
        {
            Field::Type
        } else {
            Field::Title
        };
        let mut state = AddState {
            title: LineEditor::new(&draft.title),
            tags: LineEditor::new(&draft.tags.join(", ")),
            path: LineEditor::new(match &draft.path {
                PathChoice::Fixed(p) => p,
                PathChoice::Auto => "",
            }),
            notes: TextArea::new(&draft.notes),
            type_fields: Vec::new(),
            type_row: 0,
            var_row: 0,
            var_edit: None,
            known_tags,
            changed: false,
            confirm_discard: false,
            message: None,
            error: None,
            note: unavailable,
            secret: Vec::new(),
            preview: None,
            body_scroll: 0,
            body_lines: Vec::new(),
            focus,
            draft,
            now,
            color,
        };
        state.rebuild_type_fields();
        state.rehighlight();
        state
    }

    /// The stops of Tab, in order, for the current type.
    pub fn order(&self) -> Vec<Field> {
        let mut order = vec![Field::Type, Field::Title, Field::Tags];
        order.push(if self.draft.entry_type.has_payload() {
            Field::Variables
        } else {
            Field::Fields
        });
        order.extend([Field::Path, Field::Notes]);
        order
    }

    fn clock(&self) -> FixedClock {
        FixedClock(self.now)
    }

    fn rebuild_type_fields(&mut self) {
        self.type_fields = type_field_rows(&self.draft.fields)
            .into_iter()
            .map(|(label, text)| (label, LineEditor::new(&text)))
            .collect();
        self.type_row = self.type_row.min(self.type_fields.len().saturating_sub(1));
    }

    /// Re-parses the draft to color the body (after the body or the type changed).
    pub fn rehighlight(&mut self) {
        let ty = self.draft.entry_type;
        let bytes = self.draft.render(&self.clock());
        let entry = parse_entry("new", "new.md", bytes).entry;
        self.body_lines = match entry {
            Some(entry) if ty.has_payload() => {
                highlight_pieces(lang_for(ty), &payload_pieces(&entry), self.color)
            }
            _ => Vec::new(),
        };
        if self.body_lines.is_empty() && !self.draft.body.is_empty() && ty.has_payload() {
            self.body_lines = highlight::plain(&self.draft.body);
        }
        self.secret.clear();
    }

    /// The header line: where the text came from and what was detected.
    pub fn header(&self) -> String {
        let source = match self.draft.source {
            DraftSource::Clipboard => "from clipboard",
            DraftSource::Stdin => "from stdin",
            DraftSource::Empty => "empty",
            DraftSource::Editor => "from the editor",
        };
        let mut header = format!("New entry · {source}");
        if self.draft.source != DraftSource::Empty {
            let d = &self.draft.detection;
            let level = match d.confidence {
                Confidence::High => "high",
                Confidence::Medium => "medium",
                Confidence::Low => "low",
            };
            header.push_str(&format!(" · detected {} ({level})", d.ty.name()));
        }
        if let Some(note) = &self.note {
            header.push_str(&format!(" · {note}"));
        }
        header
    }

    pub fn tag_text_ok(&self) -> bool {
        self.invalid_tags().is_empty()
    }

    /// Tags as typed that the form will not take.
    pub fn invalid_tags(&self) -> Vec<String> {
        self.tags
            .text()
            .split([',', ' ', '\t'])
            .filter_map(clean_tag)
            .filter(|t| !tag_ok(t))
            .collect()
    }

    /// Tag completions for the word being typed, most used first, at most [`COMPLETIONS`].
    pub fn completions(&self) -> Vec<String> {
        let text = self.tags.text();
        if self.focus != Field::Tags || text.ends_with([',', ' ']) || text.is_empty() {
            return Vec::new();
        }
        let partial = text
            .rsplit([',', ' '])
            .next()
            .unwrap_or("")
            .trim_start_matches('#')
            .to_lowercase();
        if partial.is_empty() {
            return Vec::new();
        }
        self.known_tags
            .iter()
            .filter(|(tag, _)| tag.starts_with(&partial) && *tag != partial)
            .filter(|(tag, _)| !self.draft.tags.contains(tag))
            .map(|(tag, _)| tag.clone())
            .take(COMPLETIONS)
            .collect()
    }

    /// The ID the entry would get now, without looking for clashes.
    pub fn derived_path(&self) -> Option<String> {
        self.draft.target_id(&|_| false).ok()
    }

    fn edited(&mut self) {
        self.changed = true;
        self.error = None;
        self.secret.clear();
    }

    fn sync_fields(&mut self) {
        let text = |i: usize| {
            self.type_fields
                .get(i)
                .map(|(_, e)| e.text().trim().to_string())
        };
        match &self.draft.fields {
            DraftFields::Link { .. } => {
                self.draft.fields = DraftFields::Link {
                    url: text(0).unwrap_or_default(),
                };
            }
            DraftFields::Code { line, end_line, .. } => {
                let nonempty = |s: Option<String>| s.filter(|s| !s.is_empty());
                self.draft.fields = DraftFields::Code {
                    repo: text(0).unwrap_or_default(),
                    git_ref: text(1).unwrap_or_default(),
                    path: text(2).unwrap_or_default(),
                    line: text(3)
                        .and_then(|s| s.parse().ok())
                        .filter(|n| *n >= 1)
                        .unwrap_or(*line),
                    end_line: match nonempty(text(4)) {
                        Some(s) => s.parse().ok().filter(|n| *n >= 1).or(*end_line),
                        None => None,
                    },
                    symbol: nonempty(text(5)),
                };
            }
            _ => {}
        }
    }

    fn move_focus(&mut self, delta: isize) {
        let order = self.order();
        let at = order.iter().position(|f| *f == self.focus).unwrap_or(0) as isize;
        self.focus = order[(at + delta).rem_euclid(order.len() as isize) as usize];
    }

    fn fail(&mut self, field: Field, message: impl Into<String>) {
        let message = message.into();
        self.focus = field;
        self.message = Some(message.clone());
        self.error = Some((field, message));
    }

    /// The write failed in the shell (the path exists, the disk): back to the form.
    pub fn save_failed(&mut self, message: String) {
        self.fail(Field::Path, message);
    }

    /// The editor returned `new` for the body (`None`: cancelled or unchanged).
    pub fn body_edited(&mut self, new: Option<String>) {
        let Some(new) = new else { return };
        if new.len() > MAX_FILE_BYTES {
            self.message = Some(format!(
                "the body is {} bytes; entries are limited to {MAX_FILE_BYTES}",
                new.len()
            ));
            return;
        }
        let before = self.draft.entry_type;
        if self.draft.set_body(new, true) {
            self.note = Some(format!(
                "type changed from {} to {}",
                before.name(),
                self.draft.entry_type.name()
            ));
            self.rebuild_type_fields();
            if self.focus == Field::Variables && !self.draft.entry_type.has_payload() {
                self.focus = Field::Fields;
            }
        }
        self.changed = true;
        self.error = None;
        self.body_scroll = 0;
        self.rehighlight();
    }

    /// The list of declared variables shown under "Variables".
    pub fn declared(&self) -> &[kb_core::author::VarDecl] {
        &self.draft.vars
    }

    pub fn paste(&mut self, text: &str) {
        let line = text.lines().next().unwrap_or("");
        match self.focus {
            Field::Title | Field::Tags | Field::Path | Field::Fields => {
                for c in line.chars().filter(|c| !c.is_control()) {
                    self.key(Key::new(Code::Char(c), 0));
                }
            }
            Field::Notes => {
                for c in text.replace("\r\n", "\n").chars() {
                    self.key(if c == '\n' {
                        Key::new(Code::Enter, 0)
                    } else {
                        Key::new(Code::Char(c), 0)
                    });
                }
            }
            _ => {}
        }
    }

    /// Typing into a field or answering the discard question: keys are taken as they are.
    pub fn takes_raw_keys(&self) -> bool {
        self.var_edit.is_some() || self.confirm_discard
    }

    pub fn handle_key(&mut self, key: Key) -> AddEvent {
        self.message = None;
        if self.var_edit.is_some() {
            return self.var_edit_key(key);
        }
        if self.confirm_discard {
            self.confirm_discard = false;
            return if matches!(key.code, Code::Char('y' | 'Y')) && key.mods == 0 {
                AddEvent::Close
            } else {
                AddEvent::None
            };
        }
        match (key.code, key.mods) {
            (Code::Esc, 0) => {
                if self.preview.is_some() {
                    self.preview = None;
                } else if self.changed {
                    self.confirm_discard = true;
                } else {
                    return AddEvent::Close;
                }
                AddEvent::None
            }
            (Code::Char('s'), CTRL) => self.submit(),
            (Code::Char('e'), CTRL) => {
                if self.draft.entry_type.has_payload() {
                    AddEvent::EditBody(self.draft.body.clone())
                } else {
                    self.message = Some("this type has no body to edit".into());
                    AddEvent::None
                }
            }
            (Code::Char('p'), CTRL) => {
                self.toggle_preview();
                AddEvent::None
            }
            (Code::Down, SHIFT) => {
                self.body_scroll =
                    (self.body_scroll + 1).min(self.body_lines.len().saturating_sub(1));
                AddEvent::None
            }
            (Code::Up, SHIFT) => {
                self.body_scroll = self.body_scroll.saturating_sub(1);
                AddEvent::None
            }
            _ if self.preview.is_some() => AddEvent::None,
            (Code::BackTab, 0) => {
                self.move_focus(-1);
                AddEvent::None
            }
            (Code::Tab, 0) => {
                if let Some(first) = self.completions().into_iter().next() {
                    self.accept_completion(&first);
                } else {
                    self.move_focus(1);
                }
                AddEvent::None
            }
            _ => {
                self.key(key);
                AddEvent::None
            }
        }
    }

    fn toggle_preview(&mut self) {
        if self.preview.is_some() {
            self.preview = None;
            return;
        }
        let bytes = self.draft.render(&self.clock());
        let id = self.derived_path().unwrap_or_else(|| "new".into());
        self.preview = parse_entry(&id, &format!("{id}.md"), bytes)
            .entry
            .map(|entry| build_preview(&entry, None, 0, self.color));
    }

    fn accept_completion(&mut self, tag: &str) {
        let text = self.tags.text().to_string();
        let cut = text.rfind([',', ' ']).map_or(0, |i| i + 1);
        self.tags = LineEditor::new(&format!("{}{tag}, ", &text[..cut]));
        self.tags_changed();
    }

    fn tags_changed(&mut self) {
        self.draft.tags = parse_tags(self.tags.text());
        self.edited();
    }

    /// A key that edits the focused field.
    fn key(&mut self, key: Key) {
        match self.focus {
            Field::Type => {
                let all = EntryType::ALL;
                let at = all
                    .iter()
                    .position(|t| *t == self.draft.entry_type)
                    .unwrap_or(0);
                let next = match (key.code, key.mods) {
                    (Code::Left, 0) => (at + all.len() - 1) % all.len(),
                    (Code::Right, 0) => (at + 1) % all.len(),
                    _ => return,
                };
                self.draft.set_type(all[next]);
                self.rebuild_type_fields();
                self.rehighlight();
                self.edited();
            }
            Field::Title => {
                if line_key(&mut self.title, key) == Some(true) {
                    self.draft.title = kb_core::author::draft::clean_title(self.title.text());
                    self.edited();
                }
            }
            Field::Tags => {
                if line_key(&mut self.tags, key) == Some(true) {
                    self.tags_changed();
                }
            }
            Field::Path => {
                if line_key(&mut self.path, key) == Some(true) {
                    let text = self.path.text().trim().to_string();
                    self.draft.path = if text.is_empty() {
                        PathChoice::Auto
                    } else {
                        PathChoice::Fixed(text)
                    };
                    self.edited();
                }
            }
            Field::Fields => self.fields_key(key),
            Field::Notes => self.notes_key(key),
            Field::Variables => self.variables_key(key),
            Field::Body => {}
        }
    }

    /// Rows in the Variables list: declared variables, then pending suggestions.
    pub fn var_rows(&self) -> usize {
        self.draft.vars.len() + self.draft.suggestions.len()
    }

    fn variables_key(&mut self, key: Key) {
        let rows = self.var_rows();
        if rows == 0 {
            return;
        }
        self.var_row = self.var_row.min(rows - 1);
        let declared = self.draft.vars.len();
        match (key.code, key.mods) {
            (Code::Down, 0) => self.var_row = (self.var_row + 1).min(rows - 1),
            (Code::Up, 0) => self.var_row = self.var_row.saturating_sub(1),
            (Code::Char(' '), 0) if self.var_row >= declared => {
                self.draft.accept(self.var_row - declared);
                self.edited();
                self.rehighlight();
                self.var_row = self.var_row.min(self.var_rows().saturating_sub(1));
            }
            (Code::Char(c), 0) if self.var_row < declared => {
                let name = self.draft.vars[self.var_row].name.clone();
                self.variable_setting(&name, c);
            }
            _ => {}
        }
    }

    /// `k` kind, `d` default, `o` options, `l` literal, on the variable `name`.
    fn variable_setting(&mut self, name: &str, key: char) {
        use kb_core::vars::VarKind;
        let Some(var) = self.draft.vars.iter().find(|v| v.name == name).cloned() else {
            return;
        };
        match key {
            'k' => {
                let next = match var.kind {
                    VarKind::Text => VarKind::Number,
                    VarKind::Number => VarKind::Enum,
                    VarKind::Enum => VarKind::Secret,
                    VarKind::Secret => VarKind::Text,
                };
                if self.draft.set_var_kind(name, next).is_ok() {
                    self.edited();
                    if next == VarKind::Enum {
                        self.message = Some("enum: type its options, separated by commas".into());
                        self.start_edit(name, VarSetting::Options);
                    }
                }
            }
            'd' => {
                if var.kind == VarKind::Secret {
                    self.message = Some("a secret variable can't have a default".into());
                } else {
                    self.start_edit(name, VarSetting::Default);
                }
            }
            'o' => {
                if var.kind == VarKind::Enum {
                    self.start_edit(name, VarSetting::Options);
                } else {
                    self.message =
                        Some("options are for enum variables (k changes the kind)".into());
                }
            }
            'l' => {
                self.draft.mark_literal(name);
                self.edited();
                self.rehighlight();
                self.var_row = self.var_row.min(self.var_rows().saturating_sub(1));
                self.message = Some(format!("{{{{{name}}}}} is now written literally"));
            }
            _ => {}
        }
    }

    fn start_edit(&mut self, name: &str, what: VarSetting) {
        let Some(var) = self.draft.vars.iter().find(|v| v.name == name) else {
            return;
        };
        let text = match what {
            VarSetting::Default => var.default.clone().unwrap_or_default(),
            VarSetting::Options => var.options.join(", "),
        };
        self.var_edit = Some(VarEdit {
            name: name.to_string(),
            what,
            editor: LineEditor::new(&text),
        });
    }

    fn var_edit_key(&mut self, key: Key) -> AddEvent {
        let Some(edit) = self.var_edit.as_mut() else {
            return AddEvent::None;
        };
        match (key.code, key.mods) {
            (Code::Esc, 0) => self.var_edit = None,
            (Code::Enter, 0) => {
                let text = edit.editor.text().trim().to_string();
                let (name, what) = (edit.name.clone(), edit.what);
                self.var_edit = None;
                let result = match what {
                    VarSetting::Default => self
                        .draft
                        .set_var_default(&name, (!text.is_empty()).then_some(text)),
                    VarSetting::Options => self.draft.set_var_options(
                        &name,
                        text.split(',')
                            .map(|o| o.trim().to_string())
                            .filter(|o| !o.is_empty())
                            .collect(),
                    ),
                };
                match result {
                    Ok(()) => self.edited(),
                    Err(e) => self.message = Some(e.to_string()),
                }
            }
            _ => {
                line_key(&mut edit.editor, key);
            }
        }
        AddEvent::None
    }

    fn fields_key(&mut self, key: Key) {
        let n = self.type_fields.len();
        if n == 0 {
            return;
        }
        match (key.code, key.mods) {
            (Code::Down, 0) => self.type_row = (self.type_row + 1).min(n - 1),
            (Code::Up, 0) => self.type_row = self.type_row.saturating_sub(1),
            _ => {
                let row = self.type_row;
                if line_key(&mut self.type_fields[row].1, key) == Some(true) {
                    self.sync_fields();
                    self.edited();
                }
            }
        }
    }

    fn notes_key(&mut self, key: Key) {
        let plain = key.mods & (CTRL | ALT) == 0;
        let changed = match (key.code, key.mods) {
            (Code::Char(c), _) if plain && !c.is_control() => {
                self.notes.insert(c);
                true
            }
            (Code::Enter, 0) => {
                self.notes.insert('\n');
                true
            }
            (Code::Backspace, 0) => self.notes.backspace(),
            (Code::Delete, 0) => self.notes.delete(),
            (Code::Left, 0) => {
                self.notes.left();
                false
            }
            (Code::Right, 0) => {
                self.notes.right();
                false
            }
            (Code::Up, 0) => {
                self.notes.up();
                false
            }
            (Code::Down, 0) => {
                self.notes.down();
                false
            }
            (Code::Home, 0) => {
                self.notes.home();
                false
            }
            (Code::End, 0) => {
                self.notes.end();
                false
            }
            _ => false,
        };
        if changed {
            self.draft.notes = self.notes.text().to_string();
            self.edited();
        }
    }

    /// Ctrl-S: checks everything the write would check and either hands the draft out or says
    /// what is wrong and focuses it. There is no "save anyway" here.
    fn submit(&mut self) -> AddEvent {
        if self.draft.title.trim().is_empty() {
            self.fail(Field::Title, "a title is required");
            return AddEvent::None;
        }
        if let Some(bad) = self.invalid_tags().first() {
            self.fail(
                Field::Tags,
                format!("tag \"{bad}\": use letters, digits, - _ . / : (40 at most)"),
            );
            return AddEvent::None;
        }
        let id = match self.draft.target_id(&|_| false) {
            Ok(id) => id,
            Err(e) => {
                self.fail(Field::Path, e.to_string());
                return AddEvent::None;
            }
        };
        let check = self.draft.check(&id, &self.clock());
        if check.too_large {
            self.fail(
                Field::Body,
                format!(
                    "the entry is {} bytes; entries are limited to {MAX_FILE_BYTES}",
                    check.bytes.len()
                ),
            );
            return AddEvent::None;
        }
        if let Some(finding) = check.secrets().first() {
            let finding = (*finding).clone();
            self.explain_secret(&finding);
            return AddEvent::None;
        }
        if let Some(finding) = check.errors().first() {
            let (field, text) = (
                field_of(finding.code),
                format!("{} {}", finding.code, finding.message),
            );
            self.fail(field, text);
            return AddEvent::None;
        }
        AddEvent::Save(Box::new(self.draft.clone()))
    }

    /// Focuses the field that holds the secret and marks it in the body.
    fn explain_secret(&mut self, finding: &Finding) {
        let text = format!("{} {}", finding.code, finding.message);
        let scan = |s: &str| kb_core::lint::secrets::scan_secrets(s).hits;
        let title_hits = scan(&self.draft.title);
        let notes_hits = scan(&self.draft.notes);
        let body_hits = scan(&self.draft.body);
        if !body_hits.is_empty() {
            self.secret = body_hits
                .iter()
                .map(|h| h.offset..token_end(&self.draft.body, h.offset))
                .collect();
            self.fail(Field::Body, text);
        } else if !title_hits.is_empty() {
            self.fail(Field::Title, text);
        } else if !notes_hits.is_empty() {
            self.fail(Field::Notes, text);
        } else {
            self.fail(Field::Fields, text);
        }
    }

    /// The first finding as the key bar shows it.
    pub fn finding_text(finding: &Finding) -> String {
        let severity = match finding.severity {
            Severity::Error => "",
            Severity::Warning => "warning ",
            Severity::Info => "info ",
        };
        format!("{severity}{} {}", finding.code, finding.message)
    }
}

/// The end of the token that starts at `offset`: up to a space or quote.
fn token_end(text: &str, offset: usize) -> usize {
    text[offset..]
        .find(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | '`' | ';' | ','))
        .map_or(text.len(), |n| offset + n.max(1))
}

/// The field a lint rule is about.
fn field_of(code: RuleCode) -> Field {
    match code {
        RuleCode::MissingTitle | RuleCode::InvalidTitle => Field::Title,
        RuleCode::InvalidTag | RuleCode::DuplicateTag => Field::Tags,
        RuleCode::MissingUrl
        | RuleCode::InvalidUrl
        | RuleCode::MissingCodeRef
        | RuleCode::InvalidCodeRef => Field::Fields,
        RuleCode::MissingPayload | RuleCode::EmptyPayload => Field::Body,
        c if (40..=49).contains(&c.number()) => Field::Variables,
        _ => Field::Title,
    }
}

/// `text`, `number`, `enum` or `secret`.
pub fn kind_name(kind: kb_core::vars::VarKind) -> &'static str {
    kb_core::vars::VarKind::NAMES[kind as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use kb_core::author::{DraftOptions, DraftSource};

    const CW: &str =
        "fields @timestamp, @message\n| filter @message like /{{fn}}/\n| stats count() by bin(5m)";

    fn open_with(text: &str, source: DraftSource, unavailable: Option<&str>) -> AddState {
        let draft = Draft::from_text(text, source, &DraftOptions::default());
        let tags = vec![
            ("lambda".into(), 5),
            ("laptop".into(), 2),
            ("auth".into(), 1),
        ];
        AddState::open(
            draft,
            tags,
            unavailable.map(str::to_string),
            SystemTime::UNIX_EPOCH,
            false,
        )
    }

    fn press(s: &mut AddState, code: Code, mods: u8) -> AddEvent {
        s.handle_key(Key::new(code, mods))
    }

    fn type_text(s: &mut AddState, text: &str) {
        for c in text.chars() {
            press(s, Code::Char(c), 0);
        }
    }

    #[test]
    fn initial_focus_follows_what_was_found() {
        assert_eq!(
            open_with(CW, DraftSource::Clipboard, None).focus,
            Field::Title
        );
        assert_eq!(open_with("", DraftSource::Empty, None).focus, Field::Type);
        assert_eq!(
            open_with(
                "",
                DraftSource::Empty,
                Some("clipboard not readable over SSH")
            )
            .focus,
            Field::Type
        );
        assert_eq!(
            open_with("remember the milk", DraftSource::Clipboard, None).focus,
            Field::Type
        );
        let s = open_with(
            "",
            DraftSource::Empty,
            Some("clipboard not readable over SSH"),
        );
        assert!(s.header().contains("clipboard not readable over SSH"));
        assert!(
            open_with(CW, DraftSource::Clipboard, None)
                .header()
                .contains("detected cw (high)")
        );
    }

    #[test]
    fn tab_and_shift_tab_walk_the_fields_in_order() {
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        s.focus = Field::Type;
        let mut seen = vec![s.focus];
        for _ in 0..5 {
            press(&mut s, Code::Tab, 0);
            seen.push(s.focus);
        }
        assert_eq!(
            seen,
            [
                Field::Type,
                Field::Title,
                Field::Tags,
                Field::Variables,
                Field::Path,
                Field::Notes
            ]
        );
        press(&mut s, Code::Tab, 0);
        assert_eq!(s.focus, Field::Type);
        press(&mut s, Code::BackTab, 0);
        assert_eq!(s.focus, Field::Notes);
    }

    #[test]
    fn arrows_change_the_type_and_drop_inapplicable_fields() {
        let mut s = open_with("https://example.com/x", DraftSource::Clipboard, None);
        s.focus = Field::Type;
        assert_eq!(s.draft.entry_type, EntryType::Link);
        assert_eq!(s.type_fields.len(), 1);
        press(&mut s, Code::Right, 0);
        assert_eq!(s.draft.entry_type, EntryType::Code);
        assert_eq!(s.type_fields.len(), 6);
        press(&mut s, Code::Left, 0);
        press(&mut s, Code::Left, 0);
        assert_eq!(s.draft.entry_type, EntryType::Prompt);
        assert!(s.type_fields.is_empty());
        assert_eq!(s.focus, Field::Type);
        assert!(s.changed);
    }

    #[test]
    fn typing_edits_title_tags_path_and_notes() {
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        type_text(&mut s, "P99");
        assert_eq!(s.draft.title, "P99");
        press(&mut s, Code::Tab, 0);
        type_text(&mut s, "Auth, x");
        assert_eq!(s.draft.tags, ["auth", "x"]);
        s.focus = Field::Path;
        type_text(&mut s, "mine/p99");
        assert_eq!(s.draft.path, PathChoice::Fixed("mine/p99".into()));
        for _ in 0..8 {
            press(&mut s, Code::Backspace, 0);
        }
        assert_eq!(s.draft.path, PathChoice::Auto);
        s.focus = Field::Notes;
        type_text(&mut s, "one");
        press(&mut s, Code::Enter, 0);
        type_text(&mut s, "two");
        assert_eq!(s.draft.notes, "one\ntwo");
    }

    #[test]
    fn tag_completion_lists_five_by_frequency_and_tab_takes_the_first() {
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        s.focus = Field::Tags;
        s.tags = LineEditor::new("");
        type_text(&mut s, "la");
        assert_eq!(s.completions(), ["lambda", "laptop"]);
        press(&mut s, Code::Tab, 0);
        assert_eq!(s.tags.text(), "lambda, ");
        assert_eq!(s.draft.tags, ["lambda"]);
        assert_eq!(s.focus, Field::Tags, "Tab took the completion and stayed");
        press(&mut s, Code::Tab, 0);
        assert_eq!(s.focus, Field::Variables, "no partial tag: Tab moves on");
        let many: Vec<(String, u32)> = (0..9).map(|i| (format!("t{i}"), 9 - i)).collect();
        s.known_tags = many;
        s.focus = Field::Tags;
        s.tags = LineEditor::new("t");
        assert_eq!(s.completions().len(), COMPLETIONS);
    }

    #[test]
    fn invalid_tags_are_marked_and_block_saving() {
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        s.focus = Field::Tags;
        type_text(&mut s, "ok, bad{{x}}");
        assert_eq!(s.invalid_tags().len(), 1);
        s.draft.title = "T".into();
        assert_eq!(press(&mut s, Code::Char('s'), CTRL), AddEvent::None);
        assert_eq!(s.focus, Field::Tags);
        assert!(s.message.as_deref().unwrap().contains("bad"));
    }

    #[test]
    fn escape_closes_when_unchanged_and_asks_when_changed() {
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        assert!(!s.changed);
        assert_eq!(press(&mut s, Code::Esc, 0), AddEvent::Close);
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        type_text(&mut s, "x");
        assert_eq!(press(&mut s, Code::Esc, 0), AddEvent::None);
        assert!(s.confirm_discard);
        assert_eq!(press(&mut s, Code::Char('n'), 0), AddEvent::None);
        assert!(!s.confirm_discard);
        press(&mut s, Code::Esc, 0);
        assert_eq!(press(&mut s, Code::Char('y'), 0), AddEvent::Close);
        // anything but y cancels
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        type_text(&mut s, "x");
        press(&mut s, Code::Esc, 0);
        assert_eq!(press(&mut s, Code::Enter, 0), AddEvent::None);
        assert!(!s.confirm_discard);
    }

    #[test]
    fn saving_needs_a_title_and_a_valid_entry_and_hands_out_the_draft() {
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        s.focus = Field::Notes;
        assert_eq!(press(&mut s, Code::Char('s'), CTRL), AddEvent::None);
        assert_eq!(
            (s.focus, s.message.as_deref()),
            (Field::Title, Some("a title is required"))
        );
        type_text(&mut s, "Fine");
        let AddEvent::Save(draft) = press(&mut s, Code::Char('s'), CTRL) else {
            panic!()
        };
        assert_eq!(draft.title, "Fine");
        // a path no title can make
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        type_text(&mut s, "¿¿");
        assert_eq!(press(&mut s, Code::Char('s'), CTRL), AddEvent::None);
        assert_eq!(s.focus, Field::Path);
    }

    #[test]
    fn a_secret_focuses_the_body_and_marks_it_with_no_way_to_save_anyway() {
        let mut s = open_with(
            "aws s3 ls # AKIAIOSFODNN7EXAMPLE\nkubectl get pods",
            DraftSource::Clipboard,
            None,
        );
        s.focus = Field::Title;
        type_text(&mut s, "Leaky");
        assert_eq!(press(&mut s, Code::Char('s'), CTRL), AddEvent::None);
        assert_eq!(s.focus, Field::Body);
        assert_eq!(s.secret.len(), 1);
        assert_eq!(&s.draft.body[s.secret[0].clone()], "AKIAIOSFODNN7EXAMPLE");
        assert!(s.message.as_deref().unwrap().starts_with("KB060"));
        assert_eq!(
            press(&mut s, Code::Char('s'), CTRL),
            AddEvent::None,
            "still refused"
        );
    }

    #[test]
    fn ctrl_e_asks_for_the_editor_and_a_returned_body_reruns_detection() {
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        let AddEvent::EditBody(text) = press(&mut s, Code::Char('e'), CTRL) else {
            panic!()
        };
        assert_eq!(text, s.draft.body);
        s.body_edited(Some("select 1 from t;".into()));
        assert_eq!(s.draft.entry_type, EntryType::Sql);
        assert!(s.changed && s.note.as_deref().unwrap().contains("cw to sql"));
        let before = s.draft.body.clone();
        s.body_edited(None);
        assert_eq!(s.draft.body, before);
        s.body_edited(Some("x".repeat(MAX_FILE_BYTES + 1)));
        assert_eq!(s.draft.body, before);
        assert!(s.message.as_deref().unwrap().contains("limited to"));
    }

    #[test]
    fn preview_toggles_and_escape_leaves_it() {
        let mut s = open_with(CW, DraftSource::Clipboard, None);
        press(&mut s, Code::Char('p'), CTRL);
        assert!(s.preview.is_some());
        assert_eq!(press(&mut s, Code::Esc, 0), AddEvent::None);
        assert!(s.preview.is_none());
        press(&mut s, Code::Char('p'), CTRL);
        press(&mut s, Code::Char('p'), CTRL);
        assert!(s.preview.is_none());
    }

    #[test]
    fn text_area_moves_by_line() {
        let mut t = TextArea::new("ab\ncde\nf");
        t.up();
        assert_eq!(t.position(), (1, 1));
        t.up();
        assert_eq!(t.position(), (0, 1));
        t.end();
        t.down();
        assert_eq!(t.position(), (1, 2));
        t.home();
        assert_eq!(t.position(), (1, 0));
        assert!(t.backspace());
        assert_eq!(t.text(), "abcde\nf");
    }

    #[test]
    fn suggestions_appear_under_the_variables_and_space_accepts_one() {
        let mut s = open_with(
            "_sourceCategory=prod/x | timeslice 5m",
            DraftSource::Clipboard,
            None,
        );
        s.focus = Field::Variables;
        assert_eq!(s.draft.suggestions.len(), 2);
        assert_eq!(s.var_rows(), 2);
        let before = s.draft.body.clone();
        press(&mut s, Code::Down, 0);
        // nothing changes without Space
        assert_eq!(s.draft.body, before);
        assert!(!s.changed);
        press(&mut s, Code::Char(' '), 0);
        assert_eq!(
            s.draft.body,
            "_sourceCategory=prod/x | timeslice {{minutes}}m"
        );
        assert!(s.changed);
        assert_eq!(s.draft.suggestions.len(), 1);
        assert_eq!(s.draft.vars[0].name, "minutes");
        assert!(
            s.body_lines
                .iter()
                .flatten()
                .any(|sp| sp.text.contains("{{minutes}}"))
        );
    }

    #[test]
    fn variable_rows_change_kind_default_options_and_literal() {
        use kb_core::vars::VarKind;
        let mut s = open_with("echo {{who}} {{token2}}", DraftSource::Clipboard, None);
        s.focus = Field::Variables;
        press(&mut s, Code::Char('k'), 0);
        assert_eq!(s.draft.vars[0].kind, VarKind::Number);
        press(&mut s, Code::Char('k'), 0);
        assert_eq!(s.draft.vars[0].kind, VarKind::Enum);
        assert!(s.var_edit.is_some(), "an enum asks for its options");
        type_text(&mut s, "a, b");
        press(&mut s, Code::Enter, 0);
        assert_eq!(s.draft.vars[0].options, ["a", "b"]);
        press(&mut s, Code::Char('d'), 0);
        type_text(&mut s, "c");
        press(&mut s, Code::Enter, 0);
        assert!(s.message.as_deref().unwrap().contains("one of the options"));
        press(&mut s, Code::Char('d'), 0);
        press(&mut s, Code::Char('u'), CTRL);
        type_text(&mut s, "b");
        press(&mut s, Code::Enter, 0);
        assert_eq!(s.draft.vars[0].default.as_deref(), Some("b"));
        press(&mut s, Code::Char('k'), 0);
        assert_eq!(s.draft.vars[0].kind, VarKind::Secret);
        press(&mut s, Code::Char('d'), 0);
        assert!(s.var_edit.is_none());
        assert!(s.message.as_deref().unwrap().contains("secret"));
        press(&mut s, Code::Char('o'), 0);
        assert!(s.message.as_deref().unwrap().contains("enum"));
        press(&mut s, Code::Char('l'), 0);
        assert_eq!(s.draft.body, "echo \\{{who}} {{token2}}");
        assert_eq!(s.draft.vars.len(), 1);
        // Esc leaves the small editor, not the form
        press(&mut s, Code::Char('d'), 0);
        assert_eq!(press(&mut s, Code::Esc, 0), AddEvent::None);
        assert!(s.var_edit.is_none() && !s.confirm_discard);
    }
}
