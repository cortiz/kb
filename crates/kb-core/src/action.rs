//! Action model, registries, and resolution.

use crate::code::LocalClone;
use crate::config::{CustomActionConfig, CustomActionInput, Settings};
use crate::entry::{Entry, EntryType, TypeFields};
use crate::varres::Values;
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    CopyPayload,
    CopyRenderedText,
    EditEntry,
    CopyCodeLocation,
    CopyWebPermalink,
    OpenUrl,
    OpenEditor,
    RunCommand,
    CustomCommand,
    CodeView,
    RepinCode,
    FetchAndCheck,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub id: String,    // stable identifier e.g. "open-sumo", "run", "psql"
    pub name: String,  // short user-visible name e.g. "Open in Sumo"
    pub key: char,     // single-letter keyboard shortcut
    pub label: String, // detailed description for menus
    pub kind: ActionKind,
    pub is_primary: bool, // whether this is the entry type's default open action
    pub needs_vars: bool, // requires M5 variable resolution prior to execution
    pub needs_confirm: bool, // requires confirmation prompt before execution
    pub requires_local_clone: bool, // disabled if no local git clone is discovered
    pub requires_display: bool, // disabled over SSH without GUI display
    pub disabled_reason: Option<String>,
}

impl Action {
    #[allow(clippy::too_many_arguments)]
    pub fn builtin(
        id: impl Into<String>,
        name: impl Into<String>,
        key: char,
        label: impl Into<String>,
        kind: ActionKind,
        is_primary: bool,
        needs_vars: bool,
        needs_confirm: bool,
        requires_local_clone: bool,
        requires_display: bool,
    ) -> Self {
        Action {
            id: id.into(),
            name: name.into(),
            key,
            label: label.into(),
            kind,
            is_primary,
            needs_vars,
            needs_confirm,
            requires_local_clone,
            requires_display,
            disabled_reason: None,
        }
    }

    pub fn is_available(&self, has_local_clone: bool, is_ssh_without_display: bool) -> bool {
        if self.disabled_reason.is_some() {
            return false;
        }
        if self.requires_local_clone && !has_local_clone {
            return false;
        }
        if self.requires_display && is_ssh_without_display {
            return false;
        }
        true
    }
}

