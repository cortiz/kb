//! Runbooks: ordered sequence of steps referencing existing entries or manual instructions.

pub mod binding;
pub mod checklist;
pub mod lint;

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::entry::Sink;
use crate::lint::RuleCode;
use crate::source::Span;
use crate::yaml::Node;

/// The target of a runbook step: either a referenced entry ID or manual instruction text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepTarget {
    Entry(String),
    Do(String),
}

impl StepTarget {
    pub fn entry_id(&self) -> Option<&str> {
        match self {
            StepTarget::Entry(id) => Some(id),
            StepTarget::Do(_) => None,
        }
    }

    pub fn is_entry(&self) -> bool {
        matches!(self, StepTarget::Entry(_))
    }

    pub fn is_do(&self) -> bool {
        matches!(self, StepTarget::Do(_))
    }
}

/// A variable binding on a runbook step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepVarBinding {
    /// Bound to a runbook-level variable: "{{var_name}}"
    RunbookVar(String),
    /// Bound to a literal constant value
    Literal(serde_json::Value),
}

/// A single step in a runbook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunbookStep {
    /// Step identifier unique within the runbook (`[a-z0-9][a-z0-9-]*`).
    pub id: String,
    /// Whether `id` was explicitly specified in the frontmatter.
    pub id_explicit: bool,
    /// Step title (1-120 chars, may contain {{var}} references).
    pub title: String,
    /// Step target: entry ID or manual text.
    pub target: StepTarget,
    /// Optional guidance note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Whether this step is optional (default false).
    #[serde(default)]
    pub optional: bool,
    /// Whether this step requires explicit confirmation in R2 (default false).
    #[serde(default)]
    pub confirm: bool,
    /// Variable bindings for this step.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, StepVarBinding>,
    /// Byte span of the step node in the source file.
    #[serde(skip)]
    pub span: Span,
    /// Byte span of the title in the source file.
    #[serde(skip)]
    pub title_span: Span,
    /// Byte span of the target (`entry:` or `do:`) in the source file.
    #[serde(skip)]
    pub target_span: Span,
    /// Byte span of the explicit `id:`, if present.
    #[serde(skip)]
    pub id_span: Option<Span>,
    /// Byte spans of explicit variable bindings.
    #[serde(skip)]
    pub vars_spans: BTreeMap<String, Span>,
}

/// Derives a slug from a step title.
pub fn slugify_step_title(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "step".to_string()
    } else {
        slug
    }
}

