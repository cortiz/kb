//! Runbook run mode (spec 015): the state machine, ordering rules and drift reconciliation.
//! Persistence is in [`fs`]; the TUI and CLI only drive these functions.

pub mod fs;
pub mod resolve;
pub mod state;

use std::collections::BTreeMap;

pub use state::*;

use crate::runbook::RunbookStep;

fn push_status(s: &mut StepState, status: StepStatus, now: i64) {
    s.status = status;
    s.status_history.push(StatusTransition {
        status,
        timestamp: now,
    });
}

/// Seconds since the Unix epoch, now.
pub fn now_secs() -> i64 {
    crate::usage::unix_secs(std::time::SystemTime::now())
}

/// Run id `<runbook-id-slug>-<timestamp>`.
pub fn run_id(runbook_id: &str, now: i64) -> String {
    let slug: String = runbook_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{}-{now}", slug.trim_matches('-'))
}

/// A new active run with every step pending.
pub fn new_run(
    runbook_id: &str,
    title_slug_source: &str,
    display_name: &str,
    hash: &str,
    now: i64,
    step_ids: &[&str],
) -> Run {
    let _ = title_slug_source;
    Run {
        schema: Some("https://kb.dev/schemas/run-v1.json".into()),
        version: RUN_VERSION,
        id: run_id(runbook_id, now),
        runbook_id: runbook_id.to_string(),
        display_name: display_name.to_string(),
        state: RunState::Active,
        created_at: now,
        updated_at: now,
        finished_at: None,
        runbook_hash_start: hash.to_string(),
        runbook_hash_resume: hash.to_string(),
        vars: BTreeMap::new(),
        var_history: Vec::new(),
        step_states: step_ids
            .iter()
            .map(|id| (id.to_string(), StepState::new(id, now)))
            .collect(),
        notes: Vec::new(),
    }
}

/// Display name: title, up to three non-secret values, and the start time (FR-001).
pub fn display_name(title: &str, values: &[&str], started: &str) -> String {
    let vals: Vec<&str> = values
        .iter()
        .copied()
        .filter(|v| !v.is_empty() && v.chars().count() <= 24)
        .take(3)
        .collect();
    if vals.is_empty() {
        format!("{title} · {started}")
    } else {
        format!("{title} ({}) · {started}", vals.join(", "))
    }
}

/// `Oct 15 17:30` (UTC) for run names.
pub fn started_label(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {d} {:02}:{:02}",
        MONTHS[(m - 1) as usize],
        rem / 3600,
        rem % 3600 / 60
    )
}

/// Sets a status, stamping the time. Returns false for an unknown step.
pub fn mark_step(run: &mut Run, step_id: &str, status: StepStatus, now: i64) -> bool {
    let Some(s) = run.step_states.get_mut(step_id) else {
        return false;
    };
    push_status(s, status, now);
    if matches!(
        status,
        StepStatus::Opened | StepStatus::Copied | StepStatus::Running
    ) {
        s.last_action_at = Some(now);
    }
    run.updated_at = now;
    true
}

pub fn reset_step(run: &mut Run, step_id: &str, now: i64) -> bool {
    mark_step(run, step_id, StepStatus::Pending, now)
}

/// Records how a command ended: exit 0 -> `done` is the caller's choice (auto_done/confirm), so
/// this only records the exit code and duration and returns the status that applies
/// (`Done`-eligible `Running`, `Failed` or `Interrupted`).
pub fn record_exit(
    run: &mut Run,
    step_id: &str,
    code: Option<i32>,
    signaled: bool,
    duration_ms: u64,
    now: i64,
) -> Option<StepStatus> {
    let s = run.step_states.get_mut(step_id)?;
    s.last_exit_code = code;
    s.last_duration_ms = Some(duration_ms);
    run.updated_at = now;
    let status = match code {
        Some(0) => return Some(StepStatus::Done),
        _ if signaled || code.is_some_and(|c| c == 130) => StepStatus::Interrupted,
        _ => StepStatus::Failed,
    };
    push_status(s, status, now);
    Some(status)
}

/// Records the entry hash a step acted on (FR-025).
pub fn record_entry_hash(run: &mut Run, step_id: &str, hash: &str) {
    if let Some(s) = run.step_states.get_mut(step_id) {
        s.entry_hash_at_action = Some(hash.to_string());
    }
}

pub fn add_step_note(run: &mut Run, step_id: &str, text: &str, now: i64) -> bool {
    let Some(s) = run.step_states.get_mut(step_id) else {
        return false;
    };
    s.notes.push(StepNote {
        timestamp: now,
        text: text.to_string(),
    });
    run.updated_at = now;
    true
}

/// Sets a run variable, recording history. Secrets must never be passed here; a value that looks
/// like a credential is refused (returns false) so it can't reach disk.
pub fn set_var(run: &mut Run, name: &str, value: &str, now: i64) -> bool {
    if !crate::lint::secrets::scan_secrets(value).hits.is_empty() {
        return false;
    }
    let old = run.vars.insert(name.to_string(), value.to_string());
    if old.as_deref() != Some(value) {
        run.var_history.push(VarChange {
            timestamp: now,
            name: name.to_string(),
            old_value: old,
            new_value: value.to_string(),
        });
    }
    run.updated_at = now;
    true
}