pub fn entry_field_value(entry: &Entry, values: Option<&Values>, field: &str) -> Option<String> {
    match field {
        "id" => Some(entry.id().to_string()),
        "path" => Some(entry.path().to_string()),
        "title" => entry.title().map(ToString::to_string),
        "notes" => Some(entry.notes().to_string()),
        "type" => entry.entry_type().map(|t| t.name().to_string()),
        "created" => entry.created().map(ToString::to_string),
        "updated" => entry.updated().map(ToString::to_string),
        "payload" => entry.payload().map(|p| p.text.clone()),
        _ => {
            let type_val = match entry.fields() {
                TypeFields::Sql { database, dialect } => match field {
                    "database" => database.clone(),
                    "dialect" => dialect.clone(),
                    _ => None,
                },
                TypeFields::Sumo { range } => match field {
                    "range" => range.clone(),
                    _ => None,
                },
                TypeFields::Cw {
                    log_groups,
                    range,
                    region,
                } => match field {
                    "region" => region.clone(),
                    "range" => range.clone(),
                    "log_groups" => (!log_groups.is_empty()).then(|| log_groups.join(" ")),
                    _ => None,
                },
                TypeFields::Cmd { shell, cwd } => match field {
                    "shell" => Some(shell.as_str().to_string()),
                    "cwd" => cwd.clone(),
                    _ => None,
                },
                TypeFields::Link { url } => match field {
                    "url" => url.clone(),
                    _ => None,
                },
                TypeFields::Code { code: Some(c) } => match field {
                    "repo" => Some(c.repo.clone()),
                    "path" => Some(c.path.clone()),
                    "line" => Some(c.line.to_string()),
                    "ref" | "git_ref" => Some(c.git_ref.clone()),
                    "symbol" => c.symbol.clone(),
                    "end_line" => c.end_line.map(|l| l.to_string()),
                    "branch" => c.branch.clone(),
                    _ => None,
                },
                _ => None,
            };
            if type_val.is_some() {
                return type_val;
            }

            if let Some(val) = values.and_then(|v| v.get(field)) {
                return Some(val.reveal().to_string());
            }
            if let Some(var) = entry.vars().iter().find(|v| v.name == field) {
                return var.default.clone().or_else(|| Some(String::new()));
            }

            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionConfigWarning {
    pub entry_type: String,
    pub name: String,
    pub key: char,
    pub message: String,
}

pub struct ActionService;

impl ActionService {
    pub fn list_actions(
        entry: &Entry,
        config: &Settings,
        clone_status: Option<&LocalClone>,
    ) -> Vec<Action> {
        let _ = clone_status;
        let mut actions = Vec::new();

        match entry.entry_type {
            Some(EntryType::Link) => {
                actions.push(Action::builtin(
                    "copy",
                    "Copy URL",
                    'c',
                    "Copy URL to clipboard",
                    ActionKind::CopyPayload,
                    false,
                    false,
                    false,
                    false,
                    false,
                ));
                actions.push(Action::builtin(
                    "open-link",
                    "Open Link",
                    'o',
                    "Open link in default browser",
                    ActionKind::OpenUrl,
                    true,
                    true,
                    false,
                    false,
                    true,
                ));
            }
            Some(EntryType::Sumo) => {
                actions.push(Action::builtin(
                    "copy",
                    "Copy Query",
                    'c',
                    "Copy Sumo query to clipboard",
                    ActionKind::CopyPayload,
                    false,
                    false,
                    false,
                    false,
                    false,
                ));
                actions.push(Action::builtin(
                    "open-sumo",
                    "Open in Sumo",
                    'o',
                    "Open deep link in Sumo Logic",
                    ActionKind::OpenUrl,
                    true,
                    true,
                    false,
                    false,
                    true,
                ));
            }
            Some(EntryType::Cw) => {
                actions.push(Action::builtin(
                    "copy",
                    "Copy Query",
                    'c',
                    "Copy CloudWatch query to clipboard",
                    ActionKind::CopyPayload,
                    false,
                    false,
                    false,
                    false,
                    false,
                ));
                actions.push(Action::builtin(
                    "open-cw",
                    "Open in CloudWatch",
                    'o',
                    "Open deep link in CloudWatch Logs Insights",
                    ActionKind::OpenUrl,
                    true,
                    true,
                    false,
                    false,
                    true,
                ));
            }
            Some(EntryType::Cmd) => {
                actions.push(Action::builtin(
                    "copy",
                    "Copy Command",
                    'c',
                    "Copy command to clipboard",
                    ActionKind::CopyPayload,
                    false,
                    false,
                    false,
                    false,
                    false,
                ));
                actions.push(Action::builtin(
                    "run",
                    "Run Command",
                    'r',
                    "Run command safely in terminal",
                    ActionKind::RunCommand,
                    false,
                    true,
                    true,
                    false,
                    false,
                ));
            }
            Some(EntryType::Code) => {
                actions.push(Action::builtin(
                    "copy",
                    "Copy Location",
                    'y',
                    "Copy path:line to clipboard",
                    ActionKind::CopyCodeLocation,
                    false,
                    false,
                    false,
                    false,
                    false,
                ));
                actions.push(Action::builtin(
                    "copy-permalink",
                    "Copy Permalink",
                    'Y',
                    "Copy web permalink to clipboard",
                    ActionKind::CopyWebPermalink,
                    false,
                    false,
                    false,
                    false,
                    false,
                ));
                actions.push(Action::builtin(
                    "code-view",
                    "Code View",
                    'o',
                    "Open code preview with drift status",
                    ActionKind::CodeView,
                    true,
                    false,
                    false,
                    false,
                    false,
                ));
                actions.push(Action::builtin(
                    "open-web",
                    "Open in Browser",
                    'g',
                    "Open web permalink in browser",
                    ActionKind::OpenUrl,
                    false,
                    false,
                    false,
                    false,
                    true,
                ));
                actions.push(Action::builtin(
                    "open-vscode",
                    "Open in VS Code",
                    'v',
                    "Open file at line in VS Code",
                    ActionKind::OpenEditor,
                    false,
                    false,
                    false,
                    true,
                    true,
                ));
                actions.push(Action::builtin(
                    "open-idea",
                    "Open in IntelliJ",
                    'i',
                    "Open file at line in IntelliJ IDEA",
                    ActionKind::OpenEditor,
                    false,
                    false,
                    false,
                    true,
                    true,
                ));
                actions.push(Action::builtin(
                    "repin",
                    "Re-pin",
                    'u',
                    "Update pinned ref and line to tracking branch tip",
                    ActionKind::RepinCode,
                    false,
                    false,
                    true,
                    true,
                    false,
                ));
                actions.push(Action::builtin(
                    "fetch",
                    "Fetch & Check",
                    'F',
                    "Fetch remote tracking branch and re-check drift",
                    ActionKind::FetchAndCheck,
                    false,
                    false,
                    false,
                    true,
                    false,
                ));
            }
            Some(EntryType::Note) => {
                actions.push(Action::builtin(
                    "copy",
                    "Copy Markdown",
                    'c',
                    "Copy the Markdown source to clipboard",
                    ActionKind::CopyPayload,
                    false,
                    false,
                    false,
                    false,
                    false,
                ));
                actions.push(Action::builtin(
                    "copy-text",
                    "Copy Text",
                    't',
                    "Copy the rendered plain text to clipboard",
                    ActionKind::CopyRenderedText,
                    false,
                    false,
                    false,
                    false,
                    false,
                ));
                actions.push(Action::builtin(
                    "edit",
                    "Edit",
                    'e',
                    "Edit the note in your editor",
                    ActionKind::EditEntry,
                    false,
                    false,
                    false,
                    false,
                    false,
                ));
            }
            Some(EntryType::Sql) | Some(EntryType::Prompt) | None => {
                actions.push(Action::builtin(
                    "copy",
                    "Copy Payload",
                    'c',
                    "Copy content to clipboard",
                    ActionKind::CopyPayload,
                    false,
                    false,
                    false,
                    false,
                    false,
                ));
            }
        }

        // Append custom actions matching entry_type
        let type_name = entry.entry_type.map_or("", |t| t.name());
        let warnings = Self::validate_custom_actions(config.custom_actions());
        for custom in config.custom_actions() {
            if custom.entry_type == type_name {
                let mut disabled_reason = None;
                if let Some(warn) = warnings
                    .iter()
                    .find(|w| w.entry_type == custom.entry_type && w.name == custom.name)
                {
                    disabled_reason = Some(warn.message.clone());
                } else if let Err(missing) =
                    Self::check_custom_action_placeholders(&custom.command, entry)
                {
                    disabled_reason = Some(format!("missing field '{missing}'"));
                }

                actions.push(Action {
                    id: custom.name.clone(),
                    name: custom.name.clone(),
                    key: custom.key,
                    label: format!("Custom action: {}", custom.command),
                    kind: ActionKind::CustomCommand,
                    is_primary: false,
                    needs_vars: true,
                    needs_confirm: true,
                    requires_local_clone: false,
                    requires_display: false,
                    disabled_reason,
                });
            }
        }

        actions
    }

    pub fn primary_action(
        entry: &Entry,
        config: &Settings,
        clone_status: Option<&LocalClone>,
    ) -> Option<Action> {
        let actions = Self::list_actions(entry, config, clone_status);
        actions.into_iter().find(|a| a.is_primary)
    }

    pub fn resolve_custom_action_command(
        command: &str,
        entry: &Entry,
        values: Option<&Values>,
        input: CustomActionInput,
    ) -> Result<String, String> {
        let re = Regex::new(r"\{\{([a-zA-Z0-9_\-]+)\}\}").map_err(|e| e.to_string())?;
        let mut missing_field = None;
        let mut resolved = re
            .replace_all(command, |caps: &regex::Captures| {
                let field = &caps[1];
                match entry_field_value(entry, values, field) {
                    Some(val) => crate::varres::escape::unquoted(&val, crate::entry::Shell::Sh),
                    None => {
                        missing_field = Some(field.to_string());
                        String::new()
                    }
                }
            })
            .to_string();

        if let Some(f) = missing_field {
            return Err(format!("missing field '{f}'"));
        }

        if input == CustomActionInput::Arg {
            let mut payload = entry.payload().map(|p| p.text.clone()).unwrap_or_default();
            if let Some(vals) = values {
                let plan = crate::varres::plan_vars(entry);
                if let Ok(res) = crate::varres::resolve(&plan, vals)
                    && let Some(out) = res.get(&crate::varres::Target::Payload)
                {
                    payload = out.text.clone();
                }
            }
            resolved.push(' ');
            resolved.push_str(&crate::varres::escape::unquoted(
                &payload,
                crate::entry::Shell::Sh,
            ));
        }

        Ok(resolved)
    }

    pub fn check_custom_action_placeholders(command: &str, entry: &Entry) -> Result<(), String> {
        let re = Regex::new(r"\{\{([a-zA-Z0-9_\-]+)\}\}").map_err(|e| e.to_string())?;
        for caps in re.captures_iter(command) {
            let field = &caps[1];
            if entry_field_value(entry, None, field).is_none() {
                return Err(field.to_string());
            }
        }
        Ok(())
    }

    pub fn validate_custom_actions(
        custom_actions: &[CustomActionConfig],
    ) -> Vec<ActionConfigWarning> {
        let mut warnings = Vec::new();
        // Check for conflicts with reserved keys per type
        for custom in custom_actions {
            let reserved: &[char] = match custom.entry_type.as_str() {
                "link" | "sumo" | "cw" => &['c', 'o', '?', 'q'],
                "cmd" => &['c', 'r', '?', 'q'],
                "code" => &['y', 'Y', 'o', 'g', 'v', 'i', 'u', 'F', '?', 'q'],
                "note" => &['c', 't', 'e', 'f', '?', 'q'],
                _ => &['c', '?', 'q'],
            };
            if reserved.contains(&custom.key) {
                warnings.push(ActionConfigWarning {
                    entry_type: custom.entry_type.clone(),
                    name: custom.name.clone(),
                    key: custom.key,
                    message: format!(
                        "custom action '{}' uses reserved shortcut key '{}' for type '{}'",
                        custom.name, custom.key, custom.entry_type
                    ),
                });
            }
        }
        warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Entry;

    fn load_entry(rel: &str) -> Entry {
        let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(rel);
        let bytes = std::fs::read(&full).unwrap();
        crate::entry::parse_entry(rel, rel, bytes).entry.unwrap()
    }

    #[test]
    fn builtin_action_registry_per_type() {
        let link_entry = load_entry("tests/fixtures/actions/entries/link-auth0.md");
        let sumo_entry = load_entry("tests/fixtures/actions/entries/sumo-5xx.md");
        let cw_entry = load_entry("tests/fixtures/actions/entries/cw-latency.md");
        let cmd_entry = load_entry("tests/fixtures/actions/entries/cmd-tail.md");
        let code_entry = load_entry("tests/fixtures/actions/entries/code-authorizer.md");

        let empty_settings =
            crate::config::load(&[], &|_| None, &crate::config::Flags::default()).settings;

        // Verify primary actions
        let link_primary = ActionService::primary_action(&link_entry, &empty_settings, None);
        assert!(link_primary.is_some());
        let link_act = link_primary.unwrap();
        assert_eq!(link_act.id, "open-link");
        assert!(link_act.is_primary);
        assert!(link_act.requires_display);
        assert!(link_act.needs_vars);

        let sumo_primary = ActionService::primary_action(&sumo_entry, &empty_settings, None);
        assert!(sumo_primary.is_some());
        assert_eq!(sumo_primary.unwrap().id, "open-sumo");

        let cw_primary = ActionService::primary_action(&cw_entry, &empty_settings, None);
        assert!(cw_primary.is_some());
        assert_eq!(cw_primary.unwrap().id, "open-cw");

        let cmd_primary = ActionService::primary_action(&cmd_entry, &empty_settings, None);
        assert!(cmd_primary.is_none());

        let code_primary = ActionService::primary_action(&code_entry, &empty_settings, None);
        assert!(code_primary.is_some());
        assert_eq!(code_primary.unwrap().id, "code-view");

        // Verify list_actions
        let cmd_actions = ActionService::list_actions(&cmd_entry, &empty_settings, None);
        assert!(cmd_actions.iter().any(|a| a.id == "copy"));
        let run_act = cmd_actions
            .iter()
            .find(|a| a.id == "run")
            .expect("cmd has run action");
        assert_eq!(run_act.key, 'r');
        assert!(run_act.needs_confirm);
        assert!(run_act.needs_vars);
        assert!(!run_act.requires_display);

        let code_actions = ActionService::list_actions(&code_entry, &empty_settings, None);
        let vscode_act = code_actions
            .iter()
            .find(|a| a.id == "open-vscode")
            .expect("code has vscode action");
        assert!(vscode_act.requires_local_clone);
        assert!(vscode_act.requires_display);

        let copy_act = code_actions
            .iter()
            .find(|a| a.id == "copy")
            .expect("code has copy action");
        assert!(!copy_act.needs_confirm);
        assert!(!copy_act.needs_vars);
        assert!(!copy_act.requires_local_clone);
    }

    fn load_settings_from_toml(toml: &str) -> crate::config::Settings {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, toml).unwrap();
        let loaded = crate::config::load(&[cfg], &|_| None, &crate::config::Flags::default());
        loaded.settings
    }

    #[test]
    fn custom_action_default_input_is_stdin() {
        let toml = r#"
[[actions.sql]]
name = "psql"
key = "p"
command = "psql -d {{database}}"
"#;
        let settings = load_settings_from_toml(toml);
        assert_eq!(settings.custom_actions().len(), 1);
        let act = &settings.custom_actions()[0];
        assert_eq!(act.input, crate::config::CustomActionInput::Stdin);
    }

    #[test]
    fn custom_action_arg_input_appends_payload() {
        let entry = load_entry("tests/fixtures/actions/entries/cmd-tail.md");
        let cmd = ActionService::resolve_custom_action_command(
            "echo [LOG]:",
            &entry,
            None,
            crate::config::CustomActionInput::Arg,
        )
        .unwrap();
        assert!(cmd.starts_with("echo [LOG]: 'tail -f"), "got: {cmd}");
    }

    #[test]
    fn custom_action_placeholders_are_shell_escaped() {
        // Construct an entry with hostile characters in database field
        let doc = r#"---
kb_format: 1
type: sql
database: analytics prod; rm -rf /
---

```sql
SELECT 1;
```
"#;
        let parsed = crate::entry::parse_entry("test.md", "test.md", doc.as_bytes().to_vec());
        let entry = parsed.entry.unwrap();
        let cmd = ActionService::resolve_custom_action_command(
            "psql -d {{database}}",
            &entry,
            None,
            crate::config::CustomActionInput::Stdin,
        )
        .unwrap();
        assert_eq!(cmd, "psql -d 'analytics prod; rm -rf /'");
    }

    #[test]
    fn custom_action_missing_placeholder_field_disables_action() {
        // SQL entry without database field
        let doc = r#"---
kb_format: 1
type: sql
---

```sql
SELECT 1;
```
"#;
        let parsed = crate::entry::parse_entry("test.md", "test.md", doc.as_bytes().to_vec());
        let entry = parsed.entry.unwrap();

        let toml = r#"
[[actions.sql]]
name = "psql"
key = "p"
command = "psql -d {{database}}"
"#;
        let settings = load_settings_from_toml(toml);

        let actions = ActionService::list_actions(&entry, &settings, None);
        let psql_act = actions
            .iter()
            .find(|a| a.id == "psql")
            .expect("psql custom action present");
        assert!(
            !psql_act.is_available(false, false),
            "action should be disabled"
        );
        assert!(psql_act.disabled_reason.is_some());
        assert!(
            psql_act
                .disabled_reason
                .as_ref()
                .unwrap()
                .contains("database"),
            "reason: {:?}",
            psql_act.disabled_reason
        );
    }

    #[test]
    fn custom_action_key_conflict_warns_and_disables() {
        let doc = r#"---
kb_format: 1
type: sql
database: reporting
---

```sql
SELECT 1;
```
"#;
        let parsed = crate::entry::parse_entry("test.md", "test.md", doc.as_bytes().to_vec());
        let entry = parsed.entry.unwrap();

        // 'c' conflicts with built-in copy key
        let toml = r#"
[[actions.sql]]
name = "clash"
key = "c"
command = "echo {{database}}"
"#;
        let settings = load_settings_from_toml(toml);

        let warnings = ActionService::validate_custom_actions(settings.custom_actions());
        assert!(!warnings.is_empty(), "expected key clash warning");
        assert!(warnings[0].message.contains("reserved shortcut key 'c'"));

        let actions = ActionService::list_actions(&entry, &settings, None);
        let clash_act = actions
            .iter()
            .find(|a| a.id == "clash")
            .expect("clash action present");
        assert!(
            !clash_act.is_available(false, false),
            "clashing action should be disabled"
        );
        assert!(clash_act.disabled_reason.is_some());
    }
}