/// Checks whether a step ID is valid: `^[a-z0-9][a-z0-9-]*$`.
pub fn is_valid_step_id(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let first = bytes[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    bytes
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Parses the `steps:` sequence of a runbook.
pub(crate) fn parse_steps(node: &Node, sink: &mut Sink<'_>) -> Vec<RunbookStep> {
    let Node::Seq { items, span, .. } = node else {
        sink.push(
            RuleCode::WrongValueType,
            node.span().start,
            format!("`steps` must be a list, not a {}", node.kind_name()),
            Some("write `steps:` followed by a `- id: ...` list".into()),
        );
        return Vec::new();
    };

    if items.is_empty() {
        sink.push(
            RuleCode::RunbookNoSteps,
            span.start,
            "a runbook must have at least one step",
            Some("add one or more `- title: ...` steps under `steps:`".into()),
        );
        return Vec::new();
    }

    if items.len() > 100 {
        sink.push(
            RuleCode::TooManySteps,
            span.start,
            format!("runbook has {} steps; the limit is 100", items.len()),
            Some("split into multiple smaller runbooks".into()),
        );
    } else if items.len() > 50 {
        sink.push(
            RuleCode::LongRunbook,
            span.start,
            format!(
                "runbook has {} steps; more than 50 is hard to follow under pressure",
                items.len()
            ),
            Some("consider breaking this runbook into shorter phases".into()),
        );
    }

    let mut steps = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    for (idx, item) in items.iter().enumerate() {
        let Node::Map { .. } = item else {
            sink.push(
                RuleCode::WrongValueType,
                item.span().start,
                format!("step must be a mapping, not a {}", item.kind_name()),
                Some("format each step as `- title: ...` with `entry:` or `do:`".into()),
            );
            continue;
        };

        // 1. Title
        let title_node = item.get("title");
        let (title, title_span) = match title_node {
            Some(node) => {
                let s = node.as_str().unwrap_or("").trim();
                let sp = node.span().clone();
                if s.is_empty() {
                    sink.push(
                        RuleCode::MissingTitle,
                        sp.start,
                        "step title cannot be empty",
                        Some("add a descriptive `title:` for this step".into()),
                    );
                    (format!("Step {}", idx + 1), sp)
                } else if s.chars().count() > crate::entry::MAX_TITLE_CHARS {
                    sink.push(
                        RuleCode::InvalidTitle,
                        sp.start,
                        format!(
                            "step title is {} characters long; the limit is {}",
                            s.chars().count(),
                            crate::entry::MAX_TITLE_CHARS
                        ),
                        Some("shorten step title".into()),
                    );
                    (s.to_string(), sp)
                } else {
                    (s.to_string(), sp)
                }
            }
            None => {
                sink.push(
                    RuleCode::MissingTitle,
                    item.span().start,
                    "each step needs a `title`",
                    Some("add `title: ...` to this step".into()),
                );
                (format!("Step {}", idx + 1), item.span().clone())
            }
        };

        // 2. Target: exactly one of `entry` or `do`
        let entry_node = item.get("entry");
        let do_node = item.get("do");

        let (target, target_span) = match (entry_node, do_node) {
            (Some(e), None) => {
                let id = e.as_str().unwrap_or("").trim().to_string();
                let sp = e.span().clone();
                (StepTarget::Entry(id), sp)
            }
            (None, Some(d)) => {
                let text = d.as_str().unwrap_or("").trim().to_string();
                let sp = d.span().clone();
                (StepTarget::Do(text), sp)
            }
            (Some(e), Some(_)) => {
                sink.push(
                    RuleCode::StepEntryAndDo,
                    e.span().start,
                    "a step cannot have both `entry:` and `do:`",
                    Some("specify either `entry:` for an existing entry or `do:` for manual instructions".into()),
                );
                let id = e.as_str().unwrap_or("").trim().to_string();
                (StepTarget::Entry(id), e.span().clone())
            }
            (None, None) => {
                sink.push(
                    RuleCode::StepEntryAndDo,
                    item.span().start,
                    "a step must have either `entry:` or `do:`",
                    Some("specify `entry: <id>` or `do: <manual instruction>`".into()),
                );
                (StepTarget::Do(String::new()), item.span().clone())
            }
        };

        // 3. ID (explicit or derived)
        let id_node = item.get("id");
        let (id, id_explicit, id_span) = if let Some(node) = id_node {
            let raw = node.as_str().unwrap_or("").trim();
            let sp = node.span().clone();
            if !is_valid_step_id(raw) {
                sink.push(
                    RuleCode::InvalidStepId,
                    sp.start,
                    format!("step id `{raw}` is invalid: must match `[a-z0-9][a-z0-9-]*`"),
                    Some("use lowercase alphanumeric characters and hyphens, starting with a letter or digit".into()),
                );
            }
            (raw.to_string(), true, Some(sp))
        } else {
            // Implicit ID derived from title
            let base_slug = slugify_step_title(&title);
            let mut derived = base_slug.clone();
            let mut counter = 2;
            while seen_ids.contains(&derived) {
                derived = format!("{base_slug}-{counter}");
                counter += 1;
            }
            sink.push(
                RuleCode::ImplicitStepId,
                item.span().start,
                format!("step omits explicit `id`; derived `{derived}` from title"),
                Some(format!("add `id: {derived}` explicitly")),
            );
            (derived, false, None)
        };

        // Check duplicate step ID
        if seen_ids.contains(&id) {
            let at = id_span.as_ref().map_or(item.span().start, |s| s.start);
            sink.push(
                RuleCode::DuplicateStepId,
                at,
                format!("duplicate step id `{id}` in runbook"),
                Some("make each step id unique within the runbook".into()),
            );
        } else {
            seen_ids.insert(id.clone());
        }

        // 4. Note
        let note = item
            .get("note")
            .and_then(|n| n.as_str().map(|s| s.trim().to_string()));

        // 5. Optional
        let optional = item
            .get("optional")
            .and_then(|n| n.as_bool())
            .unwrap_or(false);

        // 6. Confirm
        let confirm = item
            .get("confirm")
            .and_then(|n| n.as_bool())
            .unwrap_or(false);

        // 7. Step vars
        let mut vars = BTreeMap::new();
        let mut vars_spans = BTreeMap::new();
        if let Some(vars_node) = item.get("vars") {
            if let Node::Map { entries, .. } = vars_node {
                for (k, v) in entries {
                    let var_name = k.name.clone();
                    vars_spans.insert(var_name.clone(), v.span().clone());
                    if let Some(s) = v.as_str() {
                        let trimmed = s.trim();
                        if let Some(stripped) = trimmed
                            .strip_prefix("{{")
                            .and_then(|rest| rest.strip_suffix("}}"))
                        {
                            vars.insert(
                                var_name,
                                StepVarBinding::RunbookVar(stripped.trim().to_string()),
                            );
                            continue;
                        }
                    }
                    // Literal value
                    let val = match v {
                        Node::Scalar { value, .. } => {
                            if let Some(b) = v.as_bool() {
                                serde_json::Value::Bool(b)
                            } else if let Some(i) = v.as_int() {
                                serde_json::Value::Number(i.into())
                            } else {
                                serde_json::Value::String(value.clone())
                            }
                        }
                        _ => serde_json::Value::Null,
                    };
                    vars.insert(var_name, StepVarBinding::Literal(val));
                }
            } else {
                sink.push(
                    RuleCode::WrongValueType,
                    vars_node.span().start,
                    format!(
                        "step `vars` must be a mapping, not a {}",
                        vars_node.kind_name()
                    ),
                    Some("write `vars:` with `name: value` pairs".into()),
                );
            }
        }

        steps.push(RunbookStep {
            id,
            id_explicit,
            title,
            target,
            note,
            optional,
            confirm,
            vars,
            span: item.span().clone(),
            title_span,
            target_span,
            id_span,
            vars_spans,
        });
    }

    steps
}

/// Rewrites `entry:` references pointing at `old_id` to `new_id` within the runbook's frontmatter,
/// preserving formatting, comments, and all other fields.
/// Returns the new file text if any reference was changed, or `None` if `old_id` was not found.
pub fn retarget_runbook_step_references(text: &str, old_id: &str, new_id: &str) -> Option<String> {
    let outcome = crate::entry::parse_source(
        "temp",
        "temp.md",
        crate::source::Source::from_text(text.into(), false),
    );
    let entry = outcome.entry?;
    let crate::entry::TypeFields::Runbook { steps } = entry.fields() else {
        return None;
    };

    let mut replacements = Vec::new();
    for step in steps {
        if let StepTarget::Entry(ref id) = step.target
            && id == old_id
        {
            replacements.push(step.target_span.clone());
        }
    }

    if replacements.is_empty() {
        return None;
    }

    // Sort descending by start offset so earlier replacements don't shift later spans
    replacements.sort_by_key(|s| std::cmp::Reverse(s.start));

    let mut result = text.to_string();
    for span in replacements {
        let original = &result[span.clone()];
        let replacement = if original.starts_with('"') && original.ends_with('"') {
            format!("\"{new_id}\"")
        } else if original.starts_with('\'') && original.ends_with('\'') {
            format!("'{new_id}'")
        } else {
            new_id.to_string()
        };
        result.replace_range(span, &replacement);
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify_step_title("Check error rate!"), "check-error-rate");
        assert_eq!(
            slugify_step_title("Deploy {{version}} to {{env}}"),
            "deploy-version-to-env"
        );
        assert_eq!(slugify_step_title("   "), "step");
    }

    #[test]
    fn test_valid_step_id() {
        assert!(is_valid_step_id("baseline"));
        assert!(is_valid_step_id("deploy-authorizer"));
        assert!(is_valid_step_id("step-1"));
        assert!(!is_valid_step_id(""));
        assert!(!is_valid_step_id("-invalid"));
        assert!(!is_valid_step_id("Invalid"));
        assert!(!is_valid_step_id("has space"));
    }

    #[test]
    fn test_parse_steps_valid() {
        let text = r#"
steps:
  - id: baseline
    title: Check error rate
    entry: sumo/5xx
    confirm: true
    vars:
      env: "{{runbook_env}}"
      count: 10
  - title: Manual step
    do: "Check slack channel"
    optional: true
"#;
        let parsed = crate::yaml::parse(text, 0);
        let node = parsed.root.unwrap();
        let steps_node = node.get("steps").unwrap();
        let src = crate::source::Source::from_text(text.into(), false);
        let mut sink = crate::entry::Sink {
            path: "runbook.md",
            id: "runbook",
            source: &src,
            findings: Vec::new(),
        };
        let steps = parse_steps(steps_node, &mut sink);

        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].id, "baseline");
        assert!(steps[0].id_explicit);
        assert_eq!(steps[0].title, "Check error rate");
        assert_eq!(steps[0].target, StepTarget::Entry("sumo/5xx".into()));
        assert!(steps[0].confirm);
        assert!(!steps[0].optional);
        assert_eq!(
            steps[0].vars.get("env"),
            Some(&StepVarBinding::RunbookVar("runbook_env".into()))
        );
        assert_eq!(
            steps[0].vars.get("count"),
            Some(&StepVarBinding::Literal(serde_json::json!(10)))
        );

        assert_eq!(steps[1].id, "manual-step");
        assert!(!steps[1].id_explicit);
        assert_eq!(steps[1].title, "Manual step");
        assert_eq!(
            steps[1].target,
            StepTarget::Do("Check slack channel".into())
        );
        assert!(steps[1].optional);
        assert!(!steps[1].confirm);
        assert!(
            sink.findings
                .iter()
                .any(|f| f.code == RuleCode::ImplicitStepId)
        );
    }

    #[test]
    fn test_parse_steps_errors() {
        let text = r#"
steps:
  - id: s1
    title: Both entry and do
    entry: cmd/restart
    do: "restart manually"
  - id: s2
    entry: cmd/restart
  - id: s1
    title: Duplicate ID
    do: "duplicate"
"#;
        let parsed = crate::yaml::parse(text, 0);
        let node = parsed.root.unwrap();
        let steps_node = node.get("steps").unwrap();
        let src = crate::source::Source::from_text(text.into(), false);
        let mut sink = crate::entry::Sink {
            path: "runbook.md",
            id: "runbook",
            source: &src,
            findings: Vec::new(),
        };
        let steps = parse_steps(steps_node, &mut sink);

        assert_eq!(steps.len(), 3);
        let codes: Vec<_> = sink.findings.iter().map(|f| f.code).collect();
        assert!(codes.contains(&RuleCode::StepEntryAndDo));
        assert!(codes.contains(&RuleCode::MissingTitle));
        assert!(codes.contains(&RuleCode::DuplicateStepId));
    }

    #[test]
    fn test_retarget_runbook_step_references() {
        let text = r#"---
kb_format: 1
type: runbook
title: Deploy authorizer
steps:
  # Initial check
  - id: baseline
    title: Check 5xx
    entry: sumo/5xx-authorizer
    confirm: true
  - id: rollback
    title: Roll back
    do: "run rollback"
---

Notes here.
"#;
        let retargeted =
            retarget_runbook_step_references(text, "sumo/5xx-authorizer", "sumo/authorizer/5xx")
                .expect("should rewrite reference");

        assert!(retargeted.contains("entry: sumo/authorizer/5xx"));
        assert!(retargeted.contains("# Initial check"));
        assert!(retargeted.contains("confirm: true"));
        assert!(retargeted.contains("Notes here."));

        let none = retarget_runbook_step_references(text, "other/nonexistent", "foo");
        assert!(none.is_none());
    }
}