/// Sets a step-only variable answer (kept in this run only, FR US6.3). Same credential guard.
pub fn set_step_var(run: &mut Run, step_id: &str, name: &str, value: &str) -> bool {
    if !crate::lint::secrets::scan_secrets(value).hits.is_empty() {
        return false;
    }
    match run.step_states.get_mut(step_id) {
        Some(s) => {
            s.step_vars.insert(name.to_string(), value.to_string());
            true
        }
        None => false,
    }
}

/// Every non-optional step is done or skipped (FR-008). Removed steps don't count.
pub fn is_complete(run: &Run, steps: &[RunbookStep]) -> bool {
    steps.iter().all(|st| {
        st.optional
            || run
                .step_states
                .get(&st.id)
                .is_none_or(|s| s.status.is_settled())
    })
}

pub fn finish_run(run: &mut Run, now: i64) {
    run.state = RunState::Finished;
    run.finished_at = Some(now);
    run.updated_at = now;
}

pub fn abandon_run(run: &mut Run, now: i64) {
    run.state = RunState::Abandoned;
    run.finished_at = Some(now);
    run.updated_at = now;
}

/// `(done, skipped, failed)` counts over the steps currently in the runbook.
pub fn counts(run: &Run, steps: &[RunbookStep]) -> (usize, usize, usize) {
    let mut c = (0, 0, 0);
    for st in steps {
        match run.step_states.get(&st.id).map(|s| s.status) {
            Some(StepStatus::Done) => c.0 += 1,
            Some(StepStatus::Skipped) => c.1 += 1,
            Some(StepStatus::Failed) => c.2 += 1,
            _ => {}
        }
    }
    c
}

/// The earliest non-optional step before `index` that is pending, failed or interrupted (FR-010).
pub fn earlier_unfinished<'a>(
    run: &Run,
    steps: &'a [RunbookStep],
    index: usize,
) -> Option<&'a RunbookStep> {
    steps.iter().take(index).find(|st| {
        !st.optional
            && matches!(
                run.step_states.get(&st.id).map(|s| s.status),
                Some(StepStatus::Pending | StepStatus::Failed | StepStatus::Interrupted) | None
            )
    })
}

/// The next step after `index` that is not done or skipped (and not optional-settled), wrapping
/// nowhere: stays on `index` if there is none.
pub fn next_open(run: &Run, steps: &[RunbookStep], index: usize) -> usize {
    (index + 1..steps.len())
        .find(|&i| {
            run.step_states
                .get(&steps[i].id)
                .is_none_or(|s| !s.status.is_settled())
        })
        .unwrap_or(index)
}

/// What reconciliation found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drift {
    pub changed: bool,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// Steps whose id was derived from the title (suggest explicit ids).
    pub implicit_ids: bool,
}

/// Reconciles a run with the runbook on disk by step id (FR-024): new steps become pending,
/// vanished ones are flagged `removed` (history kept), returning ones are restored to pending.
pub fn reconcile(run: &mut Run, steps: &[RunbookStep], hash: &str, now: i64) -> Drift {
    let mut d = Drift::default();
    if run.runbook_hash_resume != hash {
        d.changed = true;
    }
    for st in steps {
        match run.step_states.get_mut(&st.id) {
            None => {
                run.step_states
                    .insert(st.id.clone(), StepState::new(&st.id, now));
                d.added.push(st.id.clone());
            }
            Some(s) if s.status == StepStatus::Removed => {
                push_status(s, StepStatus::Pending, now);
            }
            Some(_) => {}
        }
    }
    let live: std::collections::HashSet<&str> = steps.iter().map(|s| s.id.as_str()).collect();
    for (id, s) in run.step_states.iter_mut() {
        if !live.contains(id.as_str()) && s.status != StepStatus::Removed {
            push_status(s, StepStatus::Removed, now);
            d.removed.push(id.clone());
        }
    }
    d.implicit_ids = d.changed && steps.iter().any(|s| !s.id_explicit);
    if d.changed {
        run.runbook_hash_resume = hash.to_string();
        run.updated_at = now;
    }
    d
}

/// A reopened run with no live process: `running` steps become `interrupted` (FR-021).
pub fn recover_interrupted(run: &mut Run, now: i64) -> bool {
    let mut any = false;
    for s in run.step_states.values_mut() {
        if s.status == StepStatus::Running {
            push_status(s, StepStatus::Interrupted, now);
            any = true;
        }
    }
    if any {
        run.updated_at = now;
    }
    any
}

