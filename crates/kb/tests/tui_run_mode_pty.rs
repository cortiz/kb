//! PTY tests for runbook run mode (spec 015): the real `kb` in a terminal.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use support::pty::{CTRL_C, ENTER, ESC, Session, Spec};

const CTRL_O: &[u8] = b"\x0f";

struct Fixture {
    dir: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn marker(&self) -> PathBuf {
        self.dir.path().join("marker.txt")
    }
    fn opened(&self) -> PathBuf {
        self.dir.path().join("opened.txt")
    }
    fn runs(&self) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(self.root.join(".kb/runs")) {
            for e in rd.flatten() {
                if e.path().extension().is_some_and(|x| x == "json") {
                    out.push(serde_json::from_slice(&fs::read(e.path()).unwrap()).unwrap());
                }
            }
        }
        out
    }
    fn spec(&self, query: &str) -> Spec {
        let opener = self.dir.path().join("opener.sh");
        fs::write(
            &opener,
            format!("#!/bin/sh\necho \"$1\" >> '{}'\n", self.opened().display()),
        )
        .unwrap();
        fs::set_permissions(&opener, fs::Permissions::from_mode(0o755)).unwrap();
        Spec::new(&[query], &self.home, &self.root)
            .env("KB_OPENER_COMMAND", opener.to_str().unwrap())
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let root = dir.path().join("kb");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&root).unwrap();
    let marker = dir.path().join("marker.txt");
    write(
        &root,
        "link/docs.md",
        "---\nkb_format: 1\ntype: link\ntitle: Service docs\nurl: https://example.com/{{env}}\n---\n",
    );
    write(
        &root,
        "cmd/deploy.md",
        &format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Deploy service\nshell: sh\n---\n\n```\necho \"deploy {{{{env}}}}\" > '{}'\n```\n",
            marker.display()
        ),
    );
    write(
        &root,
        "cmd/fails.md",
        "---\nkb_format: 1\ntype: cmd\ntitle: Failing check\nshell: sh\n---\n\n```\nexit 3\n```\n",
    );
    write(
        &root,
        "runbook/service.md",
        "---\nkb_format: 1\ntype: runbook\ntitle: Service Deployment\nvars:\n  env: { kind: enum, options: [stage, prod], default: stage }\nsteps:\n  - id: docs\n    title: Open docs\n    entry: link/docs\n  - id: deploy\n    title: Deploy to {{env}}\n    entry: cmd/deploy\n  - id: verify\n    title: Check dashboard\n    do: Look at the dashboard\n    confirm: true\n---\n",
    );
    write(
        &root,
        "runbook/health.md",
        "---\nkb_format: 1\ntype: runbook\ntitle: Health Verification\nsteps:\n  - id: replica\n    title: Check replica\n    entry: cmd/fails\n  - id: tail\n    title: Last look\n    do: Look around\n---\n",
    );
    Fixture { dir, home, root }
}

fn start(f: &Fixture, query: &str, id: &str) -> Session {
    let mut s = Session::spawn(&f.spec(query));
    s.wait_for(id);
    s.send(CTRL_O);
    s
}

