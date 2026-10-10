//! Action model, registries, and resolution.

use crate::code::LocalClone;
use crate::config::{CustomActionConfig, Settings};
use crate::entry::{Entry, EntryType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    CopyPayload,
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
                actions.push(Action {
                    id: "copy".into(),
                    name: "Copy URL".into(),
                    key: 'c',
                    label: "Copy URL to clipboard".into(),
                    kind: ActionKind::CopyPayload,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: false,
                });
                actions.push(Action {
                    id: "open-link".into(),
                    name: "Open Link".into(),
                    key: 'o',
                    label: "Open link in default browser".into(),
                    kind: ActionKind::OpenUrl,
                    is_primary: true,
                    needs_vars: true,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: true,
                });
            }
            Some(EntryType::Sumo) => {
                actions.push(Action {
                    id: "copy".into(),
                    name: "Copy Query".into(),
                    key: 'c',
                    label: "Copy Sumo query to clipboard".into(),
                    kind: ActionKind::CopyPayload,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: false,
                });
                actions.push(Action {
                    id: "open-sumo".into(),
                    name: "Open in Sumo".into(),
                    key: 'o',
                    label: "Open deep link in Sumo Logic".into(),
                    kind: ActionKind::OpenUrl,
                    is_primary: true,
                    needs_vars: true,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: true,
                });
            }
            Some(EntryType::Cw) => {
                actions.push(Action {
                    id: "copy".into(),
                    name: "Copy Query".into(),
                    key: 'c',
                    label: "Copy CloudWatch query to clipboard".into(),
                    kind: ActionKind::CopyPayload,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: false,
                });
                actions.push(Action {
                    id: "open-cw".into(),
                    name: "Open in CloudWatch".into(),
                    key: 'o',
                    label: "Open deep link in CloudWatch Logs Insights".into(),
                    kind: ActionKind::OpenUrl,
                    is_primary: true,
                    needs_vars: true,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: true,
                });
            }
            Some(EntryType::Cmd) => {
                actions.push(Action {
                    id: "copy".into(),
                    name: "Copy Command".into(),
                    key: 'c',
                    label: "Copy command to clipboard".into(),
                    kind: ActionKind::CopyPayload,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: false,
                });
                actions.push(Action {
                    id: "run".into(),
                    name: "Run Command".into(),
                    key: 'r',
                    label: "Run command safely in terminal".into(),
                    kind: ActionKind::RunCommand,
                    is_primary: false,
                    needs_vars: true,
                    needs_confirm: true,
                    requires_local_clone: false,
                    requires_display: false,
                });
            }
            Some(EntryType::Code) => {
                actions.push(Action {
                    id: "copy".into(),
                    name: "Copy Location".into(),
                    key: 'y',
                    label: "Copy path:line to clipboard".into(),
                    kind: ActionKind::CopyCodeLocation,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: false,
                });
                actions.push(Action {
                    id: "copy-permalink".into(),
                    name: "Copy Permalink".into(),
                    key: 'Y',
                    label: "Copy web permalink to clipboard".into(),
                    kind: ActionKind::CopyWebPermalink,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: false,
                });
                actions.push(Action {
                    id: "code-view".into(),
                    name: "Code View".into(),
                    key: 'o',
                    label: "Open code preview with drift status".into(),
                    kind: ActionKind::CodeView,
                    is_primary: true,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: false,
                });
                actions.push(Action {
                    id: "open-web".into(),
                    name: "Open in Browser".into(),
                    key: 'g',
                    label: "Open web permalink in browser".into(),
                    kind: ActionKind::OpenUrl,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: true,
                });
                actions.push(Action {
                    id: "open-vscode".into(),
                    name: "Open in VS Code".into(),
                    key: 'v',
                    label: "Open file at line in VS Code".into(),
                    kind: ActionKind::OpenEditor,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: true,
                    requires_display: true,
                });
                actions.push(Action {
                    id: "open-idea".into(),
                    name: "Open in IntelliJ".into(),
                    key: 'i',
                    label: "Open file at line in IntelliJ IDEA".into(),
                    kind: ActionKind::OpenEditor,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: true,
                    requires_display: true,
                });
                actions.push(Action {
                    id: "repin".into(),
                    name: "Re-pin".into(),
                    key: 'u',
                    label: "Update pinned ref and line to tracking branch tip".into(),
                    kind: ActionKind::RepinCode,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: true,
                    requires_local_clone: true,
                    requires_display: false,
                });
                actions.push(Action {
                    id: "fetch".into(),
                    name: "Fetch & Check".into(),
                    key: 'F',
                    label: "Fetch remote tracking branch and re-check drift".into(),
                    kind: ActionKind::FetchAndCheck,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: true,
                    requires_display: false,
                });
            }
            Some(EntryType::Sql) | Some(EntryType::Prompt) | None => {
                actions.push(Action {
                    id: "copy".into(),
                    name: "Copy Payload".into(),
                    key: 'c',
                    label: "Copy content to clipboard".into(),
                    kind: ActionKind::CopyPayload,
                    is_primary: false,
                    needs_vars: false,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: false,
                });
            }
        }

        // Append custom actions matching entry_type
        let type_name = entry.entry_type.map_or("", |t| t.name());
        for custom in config.custom_actions() {
            if custom.entry_type == type_name {
                actions.push(Action {
                    id: custom.name.clone(),
                    name: custom.name.clone(),
                    key: custom.key,
                    label: format!("Custom action: {}", custom.command),
                    kind: ActionKind::CustomCommand,
                    is_primary: false,
                    needs_vars: true,
                    needs_confirm: false,
                    requires_local_clone: false,
                    requires_display: false,
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
}
