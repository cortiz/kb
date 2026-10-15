//! Cross-entry lint checks for runbooks (KB082, KB083, KB086, KB087, KB088, KB091, KB092, KB093).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::entry::{Entry, EntryType, TypeFields};
use crate::lint::{Finding, RuleCode, Severity};
use crate::runbook::binding::{check_kind_compatibility, check_literal_validity};
use crate::runbook::{StepTarget, StepVarBinding};

/// Runs all cross-entry checks on runbooks across the KB.
pub fn check_runbooks(
    root: &Path,
    runbooks: &[Entry],
    files_with_errors: &HashSet<String>,
) -> Vec<Finding> {
    let all = crate::walk::walk(root, &[root.to_path_buf()]).map(|w| w.files);
    let Ok(all_files) = all else {
        return Vec::new();
    };

    let file_map: HashMap<&str, &crate::walk::WalkEntry> =
        all_files.iter().map(|f| (f.id.as_str(), f)).collect();
    let known_ids: Vec<&str> = all_files.iter().map(|f| f.id.as_str()).collect();

    let mut out = Vec::new();
    let mut parsed_cache: HashMap<String, Option<crate::entry::ParseOutcome>> = HashMap::new();

    for runbook in runbooks {
        let TypeFields::Runbook { steps } = runbook.fields() else {
            continue;
        };

        let mut used_rb_vars = HashSet::new();

        // 1. Mark vars used in step titles and manual do text and notes
        for var in runbook.vars() {
            let needle = format!("{{{{{}}}}}", var.name);
            for step in steps {
                if step.title.contains(&needle) {
                    used_rb_vars.insert(var.name.clone());
                }
                if let StepTarget::Do(ref text) = step.target
                    && text.contains(&needle)
                {
                    used_rb_vars.insert(var.name.clone());
                }
            }
            if runbook.notes().contains(&needle) {
                used_rb_vars.insert(var.name.clone());
            }
        }

        // 2. Iterate each step
        for step in steps {
            let target_id = match &step.target {
                StepTarget::Entry(id) => id,
                StepTarget::Do(_) => continue,
            };

            // Does entry exist?
            let Some(file_entry) = file_map.get(target_id.as_str()) else {
                let (line, column) = runbook.source().position(step.target_span.start);
                let (state, _) = crate::usage::State::load(&root.join(".kb"));
                let renamed_id: Option<&str> = state.usage.get(target_id).and_then(|rec| {
                    for (candidate_id, candidate_entry) in file_map.iter() {
                        if *candidate_id == target_id {
                            continue;
                        }
                        if let Ok(bytes) = fs::read(&candidate_entry.abs_path)
                            && crate::usage::content_hash(&bytes) == rec.hash
                        {
                            return Some(*candidate_id);
                        }
                    }
                    None
                });
                let suggested =
                    renamed_id.or_else(|| crate::lint::suggest::suggest(target_id, &known_ids));
                let fix = match suggested {
                    Some(s) => format!("did you mean `{s}`?"),
                    None => "check the entry ID or create the entry".to_string(),
                };
                out.push(Finding {
                    path: runbook.path().to_string(),
                    id: Some(runbook.id().to_string()),
                    line,
                    column,
                    severity: RuleCode::StepMissingEntry.severity(),
                    code: RuleCode::StepMissingEntry,
                    message: format!(
                        "referenced entry `{}` does not exist",
                        crate::terminal::display(target_id)
                    ),
                    fix: Some(fix),
                });
                continue;
            };

            // Parse entry if not already cached
            let parsed_outcome = parsed_cache.entry(target_id.clone()).or_insert_with(|| {
                fs::read(&file_entry.abs_path)
                    .ok()
                    .map(|bytes| crate::entry::parse_entry(target_id, &file_entry.rel_path, bytes))
            });

            let Some(outcome) = parsed_outcome else {
                continue;
            };

            // Check if step entry is itself a runbook (KB083)
            let is_runbook =
                outcome.entry.as_ref().and_then(|e| e.entry_type()) == Some(EntryType::Runbook);

            if is_runbook {
                let (line, column) = runbook.source().position(step.target_span.start);
                out.push(Finding {
                    path: runbook.path().to_string(),
                    id: Some(runbook.id().to_string()),
                    line,
                    column,
                    severity: RuleCode::StepIsRunbook.severity(),
                    code: RuleCode::StepIsRunbook,
                    message: format!(
                        "step references runbook `{target_id}`; nested runbooks are not allowed"
                    ),
                    fix: Some("reference the individual entries the runbook uses instead".into()),
                });
                continue;
            }

            // Check if step entry has lint errors (KB091)
            let has_errors = files_with_errors.contains(target_id)
                || outcome
                    .findings
                    .iter()
                    .any(|f| f.severity == Severity::Error);

            if has_errors {
                let (line, column) = runbook.source().position(step.target_span.start);
                out.push(Finding {
                    path: runbook.path().to_string(),
                    id: Some(runbook.id().to_string()),
                    line,
                    column,
                    severity: RuleCode::StepEntryHasErrors.severity(),
                    code: RuleCode::StepEntryHasErrors,
                    message: format!("referenced entry `{target_id}` has lint errors"),
                    fix: Some(format!("fix errors in `{target_id}` with `kb lint`")),
                });
            }

            // Variable bindings
            let Some(ref target_entry) = outcome.entry else {
                continue;
            };

            let target_vars = target_entry.vars();
            let target_var_names: HashSet<&str> =
                target_vars.iter().map(|v| v.name.as_str()).collect();

            // Check explicit step bindings
            for (bound_name, binding) in &step.vars {
                let span = step
                    .vars_spans
                    .get(bound_name)
                    .cloned()
                    .unwrap_or(step.target_span.clone());
                let (line, column) = runbook.source().position(span.start);

                if !target_var_names.contains(bound_name.as_str()) {
                    out.push(Finding {
                        path: runbook.path().to_string(),
                        id: Some(runbook.id().to_string()),
                        line,
                        column,
                        severity: RuleCode::BindingNotInEntry.severity(),
                        code: RuleCode::BindingNotInEntry,
                        message: format!(
                            "step binds variable `{bound_name}`, but entry `{target_id}` does not declare it"
                        ),
                        fix: Some(format!("remove `{bound_name}` from step vars")),
                    });
                    continue;
                }

                let target_var = target_vars.iter().find(|v| v.name == *bound_name).unwrap();

                match binding {
                    StepVarBinding::RunbookVar(source_name) => {
                        used_rb_vars.insert(source_name.clone());
                        if let Some(source_var) =
                            runbook.vars().iter().find(|v| v.name == *source_name)
                        {
                            if let Err(reason) = check_kind_compatibility(
                                source_var.kind,
                                &source_var.options,
                                target_var.kind,
                                &target_var.options,
                            ) {
                                out.push(Finding {
                                    path: runbook.path().to_string(),
                                    id: Some(runbook.id().to_string()),
                                    line,
                                    column,
                                    severity: RuleCode::InvalidBinding.severity(),
                                    code: RuleCode::InvalidBinding,
                                    message: format!(
                                        "invalid binding for `{bound_name}`: {reason}"
                                    ),
                                    fix: Some("use compatible variable kind or options".into()),
                                });
                            }
                        } else {
                            out.push(Finding {
                                path: runbook.path().to_string(),
                                id: Some(runbook.id().to_string()),
                                line,
                                column,
                                severity: RuleCode::BindingUnknownVar.severity(),
                                code: RuleCode::BindingUnknownVar,
                                message: format!(
                                    "step binds to runbook variable `{source_name}`, which is not declared on the runbook"
                                ),
                                fix: Some(format!("declare `{source_name}` under `vars:` on the runbook")),
                            });
                        }
                    }
                    StepVarBinding::Literal(val) => {
                        if let Err(reason) =
                            check_literal_validity(val, target_var.kind, &target_var.options)
                        {
                            out.push(Finding {
                                path: runbook.path().to_string(),
                                id: Some(runbook.id().to_string()),
                                line,
                                column,
                                severity: RuleCode::InvalidBinding.severity(),
                                code: RuleCode::InvalidBinding,
                                message: format!(
                                    "invalid literal value for `{bound_name}`: {reason}"
                                ),
                                fix: Some(
                                    "provide a value matching the variable's type and options"
                                        .into(),
                                ),
                            });
                        }
                    }
                }
            }

            // Check implicit bindings by name
            for target_var in target_vars {
                if step.vars.contains_key(&target_var.name) {
                    continue;
                }

                if let Some(source_var) = runbook.vars().iter().find(|v| v.name == target_var.name)
                {
                    used_rb_vars.insert(source_var.name.clone());
                    if let Err(reason) = check_kind_compatibility(
                        source_var.kind,
                        &source_var.options,
                        target_var.kind,
                        &target_var.options,
                    ) {
                        let (line, column) = runbook.source().position(step.target_span.start);
                        out.push(Finding {
                            path: runbook.path().to_string(),
                            id: Some(runbook.id().to_string()),
                            line,
                            column,
                            severity: RuleCode::InvalidBinding.severity(),
                            code: RuleCode::InvalidBinding,
                            message: format!(
                                "incompatible variable `{}` bound by name to `{target_id}`: {reason}",
                                target_var.name
                            ),
                            fix: Some(format!(
                                "override with explicit binding in step `vars:` or adjust variable `{}`",
                                target_var.name
                            )),
                        });
                    }
                } else if target_var.default.is_none() {
                    let (line, column) = runbook.source().position(step.span.start);
                    out.push(Finding {
                        path: runbook.path().to_string(),
                        id: Some(runbook.id().to_string()),
                        line,
                        column,
                        severity: RuleCode::UnboundStepVar.severity(),
                        code: RuleCode::UnboundStepVar,
                        message: format!(
                            "variable `{}` in `{target_id}` is not bound and has no default; it will be prompted at runtime",
                            target_var.name
                        ),
                        fix: Some(format!(
                            "declare `{}` in runbook `vars:` or add a default in `{target_id}`",
                            target_var.name
                        )),
                    });
                }
            }
        }

        // Check unused runbook variables (KB092)
        if let Some(vars_node) = runbook.tree().get("vars")
            && let crate::yaml::Node::Map { entries, .. } = vars_node
        {
            for (key, _) in entries {
                if !used_rb_vars.contains(&key.name) {
                    let (line, column) = runbook.source().position(key.span.start);
                    out.push(Finding {
                        path: runbook.path().to_string(),
                        id: Some(runbook.id().to_string()),
                        line,
                        column,
                        severity: RuleCode::UnusedRunbookVar.severity(),
                        code: RuleCode::UnusedRunbookVar,
                        message: format!(
                            "variable `{}` is declared on the runbook but never used by any step",
                            key.name
                        ),
                        fix: Some(format!(
                            "remove `{}` from `vars:` or bind it in a step",
                            key.name
                        )),
                    });
                }
            }
        }
    }

    out
}
