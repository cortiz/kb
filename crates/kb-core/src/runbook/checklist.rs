//! Markdown checklist generation for runbooks.

use crate::entry::Entry;
use crate::runbook::StepTarget;

/// Generates a plain Markdown checklist for a runbook (FR-033, FR-034).
pub fn generate_checklist(runbook: &Entry) -> String {
    let mut out = String::new();
    let raw_title = runbook.title().unwrap_or(runbook.id());
    let title = crate::terminal::raw(raw_title);
    out.push_str(&format!("# {}\n\n", title));

    if let Some(steps) = runbook.runbook_steps() {
        for (i, step) in steps.iter().enumerate() {
            let num = i + 1;
            let opt_str = if step.optional { " (optional)" } else { "" };
            let step_title = crate::terminal::raw(&step.title);
            match &step.target {
                StepTarget::Entry(entry_id) => {
                    out.push_str(&format!("- [ ] {}. {} (`{}`){}\n", num, step_title, entry_id, opt_str));
                }
                StepTarget::Do(instruction) => {
                    out.push_str(&format!("- [ ] {}. {}{}\n", num, step_title, opt_str));
                    let clean_inst = crate::terminal::raw(instruction);
                    for line in clean_inst.lines() {
                        out.push_str(&format!("  {}\n", line));
                    }
                }
            }
        }
    }

    out
}

/// Generates a resolved Markdown checklist for a runbook (FR-033, FR-034).
/// Interpolates step titles, inlines each referenced entry's resolved payload inside a fenced code block,
/// and shields secret variables as `$ENV_VAR` or `{{name}}`.
pub fn generate_resolved_checklist<F>(
    runbook: &Entry,
    runbook_values: &std::collections::HashMap<String, String>,
    resolve_step_entry: F,
) -> String
where
    F: Fn(&str, &std::collections::HashMap<String, String>) -> Option<(String, &'static str)>,
{
    let mut out = String::new();
    let raw_title = runbook.title().unwrap_or(runbook.id());
    let mut title = raw_title.to_string();
    for (k, v) in runbook_values {
        title = title.replace(&format!("{{{{{k}}}}}"), v);
    }
    let title = crate::terminal::raw(&title);
    out.push_str(&format!("# {}\n\n", title));

    if let Some(steps) = runbook.runbook_steps() {
        for (i, step) in steps.iter().enumerate() {
            let num = i + 1;
            let opt_str = if step.optional { " (optional)" } else { "" };
            let mut step_title = step.title.clone();
            for (k, v) in runbook_values {
                step_title = step_title.replace(&format!("{{{{{k}}}}}"), v);
            }
            let step_title = crate::terminal::raw(&step_title);

            match &step.target {
                StepTarget::Entry(entry_id) => {
                    out.push_str(&format!("- [ ] {}. {} (`{}`){}\n", num, step_title, entry_id, opt_str));

                    // Compute step variables
                    let mut step_vars = std::collections::HashMap::new();
                    // Implicit bindings from runbook vars
                    for (k, v) in runbook_values {
                        step_vars.insert(k.clone(), v.clone());
                    }
                    // Explicit bindings
                    for (target_k, binding) in &step.vars {
                        match binding {
                            crate::runbook::StepVarBinding::RunbookVar(source_name) => {
                                if let Some(v) = runbook_values.get(source_name) {
                                    step_vars.insert(target_k.clone(), v.clone());
                                }
                            }
                            crate::runbook::StepVarBinding::Literal(lit) => {
                                let s = match lit {
                                    serde_json::Value::String(s) => s.clone(),
                                    _ => lit.to_string(),
                                };
                                step_vars.insert(target_k.clone(), s);
                            }
                        }
                    }

                    if let Some((payload, fence)) = resolve_step_entry(entry_id, &step_vars) {
                        out.push_str(&format!("\n```{fence}\n{}\n```\n", payload.trim_end()));
                    }
                }
                StepTarget::Do(instruction) => {
                    out.push_str(&format!("- [ ] {}. {}{}\n", num, step_title, opt_str));
                    let mut resolved_inst = instruction.clone();
                    for (k, v) in runbook_values {
                        resolved_inst = resolved_inst.replace(&format!("{{{{{k}}}}}"), v);
                    }
                    let clean_inst = crate::terminal::raw(&resolved_inst);
                    for line in clean_inst.lines() {
                        out.push_str(&format!("  {}\n", line));
                    }
                }
            }
        }
    }

    out
}

/// Returns the language identifier for a code fence block for `entry`.
pub fn entry_fence_info(entry: &Entry) -> &'static str {
    match entry.entry_type() {
        Some(crate::entry::EntryType::Sql) => "sql",
        Some(crate::entry::EntryType::Sumo) => "sumo",
        Some(crate::entry::EntryType::Cw) => "cw",
        Some(crate::entry::EntryType::Cmd) => match entry.fields() {
            crate::entry::TypeFields::Cmd { shell, .. } => match shell {
                crate::entry::Shell::Sh => "sh",
                crate::entry::Shell::Bash => "bash",
                crate::entry::Shell::Zsh => "zsh",
                crate::entry::Shell::Fish => "fish",
            },
            _ => "sh",
        },
        _ => "",
    }
}
