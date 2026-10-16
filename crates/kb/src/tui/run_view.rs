//! The run screen (spec 015): state, keys and drawing. No I/O; `run_ops` reads entries and does
//! the actions, and `AppState` carries events between the two.

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use kb_core::keys::{Command, Context, EffectiveKeymap};
use kb_core::runbook::run::{self, Drift, Run, RunState, StepStatus};
use kb_core::runbook::{RunbookStep, StepTarget};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Widget};

use super::keys::{Code, Key};
use super::layout::{truncate, width};
use super::markdown;
use super::theme::Theme;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// What the driver learned about one runbook step (entry, hash, resolved text).
#[derive(Debug, Clone)]
pub struct StepInfo {
    pub step: RunbookStep,
    /// The title with the run's variable values put in.
    pub title: String,
    /// `cmd`, `sumo`, ..., or `do`.
    pub kind: String,
    pub entry_id: Option<String>,
    pub missing: bool,
    /// Changed by an agent and not reviewed yet (the step's entry).
    pub unreviewed: bool,
    pub is_cmd: bool,
    pub entry_hash: Option<String>,
    /// The resolved payload / URL / location, secrets masked; or the `do` text.
    pub preview: Vec<String>,
    /// `name = value` rows, secrets masked.
    pub bindings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Primary,
    Copy,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunPrompt {
    Order {
        idx: usize,
        act: Act,
        earliest: String,
    },
    Unreviewed {
        idx: usize,
        act: Act,
    },
    ConfirmDone(usize),
    Finish,
    Abandon,
    Note(String),
}

/// What a key asks the driver to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEvent {
    None,
    /// Esc: back to the list; the run stays as it is.
    Close,
    /// The run changed: write it.
    Save,
    /// The run was finished or abandoned: write it and leave.
    SaveAndClose,
    Primary(usize),
    Copy(usize),
    EditVars,
}

#[derive(Debug, Clone)]
pub struct RunScreen {
    pub run: Run,
    pub title: String,
    pub steps: Vec<StepInfo>,
    pub selected: usize,
    pub scroll: usize,
    /// Another live process holds the run (its pid): nothing can change.
    pub read_only: Option<u32>,
    pub runbook_unreviewed: bool,
    pub drift: Option<Drift>,
    pub prompt: Option<RunPrompt>,
    pub warned: BTreeSet<String>,
    pub auto_done: bool,
    pub order_warnings: bool,
    pub message: Option<String>,
    pub save_error: Option<String>,
    pub show_help: bool,
    /// An action waiting for the step-variable form.
    pub awaiting_vars: Option<(usize, Act)>,
    /// A note step's text in full view: title, text, scroll.
    pub view: Option<(String, String, usize)>,
}

impl RunScreen {
    pub fn new(
        run: Run,
        title: String,
        steps: Vec<StepInfo>,
        auto_done: bool,
        order_warnings: bool,
    ) -> RunScreen {
        let selected = steps
            .iter()
            .position(|s| {
                run.step_states
                    .get(&s.step.id)
                    .is_none_or(|st| !st.status.is_settled())
            })
            .unwrap_or(0);
        RunScreen {
            run,
            title,
            steps,
            selected,
            scroll: 0,
            read_only: None,
            runbook_unreviewed: false,
            drift: None,
            prompt: None,
            warned: BTreeSet::new(),
            auto_done,
            order_warnings,
            message: None,
            save_error: None,
            show_help: false,
            awaiting_vars: None,
            view: None,
        }
    }

    pub fn status(&self, idx: usize) -> StepStatus {
        self.steps
            .get(idx)
            .and_then(|s| self.run.step_states.get(&s.step.id))
            .map_or(StepStatus::Pending, |s| s.status)
    }

    pub fn rb_steps(&self) -> Vec<RunbookStep> {
        self.steps.iter().map(|s| s.step.clone()).collect()
    }

    pub fn is_complete(&self) -> bool {
        run::is_complete(&self.run, &self.rb_steps())
    }

    fn removed(&self) -> Vec<&run::StepState> {
        self.run
            .step_states
            .values()
            .filter(|s| s.status == StepStatus::Removed)
            .collect()
    }

    fn needs_review(&self, idx: usize) -> bool {
        self.runbook_unreviewed || self.steps.get(idx).is_some_and(|s| s.unreviewed)
    }