/// Whether the entry changed since the step was last acted on.
pub fn entry_changed(run: &Run, step_id: &str, current_hash: &str) -> bool {
    run.step_states
        .get(step_id)
        .and_then(|s| s.entry_hash_at_action.as_deref())
        .is_some_and(|h| h != current_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runbook::{RunbookStep, StepTarget};

    fn step(id: &str, optional: bool) -> RunbookStep {
        RunbookStep {
            id: id.into(),
            id_explicit: true,
            title: id.into(),
            target: StepTarget::Do("x".into()),
            note: None,
            optional,
            confirm: false,
            vars: BTreeMap::new(),
            span: 0..0,
            title_span: 0..0,
            target_span: 0..0,
            id_span: None,
            vars_spans: BTreeMap::new(),
        }
    }

    #[test]
    fn lifecycle_order_and_drift() {
        let steps = vec![step("a", false), step("b", true), step("c", false)];
        let mut run = new_run("runbook/x", "x", "X", "h1", 100, &["a", "b", "c"]);
        assert_eq!(run.id, "runbook-x-100");
        assert!(!is_complete(&run, &steps));
        assert_eq!(earlier_unfinished(&run, &steps, 2).unwrap().id, "a");
        mark_step(&mut run, "a", StepStatus::Done, 101);
        assert!(earlier_unfinished(&run, &steps, 2).is_none()); // b optional
        assert_eq!(next_open(&run, &steps, 0), 1);
        mark_step(&mut run, "c", StepStatus::Skipped, 102);
        assert!(is_complete(&run, &steps));
        assert_eq!(counts(&run, &steps), (1, 1, 0));

        // exit handling
        mark_step(&mut run, "b", StepStatus::Running, 103);
        assert_eq!(
            record_exit(&mut run, "b", Some(2), false, 5, 104),
            Some(StepStatus::Failed)
        );
        assert_eq!(
            record_exit(&mut run, "b", None, true, 5, 105),
            Some(StepStatus::Interrupted)
        );
        assert_eq!(
            record_exit(&mut run, "b", Some(0), false, 5, 106),
            Some(StepStatus::Done)
        );
        assert_eq!(run.step_states["b"].last_exit_code, Some(0));

        // crash recovery
        mark_step(&mut run, "b", StepStatus::Running, 107);
        assert!(recover_interrupted(&mut run, 108));
        assert_eq!(run.step_states["b"].status, StepStatus::Interrupted);

        // drift: reorder + add d + remove c
        let steps2 = vec![step("b", true), step("a", false), step("d", false)];
        let d = reconcile(&mut run, &steps2, "h2", 200);
        assert!(d.changed);
        assert_eq!(d.added, ["d"]);
        assert_eq!(d.removed, ["c"]);
        assert_eq!(run.step_states["a"].status, StepStatus::Done);
        assert_eq!(run.step_states["c"].status, StepStatus::Removed);
        assert_eq!(run.step_states["d"].status, StepStatus::Pending);
        assert!(!reconcile(&mut run, &steps2, "h2", 201).changed);
    }

    #[test]
    fn label() {
        assert_eq!(started_label(1_760_549_400), "Oct 15 17:30");
    }

    #[test]
    fn vars_history_and_secret_guard() {
        let mut run = new_run("rb/x", "x", "X", "h", 1, &[]);
        assert!(set_var(&mut run, "env", "stage", 2));
        assert!(set_var(&mut run, "env", "prod", 3));
        assert_eq!(run.var_history.len(), 2);
        assert_eq!(run.var_history[1].old_value.as_deref(), Some("stage"));
        assert!(!set_var(&mut run, "k", "AKIAIOSFODNN7EXAMPLE", 4));
        assert!(!run.vars.contains_key("k"));
    }

    #[test]
    fn persistence_lock_prune() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut run = new_run("rb/x", "x", "X", "h", 1, &["a"]);
        fs::save(root, &run).unwrap();
        assert_eq!(fs::load(root, &run.id).unwrap(), run);
        assert!(root.join(".kb/runs/.gitignore").exists());
        {
            let _g = fs::acquire(root, &run.id, 1, &|_| true).unwrap();
            assert!(
                root.join(".kb/runs")
                    .join(format!("{}.lock", run.id))
                    .exists()
            );
        }
        assert!(
            !root
                .join(".kb/runs")
                .join(format!("{}.lock", run.id))
                .exists()
        );
        // stale lock (dead pid) is reclaimed
        std::fs::write(
            root.join(".kb/runs").join(format!("{}.lock", run.id)),
            r#"{"pid":999999,"started_at":1,"hostname":"h"}"#,
        )
        .unwrap();
        assert!(fs::locked_by(root, &run.id, &|pid| pid != 999_999).is_none());
        assert!(fs::acquire(root, &run.id, 2, &|pid| pid != 999_999).is_ok());
        // corrupt file keeps .bak
        std::fs::write(root.join(".kb/runs/bad.json"), "{nope").unwrap();
        assert!(matches!(
            fs::load(root, "bad"),
            Err(fs::LoadError::Unreadable(_))
        ));
        assert!(root.join(".kb/runs/bad.json.bak").exists());
        std::fs::remove_file(root.join(".kb/runs/bad.json")).unwrap();
        // prune
        abandon_run(&mut run, 10);
        fs::save(root, &run).unwrap();
        assert!(fs::prune(root, Some(1), 20).unwrap().is_empty());
        assert_eq!(fs::prune(root, None, 20).unwrap(), [run.id.clone()]);
    }
}