#[test]
fn a_run_goes_end_to_end_with_actions_commands_and_confirmations() {
    let f = fixture();
    let mut s = start(&f, "Service Deployment", "runbook/service.md");
    // the runbook's variable form; the default (stage) is fine
    s.wait_for("kb · variables");
    s.send(ENTER);
    s.wait_for("Open docs");
    s.wait_for("Deploy to stage");
    s.wait_for("0/3 done");

    // Enter opens the link; the step shows in progress
    s.send(ENTER);
    s.wait_for("opened");
    s.wait_until("opener ran", |_| f.opened().exists());
    assert!(
        fs::read_to_string(f.opened())
            .unwrap()
            .contains("https://example.com/stage")
    );

    // d marks done and moves to the next open step
    s.type_text("d");
    s.wait_for("1/3 done");

    // Enter on the command: the M7 confirmation, then it runs in the terminal
    s.send(ENTER);
    s.wait_for("Run Command");
    s.send(ENTER);
    s.wait_for("exit code 0");
    s.send(ENTER);
    s.wait_for("2/3 done");
    assert_eq!(
        fs::read_to_string(f.marker()).unwrap().trim(),
        "deploy stage"
    );

    // the manual confirm step needs y
    s.type_text("d");
    s.wait_for("Confirm 'Check dashboard' is complete?");
    s.type_text("y");
    s.wait_for("complete");

    s.type_text("F");
    s.wait_for("Service Deployment");

    let runs = f.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["state"], "finished");
    assert_eq!(runs[0]["vars"]["env"], "stage");
    assert_eq!(runs[0]["step_states"]["deploy"]["status"], "done");
    assert_eq!(runs[0]["step_states"]["deploy"]["last_exit_code"], 0);
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn a_failing_command_is_recorded_and_stays_selected() {
    let f = fixture();
    let mut s = start(&f, "Health Verification", "runbook/health.md");
    s.wait_for("· run");
    s.send(ENTER);
    s.wait_for("Run Command");
    s.send(ENTER);
    s.wait_for("exit code 3");
    s.send(ENTER);
    s.wait_for("failed (exit 3)");
    assert_eq!(f.runs()[0]["step_states"]["replica"]["status"], "failed");
    // abandon needs y
    s.type_text("A");
    s.wait_for("Abandon this run?");
    s.type_text("y");
    s.wait_for("Health Verification");
    assert_eq!(f.runs()[0]["state"], "abandoned");
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn skipping_resetting_and_notes_persist_in_the_run_file() {
    let f = fixture();
    let mut s = start(&f, "Health Verification", "runbook/health.md");
    s.wait_for("· run");
    s.type_text("s");
    s.wait_for("1 skipped");
    s.type_text("k");
    s.type_text("u");
    s.wait_for("0 skipped");
    s.type_text("n");
    s.type_text("looked fine");
    s.send(ENTER);
    s.wait_until("note saved", |_| {
        f.runs()
            .first()
            .is_some_and(|r| r["step_states"]["replica"]["notes"][0]["text"] == "looked fine")
    });
    let runs = f.runs();
    assert_eq!(
        runs[0]["step_states"]["replica"]["notes"][0]["text"],
        "looked fine"
    );
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn acting_out_of_order_warns_once_and_needs_y() {
    let f = fixture();
    let mut s = start(&f, "Health Verification", "runbook/health.md");
    s.wait_for("· run");
    s.type_text("j");
    s.type_text("d");
    s.wait_for("Check replica' is not done yet");
    s.type_text("n");
    s.wait_until("not done", |t| !t.contains("is not done yet"));
    s.type_text("d");
    s.type_text("y");
    s.wait_for("1/2 done");
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn order_warnings_can_be_turned_off() {
    let f = fixture();
    let mut spec = f.spec("Health Verification");
    spec = spec.env("KB_RUNBOOK_ORDER_WARNINGS", "false");
    let mut s = Session::spawn(&spec);
    s.wait_for("runbook/health.md");
    s.send(CTRL_O);
    s.wait_for("· run");
    s.type_text("j");
    s.type_text("d");
    s.wait_for("1/2 done");
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn several_runs_resume_from_a_picker_and_a_locked_run_is_read_only() {
    let f = fixture();
    // run 1: stage, step 1 done
    let mut s = start(&f, "Service Deployment", "runbook/service.md");
    s.wait_for("kb · variables");
    s.send(ENTER);
    s.wait_for("Open docs");
    s.type_text("d");
    s.wait_for("1/3 done");
    s.send(ESC);
    s.wait_for("runbook/service.md");
    s.send(CTRL_C);
    s.wait_exit();
    std::thread::sleep(std::time::Duration::from_millis(1100)); // distinct run ids

    // reopen: the picker lists it; start a second (prod) run
    let mut s = start(&f, "Service Deployment", "runbook/service.md");
    s.wait_for("Runs of Service Deployment");
    s.wait_for("1/3 done");
    s.wait_for("Start a new run");
    s.type_text("j");
    s.send(ENTER);
    s.wait_for("kb · variables");
    s.send(b"\x1b[C"); // enum: next option (prod)
    s.send(ENTER);
    s.wait_for("0/3 done");
    s.send(ESC);
    s.wait_for("runbook/service.md");
    s.send(CTRL_C);
    s.wait_exit();

    let runs = f.runs();
    assert_eq!(runs.len(), 2);
    let envs: Vec<_> = runs
        .iter()
        .map(|r| r["vars"]["env"].as_str().unwrap().to_string())
        .collect();
    assert!(envs.contains(&"stage".to_string()) && envs.contains(&"prod".to_string()));

    // resume the first: its step is still done
    let mut s = start(&f, "Service Deployment", "runbook/service.md");
    s.wait_for("Runs of Service Deployment");
    s.send(ENTER);
    s.wait_for("· run");
    s.wait_for("1/3 done");
    s.send(CTRL_C);
    s.wait_exit();
}

fn extra_fixture(f: &Fixture) {
    let marker = f.marker();
    write(
        &f.root,
        "cmd/wipe.md",
        &format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Wipe scratch\nshell: sh\n---\n\n```\nrm -rf /tmp/kb-run-mode-nothing && echo wiped > '{}'\n```\n",
            marker.display()
        ),
    );
    write(
        &f.root,
        "runbook/danger.md",
        "---\nkb_format: 1\ntype: runbook\ntitle: Danger Zone\nsteps:\n  - id: wipe\n    title: Wipe it\n    entry: cmd/wipe\n---\n",
    );
    write(
        &f.root,
        "runbook/confirmed.md",
        "---\nkb_format: 1\ntype: runbook\ntitle: Confirmed Deploy\nsteps:\n  - id: go\n    title: Ship it\n    entry: cmd/deploy\n    confirm: true\nvars:\n  env: { kind: text, default: stage }\n---\n",
    );
    write(
        &f.root,
        "cmd/extra.md",
        &format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Needs extra\nshell: sh\n---\n\n```\necho {{{{region}}}} > '{}'\n```\n",
            marker.display()
        ),
    );
    write(
        &f.root,
        "runbook/stepvars.md",
        "---\nkb_format: 1\ntype: runbook\ntitle: Step Variables\nsteps:\n  - id: x\n    title: Uses region\n    entry: cmd/extra\n---\n",
    );
}

#[test]
fn destructive_commands_need_the_typed_word_run() {
    let f = fixture();
    extra_fixture(&f);
    let mut s = start(&f, "Danger Zone", "runbook/danger.md");
    s.wait_for("· run");
    s.send(ENTER);
    s.wait_for("WARNING");
    s.wait_for("Type 'run' to execute:");
    s.send(ESC);
    s.wait_until("modal closed", |t| !t.contains("Type 'run'"));
    s.type_text("d");
    s.wait_for("1/1 done");
    assert!(!f.marker().exists(), "nothing ran");
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn an_unreviewed_runbook_and_its_commands_ask_for_more() {
    let f = fixture();
    kb_core::mcp::review::ReviewStateTracker::mark_unreviewed(&f.root, "runbook/health").unwrap();
    let mut s = start(&f, "Health Verification", "runbook/health.md");
    s.wait_for("unreviewed");
    s.wait_for("Start a run anyway?");
    s.type_text("y");
    s.wait_for("· run");
    // the command of an unreviewed runbook needs `run` typed even though it is not destructive
    s.send(ENTER);
    s.wait_for("Type 'run' to execute:");
    s.send(ESC);
    s.wait_until("modal closed", |t| !t.contains("Type 'run'"));
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn a_confirm_step_asks_after_its_command_succeeds() {
    let f = fixture();
    extra_fixture(&f);
    let mut s = start(&f, "Confirmed Deploy", "runbook/confirmed.md");
    s.wait_for("kb · variables");
    s.send(ENTER);
    s.wait_for("· run");
    s.send(ENTER);
    s.wait_for("Run Command");
    s.send(ENTER);
    s.wait_for("exit code 0");
    s.send(ENTER);
    s.wait_for("Confirm 'Ship it' is complete?");
    s.type_text("n");
    s.wait_for("opened");
    assert_eq!(f.runs()[0]["step_states"]["go"]["status"], "opened");
    s.type_text("d");
    s.type_text("y");
    s.wait_for("complete");
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn the_header_and_previews_show_active_runs() {
    let f = fixture();
    let mut s = start(&f, "Health Verification", "runbook/health.md");
    s.wait_for("· run");
    s.send(ESC);
    s.wait_for("1 active run");
    s.wait_for("active runs");
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn a_run_open_in_another_process_is_read_only() {
    let f = fixture();
    let mut first = start(&f, "Health Verification", "runbook/health.md");
    first.wait_for("· run");
    let mut second = Session::spawn(&f.spec("Health Verification"));
    second.wait_for("runbook/health.md");
    second.send(CTRL_O);
    second.wait_for("Runs of Health Verification");
    second.wait_for("open in PID");
    second.send(ENTER);
    second.wait_for("read-only");
    second.type_text("s");
    second.wait_for("Run locked by PID");
    assert_eq!(f.runs()[0]["step_states"]["replica"]["status"], "pending");
    second.send(CTRL_C);
    second.wait_exit();
    first.send(CTRL_C);
    first.wait_exit();
}

#[test]
fn a_changed_runbook_reconciles_by_step_id() {
    let f = fixture();
    let mut s = start(&f, "Health Verification", "runbook/health.md");
    s.wait_for("· run");
    s.type_text("s");
    s.wait_for("1 skipped");
    s.send(ESC);
    s.wait_for("runbook/health.md");
    s.send(CTRL_C);
    s.wait_exit();

    // reorder, add a step, remove none: ids keep their statuses
    write(
        &f.root,
        "runbook/health.md",
        "---\nkb_format: 1\ntype: runbook\ntitle: Health Verification\nsteps:\n  - id: fresh\n    title: A brand new step\n    do: New thing\n  - id: tail\n    title: Last look\n    do: Look around\n  - id: replica\n    title: Check replica\n    entry: cmd/fails\n---\n",
    );
    let mut s = start(&f, "Health Verification", "runbook/health.md");
    s.wait_for("Runs of Health Verification");
    s.send(ENTER);
    s.wait_for("Runbook modified since run started");
    s.wait_for("1 new");
    s.wait_for("A brand new step");
    s.wait_for("1 skipped");
    let r = &f.runs()[0];
    assert_eq!(r["step_states"]["replica"]["status"], "skipped");
    assert_eq!(r["step_states"]["fresh"]["status"], "pending");
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn run_variables_can_change_and_step_only_variables_stay_in_the_run() {
    let f = fixture();
    extra_fixture(&f);
    // change the run variables with V
    let mut s = start(&f, "Service Deployment", "runbook/service.md");
    s.wait_for("kb · variables");
    s.send(ENTER);
    s.wait_for("Deploy to stage");
    s.type_text("V");
    s.wait_for("kb · variables");
    s.send(b"\x1b[C");
    s.send(ENTER);
    s.wait_for("Deploy to prod");
    let r = &f.runs()[0];
    assert_eq!(r["vars"]["env"], "prod");
    assert_eq!(r["var_history"].as_array().unwrap().len(), 2);
    s.send(ESC);
    s.wait_for("runbook/service.md");
    s.send(CTRL_C);
    s.wait_exit();

    // a variable only the step's entry has is asked on first use and kept in this run only
    let mut s = start(&f, "Step Variables", "runbook/stepvars.md");
    s.wait_for("· run");
    s.send(ENTER);
    s.wait_for("kb · variables");
    s.type_text("eu-west-9");
    s.send(ENTER);
    s.wait_for("Run Command");
    s.send(ENTER);
    s.wait_for("exit code 0");
    s.send(ENTER);
    s.wait_for("1/1 done");
    assert_eq!(fs::read_to_string(f.marker()).unwrap().trim(), "eu-west-9");
    let runs = f.runs();
    let r = runs
        .iter()
        .find(|r| r["runbook_id"] == "runbook/stepvars")
        .unwrap();
    assert_eq!(r["step_states"]["x"]["step_vars"]["region"], "eu-west-9");
    let state = fs::read_to_string(f.root.join(".kb/state.json")).unwrap_or_default();
    assert!(
        !state.contains("eu-west-9"),
        "run answers never become everyday pre-fills"
    );
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn hostile_text_never_reaches_the_terminal() {
    let f = fixture();
    write(
        &f.root,
        "runbook/evil.md",
        "---\nkb_format: 1\ntype: runbook\ntitle: Evil Steps\nsteps:\n  - id: e\n    title: \"Bad \\e]0;PWNED\\a title\"\n    do: \"Do \\e]0;PWNED2\\a it\"\n    note: \"note \\e]0;PWNED3\\a here\"\n---\n",
    );
    let mut s = start(&f, "Evil Steps", "runbook/evil.md");
    s.wait_for("· run");
    s.wait_for("Bad");
    let raw = s.raw_output();
    assert!(
        !raw.contains("PWNED"),
        "no escape sequence from a file may be passed on"
    );
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn a_killed_command_step_comes_back_interrupted() {
    let f = fixture();
    write(
        &f.root,
        "cmd/slow.md",
        "---\nkb_format: 1\ntype: cmd\ntitle: Slow job\nshell: sh\n---\n\n```\nsleep 20\n```\n",
    );
    write(
        &f.root,
        "runbook/slow.md",
        "---\nkb_format: 1\ntype: runbook\ntitle: Slow Runbook\nsteps:\n  - id: slow\n    title: Wait a bit\n    entry: cmd/slow\n---\n",
    );
    let mut s = start(&f, "Slow Runbook", "runbook/slow.md");
    s.wait_for("· run");
    s.send(ENTER);
    s.wait_for("Run Command");
    s.send(ENTER);
    s.wait_until("running recorded", |_| {
        f.runs()
            .first()
            .is_some_and(|r| r["step_states"]["slow"]["status"] == "running")
    });
    s.signal("KILL");
    s.wait_exit(); // reaps it: a zombie would still look alive to `kill -0`

    // the dead process left its lock: it is stale, and the running step is now interrupted
    let mut s = start(&f, "Slow Runbook", "runbook/slow.md");
    s.wait_for("Runs of Slow Runbook");
    s.send(ENTER);
    s.wait_for("! interrupted");
    assert_eq!(f.runs()[0]["step_states"]["slow"]["status"], "interrupted");
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn ctrl_c_during_a_command_marks_the_step_interrupted() {
    let f = fixture();
    write(
        &f.root,
        "cmd/slow2.md",
        &format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Slow job\nshell: sh\n---\n\n```\ntouch '{}'; sleep 20\n```\n",
            f.marker().display()
        ),
    );
    write(
        &f.root,
        "runbook/slow2.md",
        "---\nkb_format: 1\ntype: runbook\ntitle: Slow Two\nsteps:\n  - id: slow\n    title: Wait a bit\n    entry: cmd/slow2\n---\n",
    );
    let mut s = start(&f, "Slow Two", "runbook/slow2.md");
    s.wait_for("· run");
    s.send(ENTER);
    s.wait_for("Run Command");
    s.send(ENTER);
    // only once the command itself runs is the terminal back in cooked mode and Ctrl-C a signal
    s.wait_until("command started", |_| f.marker().exists());
    s.send(CTRL_C);
    s.wait_until("interrupted recorded", |_| {
        f.runs()
            .first()
            .is_some_and(|r| r["step_states"]["slow"]["status"] == "interrupted")
    });
    s.send(ENTER); // the post-run pause
    s.wait_for("! interrupted");
    s.type_text("A");
    s.type_text("y");
    s.wait_for("runbook/slow2.md");
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn kb_runbook_run_opens_the_run_screen_and_new_skips_the_picker() {
    let f = fixture();
    let spec = Spec::new(&["runbook", "run", "Health Verification"], &f.home, &f.root);
    let mut s = Session::spawn(&spec);
    s.wait_for("· run");
    s.send(ESC);
    s.wait_until("list shown", |t| t.contains("1 active run"));
    s.send(CTRL_C);
    s.wait_exit();

    // with an active run, plain `run` lists it; --new goes straight to a fresh run
    let spec = Spec::new(&["runbook", "run", "Health Verification"], &f.home, &f.root);
    let mut s = Session::spawn(&spec);
    s.wait_for("Runs of Health Verification");
    s.send(CTRL_C);
    s.wait_exit();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let spec = Spec::new(
        &["runbook", "run", "Health Verification", "--new"],
        &f.home,
        &f.root,
    );
    let mut s = Session::spawn(&spec);
    s.wait_for("· run");
    s.send(CTRL_C);
    s.wait_exit();
    assert_eq!(f.runs().len(), 2);
}
