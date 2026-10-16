//! Run mode's I/O side (spec 015): reading runbooks and entries, building the run screen, the
//! variable forms, and starting, resuming and saving runs. The state machine itself is
//! `kb_core::runbook::run`; the screen is `run_view`.

use std::collections::BTreeMap;
use std::time::SystemTime;

use kb_core::config::Settings;
use kb_core::entry::{Entry, EntryType, TypeFields};
use kb_core::mcp::review::ReviewStateTracker;
use kb_core::runbook::StepTarget;
use kb_core::runbook::binding::{self, BindingTarget, StepBindingRow};
use kb_core::runbook::run::{self, Run, RunState, fs as runfs, resolve as runresolve};
use kb_core::search::Kb;
use kb_core::usage::{content_hash, unix_secs};
use kb_core::varres::{self, Inputs, Source, Value, Values};
use kb_core::vars::VarKind;

use super::form::{FormSpec, Submission, Then};
use super::run_picker::{RunPicker, RunRow};
use super::run_view::{RunScreen, StepInfo};

/// What lives only in this process: the lock, and secret values (never written to disk).
#[derive(Default)]
pub struct RunCtx {
    pub guard: Option<runfs::LockGuard>,
    /// Runbook-level secrets by variable name.
    pub run_secrets: BTreeMap<String, Value>,
    /// Step-only secrets by (step id, variable name).
    pub step_secrets: BTreeMap<(String, String), Value>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub fn now() -> i64 {
    unix_secs(SystemTime::now())
}

fn hash_of(kb: &Kb, id: &str) -> Option<String> {
    let bytes = std::fs::read(kb.entry_path(id).ok()?).ok()?;
    Some(format!("{:016x}", content_hash(&bytes)))
}

fn runbook(kb: &Kb, id: &str) -> Result<Entry, String> {
    let e = kb.entry(id).map_err(|e| e.to_string())?;
    if e.entry_type() != Some(EntryType::Runbook) {
        return Err(format!("{id} is not a runbook"));
    }
    Ok(e)
}

/// The values of one step's entry: run variables, bindings, step answers, then secrets from
/// this process or their environment variable. Also the names still missing.
fn values_for(
    entry: &Entry,
    run: &Run,
    row: &StepBindingRow,
    step_id: &str,
    ctx: &RunCtx,
) -> (Values, Vec<String>) {
    let texts = runresolve::step_values(run, run.step_states.get(step_id), row);
    let mut values = Values::new();
    let mut missing = Vec::new();
    for p in &varres::plan_vars(entry).vars {
        let v = &p.var;
        if v.kind == VarKind::Secret {
            let bound = row
                .bindings
                .iter()
                .find(|b| b.var_name == v.name)
                .and_then(|b| match &b.target {
                    BindingTarget::RunbookVar { name, .. } => Some(name.clone()),
                    _ => None,
                });
            let found = bound
                .and_then(|n| ctx.run_secrets.get(&n).cloned())
                .or_else(|| {
                    ctx.step_secrets
                        .get(&(step_id.to_string(), v.name.clone()))
                        .cloned()
                })
                .or_else(|| v.env.as_deref().and_then(env).map(Value::secret));
            match found {
                Some(val) => {
                    values.insert(v.name.clone(), val);
                }
                None => missing.push(v.name.clone()),
            }
        } else if let Some(t) = texts.get(&v.name) {
            values.insert(v.name.clone(), Value::plain(t.clone()));
        } else {
            missing.push(v.name.clone());
        }
    }
    (values, missing)
}

fn row_for(
    kb: &Kb,
    rb: &Entry,
    step: &kb_core::runbook::RunbookStep,
    entry: &Entry,
) -> StepBindingRow {
    let _ = kb;
    let table =
        binding::evaluate_runbook_bindings(rb.id(), rb.vars(), std::slice::from_ref(step), |_| {
            Some(entry.vars())
        });
    table.steps.into_iter().next().unwrap_or(StepBindingRow {
        step_id: step.id.clone(),
        step_index: 0,
        entry_id: None,
        bindings: Vec::new(),
    })
}

fn build_info(
    kb: &Kb,
    rb: &Entry,
    run: &Run,
    step: &kb_core::runbook::RunbookStep,
    ctx: &RunCtx,
) -> StepInfo {
    let title = runresolve::interpolate_title(&step.title, run);
    let mut info = StepInfo {
        step: step.clone(),
        title,
        kind: "do".into(),
        entry_id: None,
        missing: false,
        unreviewed: false,
        is_cmd: false,
        entry_hash: None,
        preview: Vec::new(),
        bindings: Vec::new(),
    };
    let id = match &step.target {
        StepTarget::Do(text) => {
            info.preview = text.lines().map(str::to_string).collect();
            return info;
        }
        StepTarget::Entry(id) => id,
    };
    info.entry_id = Some(id.clone());
    let Ok(entry) = kb.entry(id) else {
        info.kind = "?".into();
        info.missing = true;
        return info;
    };
    let ty = entry.entry_type();
    info.kind = ty.map_or("?", EntryType::name).to_string();
    info.is_cmd = ty == Some(EntryType::Cmd);
    info.unreviewed = ReviewStateTracker::is_unreviewed(kb.root(), id);
    info.entry_hash = hash_of(kb, id);
    let row = row_for(kb, rb, step, &entry);
    let (values, missing) = values_for(&entry, run, &row, &step.id, ctx);
    let plan = varres::plan_vars(&entry);
    let resolved = varres::preview(&plan, &values);
    info.preview = match (ty, entry.fields()) {
        (Some(EntryType::Code), TypeFields::Code { code: Some(c) }) => {
            vec![format!("{}:{} @ {}", c.path, c.line, c.git_ref)]
        }
        _ => resolved
            .primary()
            .map(|o| o.redacted().lines().map(str::to_string).collect())
            .unwrap_or_default(),
    };
    for p in &plan.vars {
        let n = &p.var.name;
        let shown = if missing.contains(n) {
            "(asked on first use)".to_string()
        } else if p.var.kind == VarKind::Secret {
            varres::MASK.to_string()
        } else {
            values
                .get(n)
                .map(|v| v.reveal().to_string())
                .unwrap_or_default()
        };
        info.bindings.push(format!("{n} = {shown}"));
    }
    info
}

fn build_screen(kb: &Kb, rb: &Entry, run: Run, settings: &Settings, ctx: &RunCtx) -> RunScreen {
    let steps: Vec<_> = rb
        .runbook_steps()
        .unwrap_or_default()
        .iter()
        .map(|s| build_info(kb, rb, &run, s, ctx))
        .collect();
    let title = rb.title().unwrap_or(rb.id()).to_string();
    let mut screen = RunScreen::new(
        run,
        title,
        steps,
        settings.runbook_auto_done(),
        settings.runbook_order_warnings(),
    );
    screen.runbook_unreviewed = ReviewStateTracker::is_unreviewed(kb.root(), rb.id());
    screen
}

/// Recomputes the resolved texts (after variables changed).
pub fn refresh(kb: &Kb, screen: &mut RunScreen, ctx: &RunCtx) {
    let Ok(rb) = runbook(kb, &screen.run.runbook_id) else {
        return;
    };
    for i in 0..screen.steps.len() {
        let step = screen.steps[i].step.clone();
        screen.steps[i] = build_info(kb, &rb, &screen.run, &step, ctx);
    }
}

/// The form for the runbook's variables. With `only_missing`, just those that have no value
/// (new variables, secrets with no environment source).
pub fn runbook_form(
    kb: &Kb,
    runbook_id: &str,
    run: Option<&Run>,
    only_missing: bool,
    ctx: &RunCtx,
) -> Result<Option<FormSpec>, String> {
    let rb = runbook(kb, runbook_id)?;
    let mut plan = varres::plan_vars(&rb);
    if only_missing && let Some(run) = run {
        let all = plan.clone();
        plan = all.restricted(|name| {
            let Some(v) = all.var(name).map(|p| &p.var) else {
                return false;
            };
            if v.kind == VarKind::Secret {
                !ctx.run_secrets.contains_key(&v.name) && v.env.as_deref().and_then(env).is_none()
            } else {
                !run.vars.contains_key(&v.name)
            }
        });
    }
    if plan.is_empty() {
        return Ok(None);
    }
    let (memory, recent) = kb.remembered(runbook_id);
    let memory = run.map_or(memory, |r| r.vars.clone());
    let bindings = varres::prefill(
        &plan,
        &Inputs {
            flags: &[],
            env: &env,
            memory: Some(&memory),
        },
    );
    Ok(Some(FormSpec {
        id: runbook_id.to_string(),
        title: rb.title().unwrap_or(runbook_id).to_string(),
        plan,
        bindings,
        recent,
    }))
}

/// Applies a submitted runbook-variable form: plain values to the run (with history), secrets to
/// this process only. Returns the names of values refused because they look like credentials.
pub fn apply_run_vars(run: &mut Run, ctx: &mut RunCtx, sub: &Submission) -> Vec<String> {
    let mut refused = Vec::new();
    let t = now();
    for (name, value, _) in &sub.values {
        if value.is_secret() {
            ctx.run_secrets.insert(name.clone(), value.clone());
        } else if !run::set_var(run, name, value.reveal(), t) {
            refused.push(name.clone());
        }
    }
    refused
}

/// Creates, locks and saves a new run from the start form (or from no form, when the runbook has
/// no variables), and builds its screen.
pub fn start_run(
    kb: &Kb,
    settings: &Settings,
    runbook_id: &str,
    sub: Option<&Submission>,
    ctx: &mut RunCtx,
    alive: &dyn Fn(u32) -> bool,
) -> Result<RunScreen, String> {
    let rb = runbook(kb, runbook_id)?;
    let hash = hash_of(kb, runbook_id).unwrap_or_default();
    let t = now();
    let steps = rb.runbook_steps().unwrap_or_default();
    let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();
    let title = rb.title().unwrap_or(runbook_id);
    let mut r = run::new_run(runbook_id, runbook_id, title, &hash, t, &ids);
    ctx.run_secrets.clear();
    ctx.step_secrets.clear();
    let mut shown: Vec<String> = Vec::new();
    if let Some(sub) = sub {
        let refused = apply_run_vars(&mut r, ctx, sub);
        if !refused.is_empty() {
            return Err(format!(
                "{} looks like a credential; use a secret variable",
                refused.join(", ")
            ));
        }
        for v in rb.vars() {
            if matches!(v.kind, VarKind::Enum | VarKind::Text)
                && let Some(val) = r.vars.get(&v.name)
            {
                shown.push(val.clone());
            }
        }
        let pairs: Vec<(String, String)> =
            r.vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let _ = kb.record_resolved_use(runbook_id, SystemTime::now(), &pairs);
    } else {
        let _ = kb.record_use(runbook_id, SystemTime::now());
    }
    let refs: Vec<&str> = shown.iter().map(String::as_str).collect();
    r.display_name = run::display_name(title, &refs, &run::started_label(t));
    ctx.guard = runfs::acquire(kb.root(), &r.id, t, alive).ok();
    runfs::save(kb.root(), &r).map_err(|e| format!("cannot save the run: {e}"))?;
    Ok(build_screen(kb, &rb, r, settings, ctx))
}

/// What opening a stored run gave.
pub struct Resumed {
    pub screen: RunScreen,
    /// New or secret variables to ask for first.
    pub form: Option<FormSpec>,
}

pub fn resume_run(
    kb: &Kb,
    settings: &Settings,
    run_id: &str,
    ctx: &mut RunCtx,
    alive: &dyn Fn(u32) -> bool,
) -> Result<Resumed, String> {
    let mut r = match runfs::load(kb.root(), run_id) {
        Ok(r) => r,
        Err(runfs::LoadError::NotFound) => return Err(format!("no run '{run_id}'")),
        Err(runfs::LoadError::Unreadable(e)) => {
            return Err(format!(
                "run '{run_id}' is unreadable ({e}); a .bak copy was kept — abandon it with `kb runbook abandon`"
            ));
        }
        Err(runfs::LoadError::Io(e)) => return Err(e.to_string()),
    };
    let rb =
        runbook(kb, &r.runbook_id).map_err(|_| format!("runbook missing: {}", r.runbook_id))?;
    let t = now();
    ctx.run_secrets.clear();
    ctx.step_secrets.clear();
    let (guard, read_only) = match runfs::acquire(kb.root(), &r.id, t, alive) {
        Ok(g) => (Some(g), None),
        Err(l) => (None, Some(l.pid)),
    };
    ctx.guard = guard;
    let mut drift = None;
    if read_only.is_none() {
        run::recover_interrupted(&mut r, t);
        let hash = hash_of(kb, &r.runbook_id).unwrap_or_default();
        drift = Some(run::reconcile(
            &mut r,
            rb.runbook_steps().unwrap_or_default(),
            &hash,
            t,
        ));
        let _ = runfs::save(kb.root(), &r);
    }
    let form = if read_only.is_none() {
        runbook_form(kb, &r.runbook_id, Some(&r), true, ctx)?
    } else {
        None
    };
    let mut screen = build_screen(kb, &rb, r, settings, ctx);
    screen.read_only = read_only;
    screen.drift = drift.filter(|d| d.changed);
    Ok(Resumed { screen, form })
}

/// Writes the run; the error text when it could not be written (progress stays in memory).
pub fn save(kb: &Kb, screen: &mut RunScreen) -> Option<String> {
    if screen.read_only.is_some() {
        return None;
    }
    match runfs::save(kb.root(), &screen.run) {
        Ok(()) => {
            screen.save_error = None;
            None
        }
        Err(e) => {
            screen.save_error = Some(e.to_string());
            screen.save_error.clone()
        }
    }
}

/// Every active run in the KB, for the header count and runbook previews.
pub fn active_runs(kb: &Kb) -> Vec<super::app::ActiveRun> {
    runfs::list(kb.root())
        .into_iter()
        .filter(|r| r.state == RunState::Active)
        .map(|r| {
            let live = r
                .step_states
                .values()
                .filter(|s| s.status != run::StepStatus::Removed);
            let total = live.clone().count();
            let done = live.filter(|s| s.status == run::StepStatus::Done).count();
            super::app::ActiveRun {
                runbook_id: r.runbook_id,
                name: r.display_name,
                progress: format!("{done}/{total} done"),
            }
        })
        .collect()
}

/// The picker rows for a runbook's active runs.
pub fn picker(kb: &Kb, runbook_id: &str, alive: &dyn Fn(u32) -> bool) -> RunPicker {
    let title = kb
        .entry(runbook_id)
        .ok()
        .and_then(|e| e.title().map(str::to_string))
        .unwrap_or_else(|| runbook_id.to_string());
    let steps = kb
        .entry(runbook_id)
        .ok()
        .and_then(|e| e.runbook_steps().map(<[_]>::to_vec))
        .unwrap_or_default();
    let rows = runfs::list(kb.root())
        .into_iter()
        .filter(|r| r.runbook_id == runbook_id && r.state == RunState::Active)
        .map(|r| {
            let (done, _, _) = run::counts(&r, &steps);
            RunRow {
                progress: format!("{done}/{} done", steps.len()),
                updated: r.updated_at,
                locked: runfs::locked_by(kb.root(), &r.id, alive),
                name: r.display_name,
                id: r.id,
            }
        })
        .collect();
    RunPicker::new(
        runbook_id,
        &title,
        rows,
        ReviewStateTracker::is_unreviewed(kb.root(), runbook_id),
    )
}

/// The form for variables of the selected step's entry that nothing has answered yet.
pub fn step_form(
    kb: &Kb,
    screen: &RunScreen,
    ctx: &RunCtx,
    idx: usize,
) -> Result<Option<FormSpec>, String> {
    let info = &screen.steps[idx];
    let Some(id) = &info.entry_id else {
        return Ok(None);
    };
    let rb = runbook(kb, &screen.run.runbook_id)?;
    let entry = kb.entry(id).map_err(|e| e.to_string())?;
    let row = row_for(kb, &rb, &info.step, &entry);
    let (_, missing) = values_for(&entry, &screen.run, &row, &info.step.id, ctx);
    if missing.is_empty() {
        return Ok(None);
    }
    let plan = varres::plan_vars(&entry).restricted(|name| missing.iter().any(|m| m == name));
    let bindings = varres::prefill(
        &plan,
        &Inputs {
            flags: &[],
            env: &env,
            memory: None,
        },
    );
    Ok(Some(FormSpec {
        id: id.clone(),
        title: info.title.clone(),
        plan,
        bindings,
        recent: BTreeMap::new(),
    }))
}

/// Keeps a submitted step form: plain answers in the run, secrets in this process only.
pub fn apply_step_vars(
    screen: &mut RunScreen,
    ctx: &mut RunCtx,
    step: usize,
    sub: &Submission,
) -> Vec<String> {
    let step_id = screen.steps[step].step.id.clone();
    let mut refused = Vec::new();
    for (name, value, _) in &sub.values {
        if value.is_secret() {
            ctx.step_secrets
                .insert((step_id.clone(), name.clone()), value.clone());
        } else if !run::set_step_var(&mut screen.run, &step_id, name, value.reveal()) {
            refused.push(name.clone());
        }
    }
    refused
}

/// Everything needed to act on the step's entry: its id and all variable values.
pub fn step_submission(
    kb: &Kb,
    screen: &RunScreen,
    ctx: &RunCtx,
    idx: usize,
) -> Result<Submission, String> {
    let info = &screen.steps[idx];
    let id = info.entry_id.clone().ok_or("manual step")?;
    let rb = runbook(kb, &screen.run.runbook_id)?;
    let entry = kb.entry(&id).map_err(|e| e.to_string())?;
    let row = row_for(kb, &rb, &info.step, &entry);
    let (values, _) = values_for(&entry, &screen.run, &row, &info.step.id, ctx);
    Ok(Submission {
        id,
        signature: 0,
        values: values
            .into_iter()
            .map(|(n, v)| (n, v, Source::Typed))
            .collect(),
        print: false,
        then: Then::Open,
    })
}

/// A note step's text for the full view.
pub fn note_text(kb: &Kb, id: &str) -> Option<String> {
    kb.entry(id).ok().map(|e| e.notes().to_string())
}
