//! Variable binding table evaluator and kind compatibility checking for runbooks.

use serde::{Deserialize, Serialize};

use crate::runbook::{RunbookStep, StepTarget, StepVarBinding};
use crate::vars::{VarKind, Variable};

/// The target of a step variable binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingTarget {
    RunbookVar { name: String, kind: String },
    Literal { value: serde_json::Value },
    Unbound,
}

/// The validity of a variable binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingValidity {
    Valid,
    Invalid { reason: String },
}

impl BindingValidity {
    pub fn is_valid(&self) -> bool {
        matches!(self, BindingValidity::Valid)
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            BindingValidity::Valid => None,
            BindingValidity::Invalid { reason } => Some(reason),
        }
    }
}

/// Binding information for one variable of a step entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarBindingInfo {
    /// Variable name declared on the step entry.
    pub var_name: String,
    /// Expected kind of the step entry variable.
    pub expected_kind: String,
    /// Binding target.
    pub target: BindingTarget,
    /// Validity of the binding.
    pub validity: BindingValidity,
}

/// Bindings for a single step in a runbook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepBindingRow {
    pub step_id: String,
    pub step_index: usize,
    pub entry_id: Option<String>,
    pub bindings: Vec<VarBindingInfo>,
}

/// Complete binding table for a runbook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingTable {
    pub runbook_id: String,
    pub steps: Vec<StepBindingRow>,
}

/// Verifies kind compatibility when binding a runbook variable to a step variable (FR-009).
pub fn check_kind_compatibility(
    source_kind: VarKind,
    source_options: &[String],
    target_kind: VarKind,
    target_options: &[String],
) -> Result<(), String> {
    // Secret rules: secret -> secret only
    if source_kind == VarKind::Secret || target_kind == VarKind::Secret {
        if source_kind == target_kind {
            return Ok(());
        }
        return Err("secret variables can only be bound to secret variables".to_string());
    }

    match (source_kind, target_kind) {
        // enum -> enum: runbook options must be a subset of step options
        (VarKind::Enum, VarKind::Enum) => {
            let missing: Vec<&str> = source_options
                .iter()
                .filter(|opt| !target_options.contains(opt))
                .map(|s| s.as_str())
                .collect();
            if missing.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "runbook enum options are not a subset of step options; missing: {}",
                    missing.join(", ")
                ))
            }
        }
        // enum -> text, number -> number, number -> text
        (VarKind::Enum, VarKind::Text) => Ok(()),
        (VarKind::Number, VarKind::Number) => Ok(()),
        (VarKind::Number, VarKind::Text) => Ok(()),
        // text -> number or enum: invalid
        (VarKind::Text, VarKind::Number) => {
            Err("cannot bind text variable to number variable".to_string())
        }
        (VarKind::Text, VarKind::Enum) => {
            Err("cannot bind text variable to enum variable".to_string())
        }
        // text -> text
        (VarKind::Text, VarKind::Text) => Ok(()),
        // enum -> number
        (VarKind::Enum, VarKind::Number) => {
            Err("cannot bind enum variable to number variable".to_string())
        }
        _ => Err(format!(
            "cannot bind {} variable to {} variable",
            source_kind.name(),
            target_kind.name()
        )),
    }
}

/// Validates a literal constant value against the target variable kind (FR-009).
pub fn check_literal_validity(
    value: &serde_json::Value,
    target_kind: VarKind,
    target_options: &[String],
) -> Result<(), String> {
    match target_kind {
        VarKind::Text => Ok(()),
        VarKind::Number => match value {
            serde_json::Value::Number(_) => Ok(()),
            serde_json::Value::String(s) if s.trim().parse::<f64>().is_ok() => Ok(()),
            _ => Err(format!("expected a number, got `{value}`")),
        },
        VarKind::Enum => match value {
            serde_json::Value::String(s) if target_options.iter().any(|o| o == s) => Ok(()),
            _ => Err(format!(
                "value `{value}` is not one of allowed options: [{}]",
                target_options.join(", ")
            )),
        },
        VarKind::Secret => Err("cannot bind literal value to a secret variable".to_string()),
    }
}

