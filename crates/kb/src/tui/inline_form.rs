//! Inline variable form for compact parameter input.

use std::collections::BTreeMap;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use kb_core::entry::Entry;
use kb_core::shell::ShellDialect;
use kb_core::varres::{self, ResolutionTarget, Value, Values, VarPlan};
use kb_core::vars::{VarKind, Variable};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineFormEvent {
    None,
    Cancel,
    Submit(BTreeMap<String, String>),
}

#[derive(Debug, Clone)]
pub struct InlineFormField {
    pub name: String,
    pub kind: VarKind,
    pub value: String,
    pub options: Vec<String>,
    pub option_idx: Option<usize>,
    pub secret: bool,
    pub env_var: Option<String>,
}

impl InlineFormField {
    pub fn new(var: &Variable, prefilled_val: Option<&String>) -> Self {
        let is_secret = var.kind == VarKind::Secret;
        let mut opt_idx = None;
        let val = if let Some(p) = prefilled_val {
            if var.kind == VarKind::Enum {
                opt_idx = var.options.iter().position(|o| o == p);
            }
            p.clone()
        } else if let Some(d) = &var.default {
            if var.kind == VarKind::Enum {
                opt_idx = var.options.iter().position(|o| o == d);
            }
            d.clone()
        } else if var.kind == VarKind::Enum && !var.options.is_empty() {
            opt_idx = Some(0);
            var.options[0].clone()
        } else {
            String::new()
        };

        Self {
            name: var.name.clone(),
            kind: var.kind,
            value: val,
            options: var.options.clone(),
            option_idx: opt_idx,
            secret: is_secret,
            env_var: var.env.clone(),
        }
    }
}

pub struct InlineForm {
    pub entry_id: String,
    pub plan: VarPlan,
    pub dialect: ShellDialect,
    pub fields: Vec<InlineFormField>,
    pub focus: usize,
}

impl InlineForm {
    pub fn new(
        entry: &Entry,
        dialect: ShellDialect,
        prefill: &BTreeMap<String, String>,
    ) -> Self {
        let plan = varres::plan_vars(entry);
        let fields: Vec<InlineFormField> = plan
            .vars
            .iter()
            .map(|p| InlineFormField::new(&p.var, prefill.get(&p.var.name)))
            .collect();

        Self {
            entry_id: entry.id().to_string(),
            plan,
            dialect,
            fields,
            focus: 0,
        }
    }

    pub fn active_field(&self) -> &InlineFormField {
        &self.fields[self.focus]
    }

    pub fn active_field_mut(&mut self) -> &mut InlineFormField {
        &mut self.fields[self.focus]
    }

    pub fn values(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        for f in &self.fields {
            m.insert(f.name.clone(), f.value.clone());
        }
        m
    }

    pub fn preview(&self) -> String {
        let mut val_map = Values::new();
        for f in &self.fields {
            val_map.insert(f.name.clone(), Value::plain(&f.value));
        }
        let res = varres::preview_for_target(
            &self.plan,
            &val_map,
            ResolutionTarget::ShellWidget(self.dialect),
        );
        res.primary()
            .map(|o| o.text.trim_end_matches('\n').to_string())
            .unwrap_or_default()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> InlineFormEvent {
        // Ctrl-C or Esc cancels
        if key.code == KeyCode::Esc || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')) {
            return InlineFormEvent::Cancel;
        }

        // Enter submits
        if key.code == KeyCode::Enter {
            return InlineFormEvent::Submit(self.values());
        }

        if self.fields.is_empty() {
            return InlineFormEvent::None;
        }

        // Navigation between fields
        if key.code == KeyCode::Tab || key.code == KeyCode::Down {
            self.focus = (self.focus + 1) % self.fields.len();
            return InlineFormEvent::None;
        }

        if key.code == KeyCode::BackTab || key.code == KeyCode::Up {
            if self.focus == 0 {
                self.focus = self.fields.len().saturating_sub(1);
            } else {
                self.focus -= 1;
            }
            return InlineFormEvent::None;
        }

        // Editing active field
        let field = self.active_field_mut();
        if field.kind == VarKind::Enum && !field.options.is_empty() {
            let n = field.options.len();
            match key.code {
                KeyCode::Right => {
                    let cur = field.option_idx.unwrap_or(0);
                    let next = (cur + 1) % n;
                    field.option_idx = Some(next);
                    field.value = field.options[next].clone();
                }
                KeyCode::Left => {
                    let cur = field.option_idx.unwrap_or(0);
                    let prev = if cur == 0 { n - 1 } else { cur - 1 };
                    field.option_idx = Some(prev);
                    field.value = field.options[prev].clone();
                }
                _ => {}
            }
        } else {
            // Text or secret field
            match key.code {
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    field.value.push(c);
                }
                KeyCode::Backspace => {
                    field.value.pop();
                }
                _ => {}
            }
        }