    fn refuse_read_only(&mut self) -> bool {
        if let Some(pid) = self.read_only {
            self.message = Some(format!("Run locked by PID {pid} (read-only)"));
            return true;
        }
        false
    }

    /// A key, after the keymap translated it (`map`) or a prompt took it.
    pub fn key(&mut self, key: Key, map: &EffectiveKeymap) -> RunEvent {
        self.message = None;
        if let Some((_, _, scroll)) = self.view.as_mut() {
            match key.code {
                Code::Down | Code::Char('j') => *scroll += 1,
                Code::Up | Code::Char('k') => *scroll = scroll.saturating_sub(1),
                Code::Esc | Code::Enter | Code::Char('q') => self.view = None,
                _ => {}
            }
            return RunEvent::None;
        }
        if self.show_help {
            self.show_help = false;
            return RunEvent::None;
        }
        if let Some(p) = self.prompt.take() {
            return self.prompt_key(p, key);
        }
        let Some(cmd) = map.lookup(Context::Run, key) else {
            return RunEvent::None;
        };
        let last = self.steps.len().saturating_sub(1);
        match cmd {
            Command::Down => {
                self.selected = (self.selected + 1).min(last);
                self.scroll = 0;
            }
            Command::Up => {
                self.selected = self.selected.saturating_sub(1);
                self.scroll = 0;
            }
            Command::ScrollPreviewDown => self.scroll += 1,
            Command::ScrollPreviewUp => self.scroll = self.scroll.saturating_sub(1),
            Command::Help => self.show_help = true,
            Command::Close => return RunEvent::Close,
            Command::StepAction => return self.begin(Act::Primary),
            Command::CopyStep => return self.begin(Act::Copy),
            Command::MarkDone => return self.begin(Act::Done),
            Command::MarkSkipped => {
                if self.refuse_read_only() {
                    return RunEvent::None;
                }
                return self.settle(StepStatus::Skipped);
            }
            Command::ResetStep => {
                if self.refuse_read_only() {
                    return RunEvent::None;
                }
                if let Some(s) = self.steps.get(self.selected) {
                    run::reset_step(&mut self.run, &s.step.id, now());
                    return RunEvent::Save;
                }
            }
            Command::AddRunNote => {
                if !self.refuse_read_only() {
                    self.prompt = Some(RunPrompt::Note(String::new()));
                }
            }
            Command::EditRunVars => {
                if !self.refuse_read_only() {
                    return RunEvent::EditVars;
                }
            }
            Command::FinishRun => {
                if self.refuse_read_only() {
                    return RunEvent::None;
                }
                if self.is_complete() {
                    run::finish_run(&mut self.run, now());
                    return RunEvent::SaveAndClose;
                }
                self.prompt = Some(RunPrompt::Finish);
            }
            Command::AbandonRun if !self.refuse_read_only() => {
                self.prompt = Some(RunPrompt::Abandon);
            }
            _ => {}
        }
        RunEvent::None
    }

    fn prompt_key(&mut self, p: RunPrompt, key: Key) -> RunEvent {
        if let RunPrompt::Note(mut text) = p {
            match key.code {
                Code::Esc => {}
                Code::Enter => {
                    let t = text.trim();
                    if !t.is_empty()
                        && let Some(s) = self.steps.get(self.selected)
                    {
                        run::add_step_note(&mut self.run, &s.step.id, t, now());
                        return RunEvent::Save;
                    }
                }
                Code::Backspace => {
                    text.pop();
                    self.prompt = Some(RunPrompt::Note(text));
                }
                Code::Char(c) if key.mods == 0 => {
                    text.push(c);
                    self.prompt = Some(RunPrompt::Note(text));
                }
                _ => self.prompt = Some(RunPrompt::Note(text)),
            }
            return RunEvent::None;
        }
        // every other prompt is a fixed `y` (spec 013 FR-024)
        if !(key.code == Code::Char('y') && key.mods == 0) {
            return RunEvent::None;
        }
        match p {
            RunPrompt::Order { idx, act, .. } => {
                if let Some(s) = self.steps.get(idx) {
                    self.warned.insert(s.step.id.clone());
                }
                self.proceed(act, idx)
            }
            RunPrompt::Unreviewed { idx, act } => self.emit(act, idx),
            RunPrompt::ConfirmDone(idx) => self.complete(idx),
            RunPrompt::Finish => {
                run::finish_run(&mut self.run, now());
                RunEvent::SaveAndClose
            }
            RunPrompt::Abandon => {
                run::abandon_run(&mut self.run, now());
                RunEvent::SaveAndClose
            }
            RunPrompt::Note(_) => RunEvent::None,
        }
    }