/// Evaluates the complete binding table for a runbook across all its steps.
pub fn evaluate_runbook_bindings<'a, F>(
    runbook_id: &str,
    runbook_vars: &[Variable],
    steps: &[RunbookStep],
    get_entry_vars: F,
) -> BindingTable
where
    F: Fn(&str) -> Option<&'a [Variable]>,
{
    let mut step_rows = Vec::new();

    for (step_idx, step) in steps.iter().enumerate() {
        let entry_id = match &step.target {
            StepTarget::Entry(id) => Some(id.clone()),
            StepTarget::Do(_) => None,
        };

        let mut bindings = Vec::new();

        if let Some(ref target_id) = entry_id {
            if let Some(target_vars) = get_entry_vars(target_id) {
                for target_var in target_vars {
                    let var_name = target_var.name.clone();
                    let expected_kind = target_var.kind.name().to_string();

                    // Check if explicitly bound
                    if let Some(binding) = step.vars.get(&var_name) {
                        match binding {
                            StepVarBinding::RunbookVar(source_name) => {
                                // Find in runbook_vars
                                if let Some(source_var) =
                                    runbook_vars.iter().find(|v| v.name == *source_name)
                                {
                                    let comp = check_kind_compatibility(
                                        source_var.kind,
                                        &source_var.options,
                                        target_var.kind,
                                        &target_var.options,
                                    );
                                    let validity = match comp {
                                        Ok(()) => BindingValidity::Valid,
                                        Err(reason) => BindingValidity::Invalid { reason },
                                    };
                                    bindings.push(VarBindingInfo {
                                        var_name,
                                        expected_kind,
                                        target: BindingTarget::RunbookVar {
                                            name: source_name.clone(),
                                            kind: source_var.kind.name().to_string(),
                                        },
                                        validity,
                                    });
                                } else {
                                    bindings.push(VarBindingInfo {
                                        var_name,
                                        expected_kind,
                                        target: BindingTarget::RunbookVar {
                                            name: source_name.clone(),
                                            kind: "unknown".to_string(),
                                        },
                                        validity: BindingValidity::Invalid {
                                            reason: format!(
                                                "referenced runbook variable `{source_name}` is not declared"
                                            ),
                                        },
                                    });
                                }
                            }
                            StepVarBinding::Literal(lit_val) => {
                                let comp = check_literal_validity(
                                    lit_val,
                                    target_var.kind,
                                    &target_var.options,
                                );
                                let validity = match comp {
                                    Ok(()) => BindingValidity::Valid,
                                    Err(reason) => BindingValidity::Invalid { reason },
                                };
                                bindings.push(VarBindingInfo {
                                    var_name,
                                    expected_kind,
                                    target: BindingTarget::Literal {
                                        value: lit_val.clone(),
                                    },
                                    validity,
                                });
                            }
                        }
                    } else {
                        // Default binding by name (FR-008)
                        if let Some(source_var) = runbook_vars.iter().find(|v| v.name == var_name) {
                            let comp = check_kind_compatibility(
                                source_var.kind,
                                &source_var.options,
                                target_var.kind,
                                &target_var.options,
                            );
                            let validity = match comp {
                                Ok(()) => BindingValidity::Valid,
                                Err(reason) => BindingValidity::Invalid { reason },
                            };
                            bindings.push(VarBindingInfo {
                                var_name: var_name.clone(),
                                expected_kind,
                                target: BindingTarget::RunbookVar {
                                    name: var_name,
                                    kind: source_var.kind.name().to_string(),
                                },
                                validity,
                            });
                        } else {
                            // Unbound step variable (FR-010)
                            bindings.push(VarBindingInfo {
                                var_name,
                                expected_kind,
                                target: BindingTarget::Unbound,
                                validity: BindingValidity::Valid,
                            });
                        }
                    }
                }
            }
        }

        step_rows.push(StepBindingRow {
            step_id: step.id.clone(),
            step_index: step_idx,
            entry_id,
            bindings,
        });
    }

    BindingTable {
        runbook_id: runbook_id.to_string(),
        steps: step_rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enum_subset_compatibility() {
        let runbook_opts = vec!["stage".into(), "prod".into()];
        let step_opts_superset = vec!["dev".into(), "stage".into(), "prod".into()];
        let step_opts_subset = vec!["prod".into()];

        assert!(check_kind_compatibility(
            VarKind::Enum,
            &runbook_opts,
            VarKind::Enum,
            &step_opts_superset
        ).is_ok());

        assert!(check_kind_compatibility(
            VarKind::Enum,
            &runbook_opts,
            VarKind::Enum,
            &step_opts_subset
        ).is_err());
    }

    #[test]
    fn test_kind_cross_compat() {
        assert!(check_kind_compatibility(VarKind::Number, &[], VarKind::Text, &[]).is_ok());
        assert!(check_kind_compatibility(VarKind::Enum, &["a".into()], VarKind::Text, &[]).is_ok());
        assert!(check_kind_compatibility(VarKind::Text, &[], VarKind::Number, &[]).is_err());
        assert!(check_kind_compatibility(VarKind::Text, &[], VarKind::Secret, &[]).is_err());
        assert!(check_kind_compatibility(VarKind::Secret, &[], VarKind::Secret, &[]).is_ok());
    }

    #[test]
    fn test_evaluate_binding_table() {
        use std::collections::BTreeMap;
        use crate::vars::{Escape, Variable};

        let test_var = |name: &str, kind: VarKind, opts: &[&str]| Variable {
            name: name.into(),
            kind,
            declared: true,
            default: None,
            options: opts.iter().map(|s| s.to_string()).collect(),
            description: None,
            env: None,
            escape: Escape::Auto,
            references: vec![],
        };

        let rb_vars = vec![
            test_var("env", VarKind::Enum, &["stage", "prod"]),
            test_var("tag", VarKind::Text, &[]),
        ];

        let mut step1_vars = BTreeMap::new();
        step1_vars.insert("version".into(), StepVarBinding::RunbookVar("tag".into()));
        step1_vars.insert("count".into(), StepVarBinding::Literal(serde_json::json!(5)));

        let steps = vec![
            RunbookStep {
                id: "deploy".into(),
                id_explicit: true,
                title: "Deploy step".into(),
                target: StepTarget::Entry("cmd/deploy".into()),
                note: None,
                optional: false,
                confirm: true,
                vars: step1_vars,
                span: 0..0,
                title_span: 0..0,
                target_span: 0..0,
                id_span: None,
                vars_spans: BTreeMap::new(),
            },
        ];

        let entry_vars = vec![
            test_var("env", VarKind::Enum, &["dev", "stage", "prod"]),
            test_var("version", VarKind::Text, &[]),
            test_var("count", VarKind::Number, &[]),
            test_var("unbound_var", VarKind::Text, &[]),
        ];

        let table = evaluate_runbook_bindings("runbook/test", &rb_vars, &steps, |id| {
            if id == "cmd/deploy" {
                Some(&entry_vars)
            } else {
                None
            }
        });

        assert_eq!(table.runbook_id, "runbook/test");
        assert_eq!(table.steps.len(), 1);
        let row = &table.steps[0];
        assert_eq!(row.step_id, "deploy");
        assert_eq!(row.bindings.len(), 4);

        // 1. env -> bound by name
        let env_b = row.bindings.iter().find(|b| b.var_name == "env").unwrap();
        assert_eq!(
            env_b.target,
            BindingTarget::RunbookVar {
                name: "env".into(),
                kind: "enum".into(),
            }
        );
        assert_eq!(env_b.validity, BindingValidity::Valid);

        // 2. version -> explicitly bound to runbook var "tag"
        let ver_b = row.bindings.iter().find(|b| b.var_name == "version").unwrap();
        assert_eq!(
            ver_b.target,
            BindingTarget::RunbookVar {
                name: "tag".into(),
                kind: "text".into(),
            }
        );
        assert_eq!(ver_b.validity, BindingValidity::Valid);

        // 3. count -> explicitly bound to literal 5
        let cnt_b = row.bindings.iter().find(|b| b.var_name == "count").unwrap();
        assert_eq!(
            cnt_b.target,
            BindingTarget::Literal {
                value: serde_json::json!(5),
            }
        );
        assert_eq!(cnt_b.validity, BindingValidity::Valid);

        // 4. unbound_var -> unbound
        let unb_b = row.bindings.iter().find(|b| b.var_name == "unbound_var").unwrap();
        assert_eq!(unb_b.target, BindingTarget::Unbound);
        assert_eq!(unb_b.validity, BindingValidity::Valid);
    }
}