        InlineFormEvent::None
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn make_ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn make_test_entry(vars_yaml: &str, payload: &str) -> Entry {
        let text = format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Test\nshell: bash\nvars:\n{vars_yaml}---\n\n```\n{payload}\n```\n"
        );
        kb_core::entry::parse_entry("test", "test.md", text.into_bytes())
            .entry
            .unwrap()
    }

    #[test]
    fn test_inline_form_navigation() {
        let entry = make_test_entry(
            "  env: { kind: text, default: dev }\n  stage: { kind: text, default: v1 }\n",
            "tail -f {{env}}-{{stage}}.log",
        );

        let mut form = InlineForm::new(
            &entry,
            ShellDialect::Zsh,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(form.focus, 0);

        // Tab moves to next
        let ev = form.handle_key(make_key(KeyCode::Tab));
        assert_eq!(ev, InlineFormEvent::None);
        assert_eq!(form.focus, 1);

        // Tab wraps around
        form.handle_key(make_key(KeyCode::Tab));
        assert_eq!(form.focus, 0);

        // BackTab moves back
        form.handle_key(make_key(KeyCode::BackTab));
        assert_eq!(form.focus, 1);
    }

    #[test]
    fn test_inline_form_enum_cycling() {
        let entry = make_test_entry(
            "  env: { kind: enum, options: [dev, staging, prod], default: dev }\n",
            "deploy --env {{env}}",
        );

        let mut form = InlineForm::new(
            &entry,
            ShellDialect::Zsh,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(form.active_field().value, "dev");

        // Cycle forward
        form.handle_key(make_key(KeyCode::Right));
        assert_eq!(form.active_field().value, "staging");

        form.handle_key(make_key(KeyCode::Right));
        assert_eq!(form.active_field().value, "prod");

        form.handle_key(make_key(KeyCode::Right));
        assert_eq!(form.active_field().value, "dev");

        // Cycle backward
        form.handle_key(make_key(KeyCode::Left));
        assert_eq!(form.active_field().value, "prod");
    }

    #[test]
    fn test_inline_form_text_editing_and_preview() {
        let entry = make_test_entry(
            "  service: { kind: text }\n",
            "systemctl restart {{service}}",
        );

        let mut form = InlineForm::new(
            &entry,
            ShellDialect::Zsh,
            &std::collections::BTreeMap::new(),
        );

        form.handle_key(make_key(KeyCode::Char('a')));
        form.handle_key(make_key(KeyCode::Char('p')));
        form.handle_key(make_key(KeyCode::Char('i')));

        assert_eq!(form.active_field().value, "api");
        assert_eq!(form.preview(), "systemctl restart api");

        // Backspace
        form.handle_key(make_key(KeyCode::Backspace));
        assert_eq!(form.active_field().value, "ap");
        assert_eq!(form.preview(), "systemctl restart ap");
    }

    #[test]
    fn test_inline_form_submission_and_cancellation() {
        let entry = make_test_entry(
            "  env: { kind: text, default: prod }\n",
            "status {{env}}",
        );

        let mut form = InlineForm::new(
            &entry,
            ShellDialect::Zsh,
            &std::collections::BTreeMap::new(),
        );

        // Enter submits
        let ev = form.handle_key(make_key(KeyCode::Enter));
        match ev {
            InlineFormEvent::Submit(values) => {
                assert_eq!(values.get("env").map(|s| s.as_str()), Some("prod"));
            }
            _ => panic!("Expected Submit event, got {:?}", ev),
        }

        // Esc cancels
        let ev = form.handle_key(make_key(KeyCode::Esc));
        assert_eq!(ev, InlineFormEvent::Cancel);

        // Ctrl-C cancels
        let ev = form.handle_key(make_ctrl(KeyCode::Char('c')));
        assert_eq!(ev, InlineFormEvent::Cancel);
    }
}


