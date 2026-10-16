//! Variable resolution for a step (FR-015): step-only answers, then step bindings, then run vars.

use std::collections::BTreeMap;

use super::state::{Run, StepState};
use crate::runbook::binding::{BindingTarget, StepBindingRow};

/// Values for the variables of a step's entry, by the entry's variable names. Variables with no
/// value are absent; the caller asks for them (they are the step-only unbound ones).
///
/// Precedence: a value the user gave for this step in this run > an explicit literal or runbook
/// variable binding > the run variable of the same name.
pub fn step_values(
    run: &Run,
    step: Option<&StepState>,
    row: &StepBindingRow,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for b in &row.bindings {
        let v = match &b.target {
            BindingTarget::RunbookVar { name, .. } => run.vars.get(name).cloned(),
            BindingTarget::Literal { value } => Some(match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            }),
            BindingTarget::Unbound => None,
        };
        if let Some(v) = v {
            out.insert(b.var_name.clone(), v);
        }
    }
    if let Some(s) = step {
        for (k, v) in &s.step_vars {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// A step title with the run's variable values put in (`Deploy {{version}}` -> `Deploy v2`).
/// Titles are display text only; payloads are resolved by `varres`.
pub fn interpolate_title(title: &str, run: &Run) -> String {
    let mut out = title.to_string();
    for (k, v) in &run.vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

/// Names of the entry variables of the step that still have no value and are not secrets handled
/// elsewhere (the TUI asks for these on first action).
pub fn missing_step_vars(run: &Run, step: Option<&StepState>, row: &StepBindingRow) -> Vec<String> {
    let have = step_values(run, step, row);
    row.bindings
        .iter()
        .filter(|b| !have.contains_key(&b.var_name))
        .map(|b| b.var_name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runbook::binding::{BindingValidity, VarBindingInfo};
    use crate::runbook::run::new_run;

    fn row() -> StepBindingRow {
        let b = |n: &str, t: BindingTarget| VarBindingInfo {
            var_name: n.into(),
            expected_kind: "text".into(),
            target: t,
            validity: BindingValidity::Valid,
        };
        StepBindingRow {
            step_id: "s".into(),
            step_index: 0,
            entry_id: Some("cmd/x".into()),
            bindings: vec![
                b(
                    "env",
                    BindingTarget::RunbookVar {
                        name: "env".into(),
                        kind: "text".into(),
                    },
                ),
                b(
                    "n",
                    BindingTarget::Literal {
                        value: serde_json::json!(5),
                    },
                ),
                b("extra", BindingTarget::Unbound),
            ],
        }
    }

    #[test]
    fn precedence_and_missing() {
        let mut run = new_run("rb/x", "rb", "T", "h", 1, &["s"]);
        run.vars.insert("env".into(), "prod".into());
        let r = row();
        let v = step_values(&run, run.step_states.get("s"), &r);
        assert_eq!(v["env"], "prod");
        assert_eq!(v["n"], "5");
        assert_eq!(
            missing_step_vars(&run, run.step_states.get("s"), &r),
            ["extra"]
        );
        run.step_states
            .get_mut("s")
            .unwrap()
            .step_vars
            .insert("extra".into(), "1".into());
        run.step_states
            .get_mut("s")
            .unwrap()
            .step_vars
            .insert("env".into(), "x".into());
        let v = step_values(&run, run.step_states.get("s"), &r);
        assert_eq!(v["env"], "x");
        assert!(missing_step_vars(&run, run.step_states.get("s"), &r).is_empty());
    }
}
