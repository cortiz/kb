//! The variable form (screen 2): its state and key handling. No drawing and no I/O; the engine in
//! `kb_core::varres` decides what a value means and what the result is.

use std::collections::BTreeMap;
use std::fmt;

use kb_core::search::Kb;
use kb_core::varres::{self, Binding, Resolved, Source, Value, Values, VarPlan};
use kb_core::vars::{VarKind, Variable};

use super::input::LineEditor;
use super::keys::{ALT, CTRL, Code, Key};

/// What happens after the form is submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    /// Leave the UI; the caller copies or prints (Enter).
    Exit,
    /// Copy and stay in the UI (Ctrl-Y).
    Stay,
    /// Open the primary link or console and stay in the UI (Ctrl-O).
    Open,
}

/// Everything the form needs, read from the entry and the saved state. Built outside `update`.
#[derive(Debug, Clone)]
pub struct FormSpec {
    pub id: String,
    pub title: String,
    pub plan: VarPlan,
    pub bindings: Vec<Binding>,
    /// Recent values by variable name, for suggestions.
    pub recent: BTreeMap<String, Vec<String>>,
}

impl FormSpec {
    /// The form for `id`, or `None` when the entry has no variables.
    pub fn load(
        kb: &Kb,
        id: &str,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Option<FormSpec>, String> {
        let entry = kb.entry(id).map_err(|e| e.to_string())?;
        let plan = varres::plan_vars(&entry);
        if plan.is_empty() {
            return Ok(None);
        }
        let (memory, recent) = kb.remembered(id);
        let bindings = varres::prefill(
            &plan,
            &varres::Inputs {
                flags: &[],
                env,
                memory: Some(&memory),
            },
        );
        Ok(Some(FormSpec {
            id: id.to_string(),
            title: entry.title().unwrap_or(id).to_string(),
            plan,
            bindings,
            recent,
        }))
    }
}

/// One variable, as the form edits it.
#[derive(Clone)]
pub struct Field {
    pub var: Variable,
    /// Where the current value came from (changes to `Typed` once the user edits it).
    pub source: Source,
    prefill: Option<Value>,
    prefill_source: Source,
    /// The selected option, for an enum.
    pub choice: Option<usize>,
    pub editor: LineEditor,
    /// A secret taken from the environment: shown masked and not editable until Ctrl-R.
    pub locked: bool,
    pub error: Option<String>,
}

impl fmt::Debug for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = self.value();
        f.debug_struct("Field")
            .field("name", &self.var.name)
            .field("value", &shown)
            .field("source", &self.source)
            .finish()
    }
}

impl Field {
    fn new(var: &Variable, binding: &Binding) -> Field {
        let secret = var.kind == VarKind::Secret;
        let locked = secret && binding.source == Source::Env && binding.value.is_some();
        let mut field = Field {
            var: var.clone(),
            source: binding.source,
            prefill: binding.value.clone(),
            prefill_source: binding.source,
            choice: None,
            editor: LineEditor::default(),
            locked,
            error: None,
        };
        if let Some(v) = binding.value.as_ref().filter(|_| !locked) {
            field.set(v.reveal());
        }
        field
    }

    pub fn secret(&self) -> bool {
        self.var.kind == VarKind::Secret
    }

    /// Puts `text` in the field. An enum takes it only when it is one of its options.
    fn set(&mut self, text: &str) {
        if self.var.kind == VarKind::Enum {
            self.choice = self.var.options.iter().position(|o| o == text);
        } else {
            self.editor = LineEditor::new(text);
        }
    }

    /// The value as it stands. `None` for an enum with nothing chosen.
    pub fn value(&self) -> Option<Value> {
        if self.locked {
            return self.prefill.clone();
        }
        match self.var.kind {
            VarKind::Enum => self
                .choice
                .map(|i| Value::plain(self.var.options[i].clone())),
            k => Some(Value::new(self.editor.text(), k == VarKind::Secret)),
        }
    }

    fn touched(&mut self) {
        self.error = None;
        self.source = if self.value() == self.prefill {
            self.prefill_source
        } else {
            Source::Typed
        };
    }

