//! Variable lifting and content generation for `kb runbook new`.

use std::collections::HashSet;

use crate::entry::Entry;
use crate::runbook::slugify_step_title;
use crate::vars::{VarKind, Variable};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftedVar {
    pub name: String,
    pub kind: VarKind,
    pub default: Option<String>,
    pub options: Vec<String>,
    pub description: Option<String>,
}

/// Lifts variables that appear in two or more steps with compatible kinds (FR-027).
/// Enum options become the intersection of the steps' options.
/// The default is set only if all steps share it.
pub fn lift_variables(entries: &[&Entry]) -> Vec<LiftedVar> {
    let mut vars_by_name: Vec<(String, Vec<&Variable>)> = Vec::new();

    for entry in entries {
        for var in entry.vars() {
            if let Some((_, list)) = vars_by_name.iter_mut().find(|(name, _)| name == &var.name) {
                list.push(var);
            } else {
                vars_by_name.push((var.name.clone(), vec![var]));
            }
        }
    }

    let mut lifted = Vec::new();

    for (name, occs) in vars_by_name {
        // Only lift when variable appears in 2 or more steps
        if occs.len() < 2 {
            continue;
        }

        let all_enum = occs.iter().all(|v| v.kind == VarKind::Enum);
        let all_number = occs.iter().all(|v| v.kind == VarKind::Number);
        let all_secret = occs.iter().all(|v| v.kind == VarKind::Secret);
        let all_text = occs.iter().all(|v| v.kind == VarKind::Text);
        let num_and_text = occs
            .iter()
            .all(|v| v.kind == VarKind::Number || v.kind == VarKind::Text);
        let enum_and_text = occs
            .iter()
            .all(|v| v.kind == VarKind::Enum || v.kind == VarKind::Text);

        let (kind, options) = if all_enum {
            let mut opts = occs[0].options.clone();
            for other in &occs[1..] {
                opts.retain(|o| other.options.contains(o));
            }
            if opts.is_empty() {
                continue; // Empty intersection -> incompatible
            }
            (VarKind::Enum, opts)
        } else if all_number {
            (VarKind::Number, Vec::new())
        } else if all_secret {
            (VarKind::Secret, Vec::new())
        } else if all_text {
            (VarKind::Text, Vec::new())
        } else if num_and_text {
            (VarKind::Number, Vec::new())
        } else if enum_and_text {
            let mut opts = occs
                .iter()
                .find(|v| v.kind == VarKind::Enum)
                .unwrap()
                .options
                .clone();
            for other in &occs {
                if other.kind == VarKind::Enum {
                    opts.retain(|o| other.options.contains(o));
                }
            }
            if opts.is_empty() {
                continue;
            }
            (VarKind::Enum, opts)
        } else {
            continue; // Incompatible
        };

        let default = if kind == VarKind::Secret {
            None
        } else if occs.iter().all(|v| v.default == occs[0].default) {
            let d = occs[0].default.clone();
            if kind == VarKind::Enum {
                if let Some(ref val) = d {
                    if !options.contains(val) {
                        None
                    } else {
                        d
                    }
                } else {
                    None
                }
            } else {
                d
            }
        } else {
            None
        };

        let description = occs
            .iter()
            .find_map(|v| v.description.as_ref())
            .map(String::clone);

        lifted.push(LiftedVar {
            name,
            kind,
            default,
            options,
            description,
        });
    }

    lifted
}

/// Generates YAML text for a new runbook entry from title, tags, and entries.
pub fn generate_runbook_content(
    title: &str,
    tags: &[String],
    entries: &[(&str, &Entry)],
) -> String {
    let mut out = String::from("---\nkb_format: 1\ntype: runbook\n");
    out.push_str(&format!("title: {title}\n"));
    if !tags.is_empty() {
        out.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }

    let entry_refs: Vec<&Entry> = entries.iter().map(|(_, e)| *e).collect();
    let lifted = lift_variables(&entry_refs);

    if !lifted.is_empty() {
        out.push_str("vars:\n");
        for var in &lifted {
            out.push_str(&format!("  {}: ", var.name));
            let mut fields = vec![format!("kind: {}", var.kind.name())];
            if !var.options.is_empty() {
                fields.push(format!("options: [{}]", var.options.join(", ")));
            }
            if let Some(ref d) = var.default {
                fields.push(format!("default: {d}"));
            }
            if let Some(ref desc) = var.description {
                fields.push(format!("description: \"{desc}\""));
            }
            out.push_str(&format!("{{ {} }}\n", fields.join(", ")));
        }
    }

    out.push_str("steps:\n");
    let mut used_ids: HashSet<String> = HashSet::new();

    for (entry_id, entry) in entries {
        let step_title = entry.title().unwrap_or(entry_id);
        let base_slug = slugify_step_title(step_title);
        let mut slug = base_slug.clone();
        let mut counter = 2;
        while used_ids.contains(&slug) {
            slug = format!("{base_slug}-{counter}");
            counter += 1;
        }
        used_ids.insert(slug.clone());

        out.push_str(&format!("  - id: {slug}\n"));
        out.push_str(&format!("    title: {step_title}\n"));
        out.push_str(&format!("    entry: {entry_id}\n"));
    }

    out.push_str("---\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::parse_entry;

    #[test]
    fn test_variable_lifting_and_content_generation() {
        let e1_text = r#"---
kb_format: 1
type: cmd
title: Deploy service
vars:
  env: { kind: enum, options: [dev, stage, prod], default: stage }
  cluster: { kind: text }
---
echo deploy
"#;
        let e2_text = r#"---
kb_format: 1
type: cw
title: Watch latency
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  cluster: { kind: text }
  minutes: { kind: number }
---
filter latency
"#;
        let e1 = parse_entry("cmd/deploy", "cmd/deploy.md", e1_text.as_bytes().to_vec())
            .entry
            .unwrap();
        let e2 = parse_entry("cw/watch", "cw/watch.md", e2_text.as_bytes().to_vec())
            .entry
            .unwrap();

        let entries = vec![("cmd/deploy", &e1), ("cw/watch", &e2)];
        let content = generate_runbook_content(
            "Deploy and Watch",
            &["deploy".into(), "ops".into()],
            &entries,
        );

        assert!(content.contains("type: runbook"));
        assert!(content.contains("title: Deploy and Watch"));
        assert!(content.contains("tags: [deploy, ops]"));
        // env was lifted with intersection [stage, prod] and shared default stage
        assert!(content.contains("env: { kind: enum, options: [stage, prod], default: stage }"));
        // cluster was lifted as text
        assert!(content.contains("cluster: { kind: text }"));
        // minutes was NOT lifted (only in e2)
        assert!(!content.contains("minutes:"));
        // steps
        assert!(content.contains("- id: deploy-service\n    title: Deploy service\n    entry: cmd/deploy"));
        assert!(content.contains("- id: watch-latency\n    title: Watch latency\n    entry: cw/watch"));
    }
}