    fn settle(&mut self, status: StepStatus) -> RunEvent {
        let Some(s) = self.steps.get(self.selected) else {
            return RunEvent::None;
        };
        run::mark_step(&mut self.run, &s.step.id, status, now());
        self.selected = run::next_open(&self.run, &self.rb_steps(), self.selected);
        self.scroll = 0;
        RunEvent::Save
    }

    fn complete(&mut self, idx: usize) -> RunEvent {
        self.selected = idx;
        self.settle(StepStatus::Done)
    }

    /// Order check (FR-010), then the action.
    pub fn begin(&mut self, act: Act) -> RunEvent {
        let idx = self.selected;
        if self.steps.is_empty() {
            return RunEvent::None;
        }
        if self.read_only.is_some() && act != Act::Copy {
            self.refuse_read_only();
            return RunEvent::None;
        }
        if self.steps[idx].missing && act != Act::Done {
            self.message = Some("Entry is missing: skip the step or mark it done".into());
            return RunEvent::None;
        }
        if self.order_warnings
            && self.read_only.is_none()
            && !self.warned.contains(&self.steps[idx].step.id)
            && let Some(e) = run::earlier_unfinished(&self.run, &self.rb_steps(), idx)
        {
            let earliest = self
                .steps
                .iter()
                .find(|s| s.step.id == e.id)
                .map_or_else(|| e.title.clone(), |s| s.title.clone());
            self.prompt = Some(RunPrompt::Order { idx, act, earliest });
            return RunEvent::None;
        }
        self.proceed(act, idx)
    }

    fn proceed(&mut self, act: Act, idx: usize) -> RunEvent {
        let is_do = matches!(self.steps[idx].step.target, StepTarget::Do(_));
        match act {
            Act::Done => self.done_request(idx),
            Act::Primary if is_do => self.done_request(idx),
            _ => {
                // commands carry their own stronger `run` confirmation in the run modal
                let needs_y =
                    self.needs_review(idx) && !(act == Act::Primary && self.steps[idx].is_cmd);
                if needs_y {
                    self.prompt = Some(RunPrompt::Unreviewed { idx, act });
                    RunEvent::None
                } else {
                    self.emit(act, idx)
                }
            }
        }
    }

    fn emit(&mut self, act: Act, idx: usize) -> RunEvent {
        match act {
            Act::Copy => RunEvent::Copy(idx),
            _ => RunEvent::Primary(idx),
        }
    }

    fn done_request(&mut self, idx: usize) -> RunEvent {
        if self.steps[idx].step.confirm {
            self.prompt = Some(RunPrompt::ConfirmDone(idx));
            RunEvent::None
        } else {
            self.complete(idx)
        }
    }

    /// An open or copy worked: the step is in progress.
    pub fn acted(&mut self, idx: usize, status: StepStatus) -> RunEvent {
        let Some(s) = self.steps.get(idx) else {
            return RunEvent::None;
        };
        let id = s.step.id.clone();
        if let Some(h) = s.entry_hash.clone() {
            run::record_entry_hash(&mut self.run, &id, &h);
        }
        run::mark_step(&mut self.run, &id, status, now());
        RunEvent::Save
    }

    /// The command is about to run: remember that, in case kb dies meanwhile (FR-021).
    pub fn starting(&mut self, idx: usize) {
        self.acted(idx, StepStatus::Running);
    }