    /// Ctrl-Z: back to the pre-filled value.
    fn reset(&mut self) {
        self.locked = self.secret() && self.prefill_source == Source::Env && self.prefill.is_some();
        self.editor = LineEditor::default();
        self.choice = None;
        if let Some(v) = self.prefill.clone().filter(|_| !self.locked) {
            self.set(v.reveal());
        }
        self.error = None;
        self.source = self.prefill_source;
    }

    /// Checks the value; returns the message to show when it is not usable.
    fn check(&self) -> Option<String> {
        match self.value() {
            None => Some(format!("choose one of {}", self.var.options.join(", "))),
            Some(v) => varres::validate(&self.var, &v).err().map(|e| e.message),
        }
    }

    /// Edits the field with a key. Returns whether the key was used.
    fn key(&mut self, key: Key) -> bool {
        let plain = key.mods & (CTRL | ALT) == 0;
        if self.var.kind == VarKind::Enum {
            let n = self.var.options.len();
            match key.code {
                Code::Left if key.mods == 0 && n > 0 => {
                    self.choice = Some(match self.choice {
                        Some(i) if i > 0 => i - 1,
                        Some(_) | None => n - 1,
                    });
                }
                Code::Right if key.mods == 0 && n > 0 => {
                    self.choice = Some(self.choice.map_or(0, |i| (i + 1) % n));
                }
                _ => return false,
            }
            self.touched();
            return true;
        }
        if self.locked {
            if key.same(Key::new(Code::Char('r'), CTRL)) {
                self.locked = false;
                self.editor = LineEditor::default();
                self.touched();
                return true;
            }
            return false;
        }
        let changed = match key.code {
            Code::Char(c) if plain && !c.is_control() => {
                self.editor.insert(c);
                true
            }
            Code::Char('a') if key.mods == CTRL => {
                self.editor.home();
                false
            }
            Code::Char('w') if key.mods == CTRL => self.editor.delete_word(),
            Code::Char('u') if key.mods == CTRL => self.editor.clear(),
            Code::Backspace if key.mods == 0 => self.editor.backspace(),
            Code::Delete if key.mods == 0 => self.editor.delete(),
            Code::Left if key.mods == 0 => {
                self.editor.left();
                false
            }
            Code::Right if key.mods == 0 => {
                self.editor.right();
                false
            }
            Code::Home if key.mods == 0 => {
                self.editor.home();
                false
            }
            Code::End if key.mods == 0 => {
                self.editor.end();
                false
            }
            _ => return false,
        };
        if changed {
            self.touched();
        }
        true
    }
}

/// What a key did to the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormEvent {
    None,
    Close,
    Submit { print: bool },
}

/// A form that passed validation, ready to be checked against the file and used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    pub id: String,
    pub signature: u64,
    pub values: Vec<(String, Value, Source)>,
    pub print: bool,
    pub then: Then,
}

#[derive(Debug, Clone)]
pub struct FormState {
    pub id: String,
    pub title: String,
    pub plan: VarPlan,
    pub fields: Vec<Field>,
    pub focus: usize,
    pub then: Then,
    pub notice: Option<String>,
    recent: BTreeMap<String, Vec<String>>,
    /// The selected suggestion, once the list has been entered with Alt-Down.
    pub suggest: Option<usize>,
}

/// How many recent values are offered.
pub const SUGGESTIONS: usize = 5;

impl FormState {
    /// Opens the form. `typed` holds values from an earlier visit in this session.
    pub fn open(
        spec: FormSpec,
        then: Then,
        typed: Option<&BTreeMap<String, String>>,
        notice: Option<String>,
    ) -> FormState {
        let fields = spec
            .plan
            .vars
            .iter()
            .zip(&spec.bindings)
            .map(|(p, b)| {
                let mut field = Field::new(&p.var, b);
                if let Some(text) = typed.and_then(|t| t.get(&p.var.name))
                    && !field.secret()
                {
                    field.set(text);
                    field.touched();
                }
                field
            })
            .collect();
        FormState {
            id: spec.id,
            title: spec.title,
            plan: spec.plan,
            fields,
            focus: 0,
            then,
            notice,
            recent: spec.recent,
            suggest: None,
        }
    }

    /// The entry changed under the form: rebuilds it, keeping what was typed where the variable
    /// is still there with the same kind.
    pub fn reopen(&mut self, spec: FormSpec, notice: &str) {
        let typed: BTreeMap<String, Value> = self
            .fields
            .iter()
            .filter(|f| f.source == Source::Typed)
            .filter_map(|f| Some((f.var.name.clone(), f.value()?)))
            .collect();
        let kinds: BTreeMap<String, VarKind> = self
            .fields
            .iter()
            .map(|f| (f.var.name.clone(), f.var.kind))
            .collect();
        let mut fresh = FormState::open(spec, self.then, None, Some(notice.to_string()));
        for f in &mut fresh.fields {
            if kinds.get(&f.var.name) == Some(&f.var.kind)
                && let Some(v) = typed.get(&f.var.name)
            {
                f.locked = false;
                f.set(v.reveal());
                if f.value().is_some() {
                    f.touched();
                }
            }
        }
        *self = fresh;
    }

    pub fn counter(&self) -> String {
        format!("field {}/{}", self.focus + 1, self.fields.len())
    }

    /// The suggestions for the focused field: recent values of a text variable of this name.
    pub fn suggestions(&self) -> Vec<String> {
        let Some(f) = self.fields.get(self.focus) else {
            return Vec::new();
        };
        if f.var.kind != VarKind::Text || f.locked {
            return Vec::new();
        }
        self.recent
            .get(&f.var.name)
            .map(|r| {
                r.iter()
                    .filter(|v| v.as_str() != f.editor.text())
                    .take(SUGGESTIONS)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn values(&self) -> Values {
        self.fields
            .iter()
            .filter_map(|f| Some((f.var.name.clone(), f.value()?)))
            .collect()
    }

    /// The text the user would get now; a variable without a valid value keeps its placeholder.
    pub fn preview(&self) -> Resolved {
        varres::preview(&self.plan, &self.values())
    }

    /// Non-secret values as they stand, kept for the rest of the session when the form is closed.
    pub fn session_values(&self) -> BTreeMap<String, String> {
        self.fields
            .iter()
            .filter(|f| !f.secret() && f.source == Source::Typed)
            .filter_map(|f| Some((f.var.name.clone(), f.value()?.reveal().to_string())))
            .collect()
    }

    /// Marks every field that cannot be used and focuses the first one. True when all are fine.
    pub fn validate_all(&mut self) -> bool {
        let mut first = None;
        for (i, f) in self.fields.iter_mut().enumerate() {
            f.error = f.check();
            if f.error.is_some() && first.is_none() {
                first = Some(i);
            }
        }
        if let Some(i) = first {
            self.focus = i;
            self.suggest = None;
        }
        first.is_none()
    }

    pub fn submission(&self, print: bool) -> Submission {
        Submission {
            id: self.id.clone(),
            signature: self.plan.signature,
            values: self
                .fields
                .iter()
                .filter_map(|f| Some((f.var.name.clone(), f.value()?, f.source)))
                .collect(),
            print,
            then: self.then,
        }
    }

    /// Pasted text goes into the focused field; only its first line is kept.
    pub fn paste(&mut self, text: &str) {
        let Some(f) = self.fields.get_mut(self.focus) else {
            return;
        };
        if f.var.kind == VarKind::Enum || f.locked {
            return;
        }
        let mut lines = text.lines();
        let first = lines.next().unwrap_or("");
        if lines.next().is_some() {
            self.notice = Some("Pasted text was cut to its first line".into());
        }
        for c in first.chars().filter(|c| !c.is_control()) {
            f.editor.insert(c);
        }
        f.touched();
    }

    fn move_focus(&mut self, delta: isize) {
        let n = self.fields.len() as isize;
        self.focus = (self.focus as isize + delta).rem_euclid(n) as usize;
        self.suggest = None;
    }

    pub fn handle_key(&mut self, key: Key) -> FormEvent {
        self.notice = None;
        // the suggestion list, once entered
        if let Some(sel) = self.suggest {
            let items = self.suggestions();
            match key.code {
                Code::Down if key.mods == 0 && !items.is_empty() => {
                    self.suggest = Some((sel + 1).min(items.len() - 1));
                    return FormEvent::None;
                }
                Code::Up if key.mods == 0 => {
                    self.suggest = Some(sel.saturating_sub(1));
                    return FormEvent::None;
                }
                Code::Enter | Code::Tab if key.mods == 0 => {
                    if let Some(item) = items.get(sel) {
                        let f = &mut self.fields[self.focus];
                        f.editor = LineEditor::new(item);
                        f.touched();
                    }
                    self.suggest = None;
                    return FormEvent::None;
                }
                Code::Esc => {
                    self.suggest = None;
                    return FormEvent::None;
                }
                _ => self.suggest = None,
            }
        }
        match (key.code, key.mods) {
            (Code::Esc, 0) => FormEvent::Close,
            (Code::Enter, 0) | (Code::Enter, ALT) => {
                if self.validate_all() {
                    FormEvent::Submit {
                        print: key.mods == ALT,
                    }
                } else {
                    FormEvent::None
                }
            }
            (Code::Tab | Code::Down, 0) => {
                self.move_focus(1);
                FormEvent::None
            }
            (Code::BackTab | Code::Up, 0) => {
                self.move_focus(-1);
                FormEvent::None
            }
            (Code::Down, ALT) => {
                if !self.suggestions().is_empty() {
                    self.suggest = Some(0);
                }
                FormEvent::None
            }
            (Code::Char('z'), CTRL) => {
                self.fields[self.focus].reset();
                FormEvent::None
            }
            _ => {
                self.fields[self.focus].key(key);
                FormEvent::None
            }
        }
    }
}

/// The form's keys, as the key bar and tests list them.
pub const KEY_BAR: &str = "Tab next · Enter copy · Alt-Enter print · ^Z reset · Esc back";

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use kb_core::entry::parse_entry;
    use kb_core::varres::{Inputs, plan_vars, prefill};

    pub(crate) fn spec_of(front: &str, payload: &str) -> FormSpec {
        spec_with(front, payload, &[], &|_| None)
    }

    pub(crate) fn spec_with(
        front: &str,
        payload: &str,
        recent: &[(&str, &[&str])],
        env: &dyn Fn(&str) -> Option<String>,
    ) -> FormSpec {
        let text = format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Deploy\nshell: bash\n{front}---\n\n```\n{payload}\n```\n"
        );
        let entry = parse_entry("x", "x.md", text.into_bytes()).entry.unwrap();
        let plan = plan_vars(&entry);
        let bindings = prefill(
            &plan,
            &Inputs {
                flags: &[],
                env,
                memory: None,
            },
        );
        FormSpec {
            id: "x".into(),
            title: "Deploy".into(),
            plan,
            bindings,
            recent: recent
                .iter()
                .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
                .collect(),
        }
    }

    const FRONT: &str = "vars:\n  env: { kind: enum, options: [prod, stage, dev], default: stage }\n  minutes: { kind: number }\n  host: { kind: text, default: web }\n";
    const PAYLOAD: &str = "run {{env}} {{minutes}} {{host}}";

    fn form() -> FormState {
        FormState::open(spec_of(FRONT, PAYLOAD), Then::Exit, None, None)
    }

    fn press(f: &mut FormState, code: Code, mods: u8) -> FormEvent {
        f.handle_key(Key::new(code, mods))
    }

    fn type_text(f: &mut FormState, text: &str) {
        for c in text.chars() {
            press(f, Code::Char(c), 0);
        }
    }

    fn preview_text(f: &FormState) -> String {
        f.preview().primary().unwrap().text.clone()
    }

    #[test]
    fn fields_follow_the_payload_and_are_prefilled() {
        let f = form();
        let names: Vec<_> = f.fields.iter().map(|x| x.var.name.as_str()).collect();
        assert_eq!(names, vec!["env", "minutes", "host"]);
        assert_eq!(f.fields[0].choice, Some(1));
        assert_eq!(f.fields[0].source, Source::Default);
        assert_eq!(f.fields[2].editor.text(), "web");
        assert_eq!(f.counter(), "field 1/3");
        assert_eq!(preview_text(&f), "run stage {{minutes}} web\n");
    }

    #[test]
    fn tab_and_arrows_move_focus_and_wrap() {
        let mut f = form();
        press(&mut f, Code::Tab, 0);
        assert_eq!(f.focus, 1);
        press(&mut f, Code::Down, 0);
        press(&mut f, Code::Tab, 0);
        assert_eq!(f.focus, 0, "wraps");
        press(&mut f, Code::BackTab, 0);
        assert_eq!(f.focus, 2);
        press(&mut f, Code::Up, 0);
        assert_eq!(f.focus, 1);
    }

    #[test]
    fn left_and_right_change_an_enum_and_the_preview_follows() {
        let mut f = form();
        press(&mut f, Code::Right, 0);
        assert_eq!(f.fields[0].choice, Some(2));
        assert_eq!(f.fields[0].source, Source::Typed);
        press(&mut f, Code::Right, 0);
        assert_eq!(f.fields[0].choice, Some(0), "wraps");
        press(&mut f, Code::Left, 0);
        assert_eq!(f.fields[0].choice, Some(2));
        assert!(preview_text(&f).starts_with("run dev "));
    }

    #[test]
    fn typing_edits_text_and_number_fields_with_line_editing() {
        let mut f = form();
        press(&mut f, Code::Tab, 0);
        type_text(&mut f, "30");
        assert_eq!(f.fields[1].editor.text(), "30");
        press(&mut f, Code::Left, 0);
        press(&mut f, Code::Char('1'), 0);
        assert_eq!(f.fields[1].editor.text(), "310");
        press(&mut f, Code::Home, 0);
        press(&mut f, Code::Delete, 0);
        assert_eq!(f.fields[1].editor.text(), "10");
        press(&mut f, Code::Char('u'), CTRL);
        assert_eq!(f.fields[1].editor.text(), "");
        type_text(&mut f, "ab cd");
        press(&mut f, Code::Char('w'), CTRL);
        assert_eq!(f.fields[1].editor.text(), "ab ");
        press(&mut f, Code::Backspace, 0);
        press(&mut f, Code::Char('a'), CTRL);
        press(&mut f, Code::Char('X'), 0);
        assert_eq!(f.fields[1].editor.text(), "Xab");
        press(&mut f, Code::End, 0);
        press(&mut f, Code::Char('!'), 0);
        assert_eq!(f.fields[1].editor.text(), "Xab!");
    }

    #[test]
    fn ctrl_z_resets_the_focused_field_to_its_prefill() {
        let mut f = form();
        press(&mut f, Code::Tab, 0);
        press(&mut f, Code::Tab, 0);
        type_text(&mut f, "x");
        assert_eq!(f.fields[2].editor.text(), "webx");
        assert_eq!(f.fields[2].source, Source::Typed);
        press(&mut f, Code::Char('z'), CTRL);
        assert_eq!(f.fields[2].editor.text(), "web");
        assert_eq!(f.fields[2].source, Source::Default);
        // an enum
        f.focus = 0;
        press(&mut f, Code::Right, 0);
        press(&mut f, Code::Char('z'), CTRL);
        assert_eq!(f.fields[0].choice, Some(1));
    }

    #[test]
    fn a_wrong_value_blocks_submit_focuses_it_and_explains() {
        let mut f = form();
        press(&mut f, Code::Tab, 0);
        type_text(&mut f, "abc");
        press(&mut f, Code::Tab, 0);
        assert_eq!(press(&mut f, Code::Enter, 0), FormEvent::None);
        assert_eq!(f.focus, 1);
        assert_eq!(
            f.fields[1].error.as_deref(),
            Some("expected a number like 30, -5 or 0.25")
        );
        // editing clears the message
        press(&mut f, Code::Backspace, 0);
        assert_eq!(f.fields[1].error, None);
    }

    #[test]
    fn an_unchosen_enum_is_not_valid() {
        let mut f = FormState::open(
            spec_of(
                "vars:\n  env: { kind: enum, options: [prod, stage] }\n",
                "x {{env}}",
            ),
            Then::Exit,
            None,
            None,
        );
        assert_eq!(f.fields[0].choice, None);
        assert_eq!(press(&mut f, Code::Enter, 0), FormEvent::None);
        assert_eq!(
            f.fields[0].error.as_deref(),
            Some("choose one of prod, stage")
        );
        press(&mut f, Code::Right, 0);
        assert_eq!(f.fields[0].choice, Some(0));
        assert_eq!(
            press(&mut f, Code::Enter, 0),
            FormEvent::Submit { print: false }
        );
    }

    #[test]
    fn enter_and_alt_enter_submit_when_everything_is_valid() {
        let mut f = form();
        press(&mut f, Code::Tab, 0);
        type_text(&mut f, "30");
        assert_eq!(
            press(&mut f, Code::Enter, 0),
            FormEvent::Submit { print: false }
        );
        assert_eq!(
            press(&mut f, Code::Enter, ALT),
            FormEvent::Submit { print: true }
        );
        let sub = f.submission(true);
        assert_eq!(sub.id, "x");
        assert!(sub.print && sub.then == Then::Exit);
        let got: Vec<_> = sub
            .values
            .iter()
            .map(|(n, v, s)| (n.as_str(), v.reveal(), *s))
            .collect();
        assert_eq!(
            got,
            vec![
                ("env", "stage", Source::Default),
                ("minutes", "30", Source::Typed),
                ("host", "web", Source::Default)
            ]
        );
    }

    #[test]
    fn esc_closes_and_keeps_typed_values_for_the_session() {
        let mut f = form();
        press(&mut f, Code::Tab, 0);
        type_text(&mut f, "45");
        assert_eq!(press(&mut f, Code::Esc, 0), FormEvent::Close);
        let kept = f.session_values();
        assert_eq!(kept.get("minutes").map(String::as_str), Some("45"));
        assert!(!kept.contains_key("host"), "untouched values are not kept");
        let again = FormState::open(spec_of(FRONT, PAYLOAD), Then::Exit, Some(&kept), None);
        assert_eq!(again.fields[1].editor.text(), "45");
        assert_eq!(again.fields[1].source, Source::Typed);
    }

    #[test]
    fn pasted_text_keeps_its_first_line_and_says_so() {
        let mut f = form();
        press(&mut f, Code::Tab, 0);
        f.paste("12\n34");
        assert_eq!(f.fields[1].editor.text(), "12");
        assert!(f.notice.is_some());
        f.focus = 0;
        f.paste("x");
        assert_eq!(f.fields[0].choice, Some(1), "enums ignore paste");
    }

    #[test]
    fn a_secret_from_the_environment_is_locked_until_ctrl_r() {
        let env = |n: &str| (n == "KB_TOK").then(|| "S3CR3T-MARKER".to_string());
        let spec = spec_with(
            "vars:\n  token: { kind: secret, env: KB_TOK }\n",
            "curl {{token}}",
            &[],
            &env,
        );
        let mut f = FormState::open(spec, Then::Exit, None, None);
        assert!(f.fields[0].locked);
        assert_eq!(f.fields[0].source, Source::Env);
        press(&mut f, Code::Char('x'), 0);
        assert_eq!(
            f.fields[0].value().unwrap().reveal(),
            "S3CR3T-MARKER",
            "not editable"
        );
        // the preview masks it
        let (shown, _) = f.preview().primary().unwrap().display();
        assert_eq!(shown, "curl ••••\n");
        assert!(!format!("{f:?}").contains("S3CR3T"));
        assert!(!format!("{:?}", f.submission(false)).contains("S3CR3T"));
        press(&mut f, Code::Char('r'), CTRL);
        assert!(!f.fields[0].locked);
        type_text(&mut f, "new");
        assert_eq!(f.fields[0].value().unwrap().reveal(), "new");
        assert_eq!(f.fields[0].source, Source::Typed);
        assert!(!format!("{f:?}").contains("new\""));
        press(&mut f, Code::Char('z'), CTRL);
        assert!(f.fields[0].locked, "reset locks it again");
    }

    #[test]
    fn a_typed_secret_is_never_kept_for_the_session() {
        let spec = spec_of("vars:\n  token: { kind: secret }\n", "curl {{token}}");
        let mut f = FormState::open(spec, Then::Exit, None, None);
        type_text(&mut f, "S3CR3T-MARKER");
        assert!(f.session_values().is_empty());
        assert!(!format!("{f:?}").contains("S3CR3T"));
    }

    #[test]
    fn suggestions_come_from_recent_values_and_only_for_text() {
        let spec = spec_with(
            FRONT,
            PAYLOAD,
            &[("host", &["api", "web", "db"]), ("env", &["prod"])],
            &|_| None,
        );
        let mut f = FormState::open(spec, Then::Exit, None, None);
        assert!(f.suggestions().is_empty(), "enum: none");
        f.focus = 1;
        assert!(f.suggestions().is_empty(), "no history for minutes");
        f.focus = 2;
        assert_eq!(
            f.suggestions(),
            vec!["api", "db"],
            "the current value is left out"
        );
        // up/down still move between fields until the list is entered
        press(&mut f, Code::Up, 0);
        assert_eq!(f.focus, 1);
        f.focus = 2;
        press(&mut f, Code::Down, ALT);
        assert_eq!(f.suggest, Some(0));
        press(&mut f, Code::Down, 0);
        assert_eq!(f.suggest, Some(1));
        assert_eq!(f.focus, 2);
        press(&mut f, Code::Enter, 0);
        assert_eq!(f.fields[2].editor.text(), "db");
        assert_eq!(f.suggest, None);
        assert_eq!(f.fields[2].source, Source::Typed);
        // Esc closes the list without closing the form
        press(&mut f, Code::Down, ALT);
        assert_eq!(press(&mut f, Code::Esc, 0), FormEvent::None);
        assert_eq!(f.suggest, None);
        // typing closes it too
        press(&mut f, Code::Down, ALT);
        press(&mut f, Code::Char('x'), 0);
        assert_eq!(f.suggest, None);
    }

    #[test]
    fn at_most_five_suggestions() {
        let spec = spec_with(
            FRONT,
            PAYLOAD,
            &[("host", &["a", "b", "c", "d", "e", "f", "g"])],
            &|_| None,
        );
        let mut f = FormState::open(spec, Then::Exit, None, None);
        f.focus = 2;
        assert_eq!(f.suggestions().len(), SUGGESTIONS);
    }

    #[test]
    fn reopening_after_an_edit_keeps_typed_values_that_still_fit() {
        let mut f = form();
        press(&mut f, Code::Tab, 0);
        type_text(&mut f, "30");
        let new = spec_of(
            "vars:\n  minutes: { kind: number }\n  region: { kind: text, default: eu }\n  env: { kind: text }\n",
            "go {{minutes}} {{region}} {{env}}",
        );
        f.reopen(new, "The entry changed; review the fields");
        let names: Vec<_> = f.fields.iter().map(|x| x.var.name.as_str()).collect();
        assert_eq!(names, vec!["minutes", "region", "env"]);
        assert_eq!(f.fields[0].editor.text(), "30");
        assert_eq!(f.fields[1].editor.text(), "eu");
        assert_eq!(
            f.fields[2].editor.text(),
            "",
            "env changed kind, so nothing carried over"
        );
        assert!(f.notice.as_deref().unwrap().contains("changed"));
    }

    #[test]
    fn the_key_bar_text_is_the_documented_one() {
        assert_eq!(
            KEY_BAR,
            "Tab next · Enter copy · Alt-Enter print · ^Z reset · Esc back"
        );
    }
}