    /// The command ended (FR-016).
    pub fn command_ended(
        &mut self,
        idx: usize,
        code: Option<i32>,
        signaled: bool,
        ms: u64,
    ) -> RunEvent {
        let Some(s) = self.steps.get(idx) else {
            return RunEvent::None;
        };
        let id = s.step.id.clone();
        let t = now();
        if run::record_exit(&mut self.run, &id, code, signaled, ms, t) == Some(StepStatus::Done) {
            run::mark_step(&mut self.run, &id, StepStatus::Opened, t);
            if self.auto_done {
                if self.steps[idx].step.confirm {
                    self.selected = idx;
                    self.prompt = Some(RunPrompt::ConfirmDone(idx));
                } else {
                    self.complete(idx);
                }
            }
        }
        RunEvent::Save
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

fn ago(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 86_400 {
        format!("{}d {}h", s / 86_400, s % 86_400 / 3600)
    } else if s >= 3600 {
        format!("{}h {}m", s / 3600, s % 3600 / 60)
    } else if s >= 60 {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}

fn clean(s: &str) -> String {
    kb_core::terminal::display(s).into_owned()
}

fn wrap_plain(text: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut cur = String::new();
        for word in line.split_whitespace() {
            if !cur.is_empty() && width(&cur) + 1 + width(word) > cols {
                out.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
        out.push(cur);
    }
    out
}

fn note_lines(text: &str, cols: usize) -> Vec<String> {
    let blocks = markdown::render_notes(&clean(text));
    markdown::wrap(&blocks, cols.max(8))
        .into_iter()
        .map(|l| l.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect()
}

pub fn key_bar(map: &EffectiveKeymap) -> String {
    let parts = [
        (Command::StepAction, "action"),
        (Command::CopyStep, "copy"),
        (Command::MarkDone, "done"),
        (Command::MarkSkipped, "skip"),
        (Command::ResetStep, "reset"),
        (Command::AddRunNote, "note"),
        (Command::EditRunVars, "vars"),
        (Command::FinishRun, "finish"),
        (Command::AbandonRun, "abandon"),
        (Command::Close, "back"),
        (Command::Help, "help"),
    ];
    parts
        .iter()
        .filter_map(|(c, l)| map.hint(Context::Run, *c).map(|k| format!("{k} {l}")))
        .collect::<Vec<_>>()
        .join("  ")
}

pub fn draw_run(
    screen: &RunScreen,
    map: &EffectiveKeymap,
    theme: &Theme,
    area: Rect,
    buf: &mut Buffer,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} · run ", clean(&screen.title)))
        .border_style(theme.border());
    let inner = block.inner(area);
    block.render(area, buf);
    if inner.height < 6 || inner.width < 30 {
        return;
    }
    let w = usize::from(inner.width);
    let mut y = inner.y;

    // header
    let (done, skipped, failed) = run::counts(&screen.run, &screen.rb_steps());
    let end = screen.run.finished_at.unwrap_or_else(now);
    let mut progress = format!("{done}/{} done · {skipped} skipped", screen.steps.len());
    if failed > 0 {
        progress.push_str(&format!(" · {failed} failed"));
    }
    progress.push_str(&format!(" · {}", ago(end - screen.run.created_at)));
    if screen.run.state == RunState::Active && screen.is_complete() {
        progress.push_str(" · complete");
    } else if screen.run.state != RunState::Active {
        progress.push_str(&format!(" · {:?}", screen.run.state).to_lowercase());
    }
    buf.set_string(
        inner.x,
        y,
        truncate(&clean(&screen.run.display_name), w),
        theme.text().add_modifier(Modifier::BOLD),
    );
    y += 1;
    buf.set_string(inner.x, y, truncate(&progress, w), theme.muted());
    y += 1;
    let warn = Style::default().fg(Color::Yellow);
    let banner = |text: String, y: &mut u16, buf: &mut Buffer| {
        buf.set_string(inner.x, *y, truncate(&text, w), warn);
        *y += 1;
    };
    if let Some(pid) = screen.read_only {
        banner(format!("Run locked by PID {pid} (read-only)"), &mut y, buf);
    }
    if let Some(d) = &screen.drift
        && d.changed
    {
        let mut t = String::from("Runbook modified since run started");
        if !d.added.is_empty() {
            t.push_str(&format!(" · {} new", d.added.len()));
        }
        if !d.removed.is_empty() {
            t.push_str(&format!(" · {} removed", d.removed.len()));
        }
        if d.implicit_ids {
            t.push_str(" · add explicit step ids");
        }
        banner(t, &mut y, buf);
    }
    if let Some(e) = &screen.save_error {
        banner(format!("Progress NOT saved: {}", clean(e)), &mut y, buf);
    }

    let foot = 2u16; // prompt/message line + key bar
    let body_h = inner.bottom().saturating_sub(y + foot);
    let left_w = (inner.width * 2 / 5)
        .max(24)
        .min(inner.width.saturating_sub(10));
    let sep_x = inner.x + left_w;

    // left: steps
    let removed = screen.removed();
    let total_rows = screen.steps.len()
        + if removed.is_empty() {
            0
        } else {
            1 + removed.len()
        };
    let h = usize::from(body_h);
    let off = if screen.selected >= h {
        screen.selected + 1 - h
    } else {
        0
    };
    let lw = usize::from(left_w).saturating_sub(1);
    for row in 0..h.min(total_rows.saturating_sub(off)) {
        let i = off + row;
        let yy = y + row as u16;
        if i < screen.steps.len() {
            let s = &screen.steps[i];
            let st = screen.status(i);
            let mut text = format!(
                "{}{:>2}. {} [{}] {}",
                if i == screen.selected { "›" } else { " " },
                i + 1,
                st.icon(),
                s.kind,
                clean(&s.title)
            );
            if s.unreviewed {
                text.push_str(" agent");
            }
            if run::entry_changed(
                &screen.run,
                &s.step.id,
                s.entry_hash.as_deref().unwrap_or(""),
            ) {
                text.push_str(" *");
            }
            if s.step.optional {
                text.push_str(" (opt)");
            }
            let style = if i == screen.selected {
                theme.selection()
            } else {
                match st {
                    StepStatus::Failed => Style::default().fg(Color::LightRed),
                    StepStatus::Interrupted => warn,
                    StepStatus::Done | StepStatus::Skipped => theme.muted(),
                    _ => theme.text(),
                }
            };
            buf.set_string(inner.x, yy, truncate(&text, lw), style);
        } else {
            let j = i - screen.steps.len();
            let text = if j == 0 {
                format!("── removed since start ({})", removed.len())
            } else {
                let r = removed[j - 1];
                let last = r
                    .status_history
                    .iter()
                    .rev()
                    .find(|t| t.status != StepStatus::Removed)
                    .map_or(StepStatus::Pending, |t| t.status);
                format!("   {} {}", last.icon(), clean(&r.step_id))
            };
            buf.set_string(inner.x, yy, truncate(&text, lw), theme.muted());
        }
    }
    for r in 0..body_h {
        buf.set_string(sep_x, y + r, "│", theme.border());
    }

    // right: the selected step
    let rx = sep_x + 2;
    let rw = usize::from(inner.right().saturating_sub(rx));
    let mut lines: Vec<(String, Style)> = Vec::new();
    if let Some(s) = screen.steps.get(screen.selected) {
        let st = screen.run.step_states.get(&s.step.id);
        let status = screen.status(screen.selected);
        lines.push((clean(&s.title), theme.text().add_modifier(Modifier::BOLD)));
        let mut meta = s.entry_id.clone().unwrap_or_else(|| "manual step".into());
        if s.missing {
            meta.push_str("  [missing]");
        }
        if s.unreviewed {
            meta.push_str("  [agent: unreviewed]");
        }
        if run::entry_changed(
            &screen.run,
            &s.step.id,
            s.entry_hash.as_deref().unwrap_or(""),
        ) {
            meta.push_str("  [changed since run start]");
        }
        if s.step.confirm {
            meta.push_str("  [confirm]");
        }
        lines.push((meta, theme.muted()));
        let mut stat = format!("{} {:?}", status.icon(), status).to_lowercase();
        if let Some(st) = st {
            if status == StepStatus::Failed
                && let Some(c) = st.last_exit_code
            {
                stat.push_str(&format!(" (exit {c})"));
            }
            if let Some(ms) = st.last_duration_ms {
                stat.push_str(&format!(" · last run {:.1}s", ms as f64 / 1000.0));
            }
        }
        lines.push((stat, theme.text()));
        if let Some(n) = &s.step.note {
            lines.push((String::new(), theme.text()));
            for l in note_lines(n, rw) {
                lines.push((l, theme.text()));
            }
        }
        if !s.preview.is_empty() {
            lines.push((String::new(), theme.text()));
            for l in &s.preview {
                lines.push((clean(l), theme.variable()));
            }
        }
        if !s.bindings.is_empty() {
            lines.push((String::new(), theme.text()));
            lines.push(("Variables".into(), theme.muted()));
            for b in &s.bindings {
                lines.push((format!("  {}", clean(b)), theme.text()));
            }
        }
        if let Some(st) = st
            && !st.notes.is_empty()
        {
            lines.push((String::new(), theme.text()));
            lines.push(("Notes".into(), theme.muted()));
            for n in &st.notes {
                for l in wrap_plain(
                    &format!("{} {}", crate::output::utc(n.timestamp), clean(&n.text)),
                    rw.saturating_sub(2),
                ) {
                    lines.push((format!("  {l}"), theme.text()));
                }
            }
        }
    }
    let max_scroll = lines.len().saturating_sub(usize::from(body_h));
    let skip = screen.scroll.min(max_scroll);
    for (r, (text, style)) in lines
        .iter()
        .skip(skip)
        .take(usize::from(body_h))
        .enumerate()
    {
        buf.set_string(rx, y + r as u16, truncate(&clean(text), rw), *style);
    }

    // prompt / message line, then the key bar
    let py = inner.bottom() - 2;
    let (ptext, pstyle) = match &screen.prompt {
        Some(RunPrompt::Order { earliest, .. }) => (
            format!(
                "Step '{}' is not done yet. Continue anyway? [y/N]",
                clean(earliest)
            ),
            warn,
        ),
        Some(RunPrompt::Unreviewed { .. }) => (
            "Changed by an agent and not reviewed. Continue? [y/N]".to_string(),
            warn,
        ),
        Some(RunPrompt::ConfirmDone(i)) => (
            format!(
                "Confirm '{}' is complete? [y/N]",
                clean(&screen.steps[*i].title)
            ),
            warn,
        ),
        Some(RunPrompt::Finish) => (
            "The run is not complete. Finish it anyway? [y/N]".into(),
            warn,
        ),
        Some(RunPrompt::Abandon) => ("Abandon this run? [y/N]".into(), warn),
        Some(RunPrompt::Note(t)) => (format!("Note: {}▌", clean(t)), theme.text()),
        None => (
            clean(&screen.message.clone().unwrap_or_default()),
            theme.muted(),
        ),
    };
    buf.set_string(inner.x, py, truncate(&ptext, w), pstyle);
    buf.set_string(inner.x, py + 1, truncate(&key_bar(map), w), theme.muted());

    if let Some((title, text, scroll)) = &screen.view {
        let r = inner;
        ratatui::widgets::Clear.render(r, buf);
        buf.set_string(
            r.x,
            r.y,
            truncate(&clean(title), usize::from(r.width)),
            theme.text().add_modifier(Modifier::BOLD),
        );
        let body = note_lines(text, usize::from(r.width));
        let rows = usize::from(r.height.saturating_sub(2));
        let skip = (*scroll).min(body.len().saturating_sub(rows));
        for (i, l) in body.iter().skip(skip).take(rows).enumerate() {
            buf.set_string(
                r.x,
                r.y + 1 + i as u16,
                truncate(l, usize::from(r.width)),
                theme.text(),
            );
        }
        buf.set_string(r.x, r.bottom() - 1, "Esc back to the run", theme.muted());
    }
    if screen.show_help {
        draw_help(map, theme, inner, buf);
    }
}

fn draw_help(map: &EffectiveKeymap, theme: &Theme, area: Rect, buf: &mut Buffer) {
    let rows: Vec<String> = map
        .context(Context::Run)
        .map(|(c, _)| {
            format!(
                "{:<22}{}",
                c.name(),
                map.hint(Context::Run, c).unwrap_or_default()
            )
        })
        .collect();
    let w = 36u16.min(area.width);
    let h = (rows.len() as u16 + 2).min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let r = Rect::new(x, y, w, h);
    ratatui::widgets::Clear.render(r, buf);
    let b = Block::default()
        .borders(Borders::ALL)
        .title(" run keys ")
        .border_style(theme.border());
    let inner = b.inner(r);
    b.render(r, buf);
    for (i, row) in rows.iter().take(usize::from(inner.height)).enumerate() {
        buf.set_string(
            inner.x,
            inner.y + i as u16,
            truncate(row, usize::from(inner.width)),
            theme.text(),
        );
    }
}
