//! The TUI end to end: the real `kb` under a pseudo-terminal.

mod support;

use std::fs;
use std::time::Duration;

use kb_core::entry_json::PrintMode;
use kb_core::search::Kb;
use support::pty::*;

fn expected(root: &std::path::Path, id: &str, mode: PrintMode) -> String {
    Kb::open(root).unwrap().pick(id, mode).unwrap()
}

// ---- User Story 1 ----

#[test]
fn the_first_screen_has_every_region() {
    let (_t, home, root) = kb_home("search");
    let mut s = Session::spawn(&Spec::new(&[], &home, &root));
    s.wait_for("32 entries");
    s.wait_for("Auth middleware");
    s.wait_for("Enter pick"); // the whole first frame, not a half-drawn one
    let rows = s.rows();
    assert!(
        rows[0].contains("kb") && rows[0].contains("~/kb") && rows[0].contains("32 entries"),
        "{}",
        rows[0]
    );
    assert!(rows[1].trim_start().starts_with('›'), "{}", rows[1]);
    assert!(rows[1].trim_end().ends_with("32 / 32"), "{}", rows[1]);
    assert!(
        rows[2].contains("all 32") && rows[2].contains("sql 5"),
        "{}",
        rows[2]
    );
    let text = s.screen();
    assert!(text.contains("results") && text.contains("preview"));
    assert!(rows[39].contains("Enter pick"), "{}", rows[39]);
    assert!(s.alternate_screen());
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn typing_updates_the_list_and_enter_prints_the_pick() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("out");
    let mut spec = Spec::new(&[], &home, &root);
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    s.wait_for("32 entries");
    s.type_text("5xx auth");
    s.wait_until("the first result", |t| {
        t.contains("1 / 32") || t.contains("2 / 32")
    });
    s.wait_for("5xx by authorizer");
    let rows = s.rows();
    assert!(rows[4].contains("5xx by authorizer"), "{}", rows[4]);
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);
    let want = expected(&root, "sumo/5xx-by-authorizer", PrintMode::Payload);
    assert_eq!(fs::read_to_string(&out).unwrap(), want);
    assert!(!want.ends_with("\n\n"));
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".kb/state.json")).unwrap()).unwrap();
    assert_eq!(state["usage"]["sumo/5xx-by-authorizer"]["count"], 1);
}

#[test]
fn the_print_modes() {
    let (t, home, root) = kb_home("search");
    for (mode, args) in [
        (PrintMode::Id, ["--print", "id"]),
        (PrintMode::Path, ["--print", "path"]),
        (PrintMode::Json, ["--print", "json"]),
    ] {
        let out = t.path().join(format!("out-{mode:?}"));
        let mut spec = Spec::new(&args, &home, &root);
        spec.out = Some(out.clone());
        let mut s = Session::spawn(&spec);
        s.wait_for("32 entries");
        s.type_text("5xx auth");
        s.wait_for("5xx by authorizer");
        s.send(ENTER);
        assert_eq!(s.wait_exit(), 0, "{mode:?}");
        let got = fs::read_to_string(&out).unwrap();
        assert_eq!(
            got,
            expected(&root, "sumo/5xx-by-authorizer", mode),
            "{mode:?}"
        );
    }
    let got = fs::read_to_string(t.path().join("out-Id")).unwrap();
    assert_eq!(got, "sumo/5xx-by-authorizer");
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(t.path().join("out-Json")).unwrap()).unwrap();
    assert_eq!(json["title"], "5xx by authorizer");
}

#[test]
fn words_after_kb_prefill_the_search_line() {
    let (_t, home, root) = kb_home("search");
    let mut s = Session::spawn(&Spec::new(&["5xx", "auth"], &home, &root));
    s.wait_for("5xx by authorizer");
    let rows = s.rows();
    assert!(rows[1].contains("5xx auth"), "{}", rows[1]);
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn escape_clears_the_text_then_quits_with_130_and_prints_nothing() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("out");
    let mut spec = Spec::new(&[], &home, &root);
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    s.wait_for("32 entries");
    s.type_text("foo");
    s.wait_until("text shown", |t| t.contains("› foo"));
    s.send(ESC);
    s.wait_until("text cleared", |t| !t.contains("› foo"));
    std::thread::sleep(Duration::from_millis(100));
    assert!(s.is_running());
    s.send(ESC);
    assert_eq!(s.wait_exit(), 130);
    assert_eq!(fs::read_to_string(&out).unwrap(), "");
}

#[test]
fn stdout_can_be_a_pipe_and_only_the_pick_goes_through_it() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("out");
    let mut spec = Spec::new(&[], &home, &root);
    spec.out = Some(out.clone());
    spec.pipe = true;
    let mut s = Session::spawn(&spec);
    s.wait_for("32 entries");
    s.type_text("5xx auth");
    s.wait_for("5xx by authorizer");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);
    let got = fs::read_to_string(&out).unwrap();
    assert_eq!(
        got,
        expected(&root, "sumo/5xx-by-authorizer", PrintMode::Payload)
    );
    assert!(!got.contains('\x1b'));
    assert!(
        !s.raw_output().contains(&got),
        "the UI is drawn on the terminal, not through stdout"
    );
}

#[test]
fn the_terminal_is_restored_after_the_exit() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("out");
    let mut spec = Spec::new(&[], &home, &root);
    spec.out = Some(out.clone());
    spec.after = Some("stty -a".into());
    let mut s = Session::spawn(&spec);
    s.wait_for("32 entries");
    s.send(ESC);
    s.wait_for("exit=130");
    s.wait_for("icanon");
    let screen = s.screen();
    assert!(
        !screen.contains("-icanon") && !screen.contains("-echo "),
        "{screen}"
    );
    assert!(!s.alternate_screen() && !s.cursor_hidden());
}

/// Writes `~/.config/kb/config.toml`, where both platforms look first.
fn write_config(home: &std::path::Path, text: &str) {
    let dir = home.join(".config/kb");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("config.toml"), text).unwrap();
}

#[test]
fn a_bad_theme_does_not_stop_the_ui_from_starting() {
    // FR-016: config problems never stop kb; the default theme is used
    let (_t, home, root) = kb_home("search");
    write_config(&home, "theme = \"blue\"\n");
    let mut s = Session::spawn(&Spec::new(&[], &home, &root));
    s.wait_for("32 entries");
    s.send(ESC);
    assert_eq!(s.wait_exit(), 130);
}

// ---- M4 User Story 1: Enter copies ----

/// The clipboard program kb tries first on this host, and the environment that selects it: `pbcopy`
/// on macOS, `wl-copy` under Wayland on Linux.
fn native_clipboard() -> (&'static str, Vec<(&'static str, &'static str)>) {
    if cfg!(target_os = "macos") {
        ("pbcopy", vec![])
    } else {
        ("wl-copy", vec![("WAYLAND_DISPLAY", "wayland-test")])
    }
}

/// What Enter should have put on the clipboard for `id`, and how many lines it has.
fn clipboard_text(root: &std::path::Path, id: &str) -> (String, usize) {
    let text = expected(root, id, PrintMode::Payload);
    let text = text.trim_end_matches(['\n', '\r']).to_string();
    let lines = text.lines().count();
    (text, lines)
}

fn lines_label(n: usize) -> String {
    format!("{n} line{}", if n == 1 { "" } else { "s" })
}

/// Searches for `5xx auth` and presses Enter; returns the exit code.
fn pick_the_headline_entry(s: &mut Session) -> u32 {
    s.wait_for("32 entries");
    s.type_text("5xx auth");
    s.wait_for("5xx by authorizer");
    s.send(ENTER);
    s.wait_exit()
}

#[test]
fn enter_copies_with_the_native_tool_and_confirms_on_stderr() {
    let (t, home, root) = kb_home("search");
    let (tool, extra_env) = native_clipboard();
    let copied = t.path().join("copied");
    let out = t.path().join("stdout");
    let mut spec = Spec::new(&[], &home, &root)
        .copy_mode("auto")
        .env("KB_TEST_COPIED", copied.to_str().unwrap())
        .with_fake_tool(tool, "cat > \"$KB_TEST_COPIED\"");
    for (k, v) in extra_env {
        spec = spec.env(k, v);
    }
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    assert_eq!(pick_the_headline_entry(&mut s), 0);
    let (text, lines) = clipboard_text(&root, "sumo/5xx-by-authorizer");
    assert_eq!(fs::read_to_string(&copied).unwrap(), text);
    assert_eq!(fs::read_to_string(&out).unwrap(), "", "nothing on stdout");
    let screen = s.screen();
    assert!(
        screen.contains(&format!(
            "Copied sumo/5xx-by-authorizer ({}) via {tool}",
            lines_label(lines)
        )),
        "{screen}"
    );
    assert!(
        !s.alternate_screen() && !s.cursor_hidden(),
        "terminal restored"
    );
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".kb/state.json")).unwrap()).unwrap();
    assert_eq!(state["usage"]["sumo/5xx-by-authorizer"]["count"], 1);
}

#[test]
fn over_ssh_enter_sends_the_text_through_the_terminal() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("stdout");
    let mut spec = Spec::new(&[], &home, &root)
        .copy_mode("auto")
        .env("SSH_CONNECTION", "1.2.3.4 22 5.6.7.8 22");
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    assert_eq!(pick_the_headline_entry(&mut s), 0);
    let (text, lines) = clipboard_text(&root, "sumo/5xx-by-authorizer");
    let osc = format!(
        "\x1b]52;c;{}\x07",
        kb_cli::platform::osc52::base64(text.as_bytes())
    );
    assert!(s.raw_output().contains(&osc), "no OSC 52 in the output");
    assert!(
        s.guarded_calls().is_empty(),
        "no local tool is used over SSH"
    );
    let screen = s.screen();
    assert!(
        screen.contains(&format!(
            "Sent sumo/5xx-by-authorizer ({}) via terminal (OSC 52)",
            lines_label(lines)
        )),
        "{screen}"
    );
    assert!(
        !screen.contains("Copied"),
        "kb cannot confirm a terminal copy: {screen}"
    );
    assert_eq!(fs::read_to_string(&out).unwrap(), "");
}

#[test]
fn inside_tmux_the_sequence_is_wrapped_for_passthrough() {
    let (_t, home, root) = kb_home("search");
    let spec = Spec::new(&[], &home, &root)
        .copy_mode("osc52")
        .env("TMUX", "/tmp/tmux-1/default,1,0");
    let mut s = Session::spawn(&spec);
    assert_eq!(pick_the_headline_entry(&mut s), 0);
    assert!(
        s.raw_output().contains("\x1bPtmux;\x1b\x1b]52;c;"),
        "not wrapped for tmux"
    );
}

#[test]
fn the_stdout_method_prints_the_trimmed_payload_and_says_so() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("stdout");
    let mut spec = Spec::new(&[], &home, &root).copy_mode("stdout");
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    assert_eq!(pick_the_headline_entry(&mut s), 0);
    let (text, lines) = clipboard_text(&root, "sumo/5xx-by-authorizer");
    assert_eq!(fs::read_to_string(&out).unwrap(), text, "no newline added");
    let screen = s.screen();
    assert!(
        screen.contains(&format!(
            "Printed sumo/5xx-by-authorizer ({}) to stdout",
            lines_label(lines)
        )),
        "{screen}"
    );
}

#[test]
fn a_payload_over_the_limit_is_refused_not_dumped() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("stdout");
    let mut spec = Spec::new(&[], &home, &root)
        .copy_mode("auto")
        .env("SSH_CONNECTION", "1.2.3.4 22 5.6.7.8 22")
        .env("KB_CLIPBOARD_OSC52_MAX_BYTES", "10");
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    assert_eq!(pick_the_headline_entry(&mut s), 2);
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        "",
        "the payload is not printed"
    );
    let screen = s.screen();
    assert!(
        screen.contains("over clipboard.osc52_max_bytes (10)"),
        "{screen}"
    );
    assert!(
        screen.contains("kb show --raw sumo/5xx-by-authorizer"),
        "{screen}"
    );
    assert!(!s.raw_output().contains("\x1b]52;"), "nothing was sent");
    let state = fs::read_to_string(root.join(".kb/state.json")).unwrap_or_default();
    assert!(
        !state.contains("sumo/5xx-by-authorizer"),
        "no use recorded: {state}"
    );
}

#[test]
fn tui_enter_print_in_the_config_file_prints_like_m3() {
    let (t, home, root) = kb_home("search");
    write_config(&home, "[tui]\nenter = \"print\"\n");
    let out = t.path().join("stdout");
    // the harness default is print too; remove it so the file is what decides
    let mut spec = Spec::new(&[], &home, &root).unset("KB_TUI_ENTER");
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    assert_eq!(pick_the_headline_entry(&mut s), 0);
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        expected(&root, "sumo/5xx-by-authorizer", PrintMode::Payload)
    );
}

#[test]
fn the_print_flag_beats_the_enter_setting_in_both_directions() {
    let (t, home, root) = kb_home("search");
    // tui.enter = copy (the default) and --print id: the id is printed, nothing is copied
    let out = t.path().join("id");
    let mut spec = Spec::new(&["--print", "id"], &home, &root).unset("KB_TUI_ENTER");
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    assert_eq!(pick_the_headline_entry(&mut s), 0);
    assert_eq!(fs::read_to_string(&out).unwrap(), "sumo/5xx-by-authorizer");
    assert!(s.guarded_calls().is_empty());
    // tui.enter = print in the file and --print payload: still prints
    write_config(&home, "[tui]\nenter = \"copy\"\n");
    let out = t.path().join("payload");
    let mut spec = Spec::new(&["--print", "payload"], &home, &root).unset("KB_TUI_ENTER");
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    assert_eq!(pick_the_headline_entry(&mut s), 0);
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        expected(&root, "sumo/5xx-by-authorizer", PrintMode::Payload)
    );
}

#[test]
fn config_problems_do_not_stop_the_ui_and_are_reported_after_it_exits() {
    let (_t, home, root) = kb_home("search");
    write_config(&home, "theme = \"blue\"\nbogus = 1\n");
    let mut s = Session::spawn(&Spec::new(&[], &home, &root));
    s.wait_for("32 entries");
    s.wait_for("⚠ config: 2 problems (run kb doctor)");
    s.send(ESC);
    assert_eq!(s.wait_exit(), 130);
    let screen = s.screen();
    assert!(
        screen.contains(
            "theme: \"blue\" is not valid (expected one of auto, dark, light); using \"auto\""
        ),
        "{screen}"
    );
    assert!(
        screen.contains("unknown key \"bogus\" (ignored)"),
        "{screen}"
    );
}

// ---- M4 User Story 2: an ambiguous `kb copy` / `kb show` opens the UI ----

/// The id of the first result for `query`, as the UI will show it.
fn first_result(root: &std::path::Path, query: &str) -> String {
    let mut kb = Kb::open(root).unwrap();
    let q = kb_core::query::Query::parse(query);
    kb.search(&q, std::time::SystemTime::now()).unwrap().results[0]
        .id
        .clone()
}

#[test]
fn an_ambiguous_copy_opens_the_ui_and_enter_copies_even_when_tui_enter_is_print() {
    let (t, home, root) = kb_home("search");
    let (tool, extra_env) = native_clipboard();
    let copied = t.path().join("copied");
    // the harness default is KB_TUI_ENTER=print: kb copy must ignore it
    let mut spec = Spec::new(&["copy", "latency"], &home, &root)
        .env("KB_CLIPBOARD_METHOD", "native")
        .env("KB_TEST_COPIED", copied.to_str().unwrap())
        .with_fake_tool(tool, "cat > \"$KB_TEST_COPIED\"");
    for (k, v) in extra_env {
        spec = spec.env(k, v);
    }
    let mut s = Session::spawn(&spec);
    s.wait_for("Latency p99 production"); // the results, not just the first frame
    assert!(s.rows()[1].contains("latency"), "{}", s.rows()[1]);
    let id = first_result(&root, "latency");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);
    let (text, _) = clipboard_text(&root, &id);
    assert_eq!(fs::read_to_string(&copied).unwrap(), text);
    assert!(
        s.screen().contains(&format!("Copied {id}")),
        "{}",
        s.screen()
    );
}

#[test]
fn cancelling_an_ambiguous_copy_copies_nothing() {
    let (t, home, root) = kb_home("search");
    let (tool, extra_env) = native_clipboard();
    let copied = t.path().join("copied");
    let mut spec = Spec::new(&["copy", "latency"], &home, &root)
        .env("KB_CLIPBOARD_METHOD", "native")
        .env("KB_TEST_COPIED", copied.to_str().unwrap())
        .with_fake_tool(tool, "cat > \"$KB_TEST_COPIED\"");
    for (k, v) in extra_env {
        spec = spec.env(k, v);
    }
    let mut s = Session::spawn(&spec);
    s.wait_for("Latency p99 production");
    s.send(ESC); // clears the query
    s.wait_until("query cleared", |t| !t.contains("› latency"));
    s.send(ESC);
    assert_eq!(s.wait_exit(), 130);
    assert!(!copied.exists());
}

#[test]
fn an_ambiguous_show_opens_the_ui_and_enter_prints_the_formatted_entry() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("stdout");
    let mut spec = Spec::new(&["show", "latency"], &home, &root);
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    s.wait_for("Latency p99 production");
    let id = first_result(&root, "latency");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);
    let printed = fs::read_to_string(&out).unwrap();
    assert!(
        printed.contains("── payload"),
        "the preview as text: {printed}"
    );
    assert!(printed.contains(&format!("{id}.md")), "{printed}");
    assert!(!printed.contains('\x1b'), "stdout is a file, so no colors");
}

// ---- M4 User Story 5: the editor comes from the platform layer ----

#[test]
fn ctrl_e_runs_the_editor_named_in_the_config_not_the_environment() {
    let (t, home, root) = kb_home("search");
    let marker = t.path().join("which");
    let configured = script(
        t.path(),
        "configured",
        &format!("echo configured > {}", marker.display()),
    );
    let from_env = script(
        t.path(),
        "from-env",
        &format!("echo env > {}", marker.display()),
    );
    write_config(&home, &format!("editor = \"{}\"\n", configured.display()));
    let spec = Spec::new(&[], &home, &root)
        .env("VISUAL", from_env.to_str().unwrap())
        .env("EDITOR", from_env.to_str().unwrap());
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    s.send(CTRL_E);
    s.wait_until("the editor ran", |_| {
        fs::read_to_string(&marker).is_ok_and(|t| !t.is_empty())
    });
    assert_eq!(fs::read_to_string(&marker).unwrap().trim(), "configured");
    s.wait_for("32 entries");
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn kb_editor_in_the_environment_beats_the_file() {
    let (t, home, root) = kb_home("search");
    let marker = t.path().join("which");
    let from_file = script(
        t.path(),
        "file",
        &format!("echo file > {}", marker.display()),
    );
    let from_kb_editor = script(t.path(), "kbe", &format!("echo kbe > {}", marker.display()));
    write_config(&home, &format!("editor = \"{}\"\n", from_file.display()));
    let spec = Spec::new(&[], &home, &root).env("KB_EDITOR", from_kb_editor.to_str().unwrap());
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    s.send(CTRL_E);
    s.wait_until("the editor ran", |_| {
        fs::read_to_string(&marker).is_ok_and(|t| !t.is_empty())
    });
    assert_eq!(fs::read_to_string(&marker).unwrap().trim(), "kbe");
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn ctrl_y_copies_with_the_platform_tool_and_stays_open() {
    let (t, home, root) = kb_home("search");
    let (tool, extra_env) = native_clipboard();
    let copied = t.path().join("copied");
    let mut spec = Spec::new(&[], &home, &root)
        .env("KB_CLIPBOARD_METHOD", "native")
        .env("KB_TEST_COPIED", copied.to_str().unwrap())
        .with_fake_tool(tool, "cat > \"$KB_TEST_COPIED\"");
    for (k, v) in extra_env {
        spec = spec.env(k, v);
    }
    let mut s = Session::spawn(&spec);
    s.wait_for("32 entries");
    s.type_text("5xx auth");
    s.wait_for("5xx by authorizer");
    s.send(CTRL_Y);
    let (text, _) = clipboard_text(&root, "sumo/5xx-by-authorizer");
    s.wait_until("the key bar says how it was copied", |t| {
        t.contains(&format!("Copied via {tool}"))
    });
    assert_eq!(fs::read_to_string(&copied).unwrap(), text);
    assert!(s.alternate_screen() && s.is_running(), "the UI stays open");
    // the message goes away by itself after two seconds
    s.wait_until("the key bar is back", |t| {
        !t.contains("Copied via") && t.contains("Enter pick")
    });
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".kb/state.json")).unwrap()).unwrap();
    assert_eq!(state["usage"]["sumo/5xx-by-authorizer"]["count"], 1);
}

#[test]
fn ctrl_y_over_ssh_sends_through_the_terminal_without_disturbing_the_screen() {
    let (_t, home, root) = kb_home("search");
    let spec = Spec::new(&[], &home, &root).env("SSH_CONNECTION", "1.2.3.4 22 5.6.7.8 22");
    let mut s = Session::spawn(&spec);
    s.wait_for("32 entries");
    s.type_text("5xx auth");
    s.wait_for("sumo/5xx-by-authorizer.md"); // the preview has loaded
    // the header shows "refreshing…" until the background refresh ends; on a slow machine that
    // can be after the preview, and would change the row between the two readings
    s.wait_until("the refresh is over", |t| !t.contains("refreshing…"));
    let before: Vec<String> = s.rows()[..4].to_vec();
    s.send(CTRL_Y);
    s.wait_for("Sent via terminal (OSC 52)");
    let (text, _) = clipboard_text(&root, "sumo/5xx-by-authorizer");
    let osc = format!(
        "\x1b]52;c;{}\x07",
        kb_cli::platform::osc52::base64(text.as_bytes())
    );
    assert!(s.raw_output().contains(&osc));
    // the header, search line, chips and box tops are untouched, and no escape text leaked in
    assert_eq!(s.rows()[..4], before[..]);
    assert!(
        !s.screen().contains("52;c") && !s.screen().contains("]52"),
        "{}",
        s.screen()
    );
    assert!(
        !s.screen().contains("Copied"),
        "kb cannot confirm a terminal copy"
    );
    assert!(s.guarded_calls().is_empty());
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn ctrl_y_with_a_payload_over_the_limit_says_so_and_records_nothing() {
    let (_t, home, root) = kb_home("search");
    // auto over ssh is the terminal, then (in the UI) nothing; a forced osc52 would fall back to a tool
    let spec = Spec::new(&[], &home, &root)
        .env("SSH_CONNECTION", "1.2.3.4 22 5.6.7.8 22")
        .env("KB_CLIPBOARD_METHOD", "auto")
        .env("KB_CLIPBOARD_OSC52_MAX_BYTES", "10");
    let mut s = Session::spawn(&spec);
    s.wait_for("32 entries");
    s.type_text("5xx auth");
    s.wait_for("5xx by authorizer");
    s.send(CTRL_Y);
    s.wait_for("Too large for the terminal");
    assert!(!s.raw_output().contains("\x1b]52;"));
    s.send(CTRL_C);
    s.wait_exit();
    assert!(!root.join(".kb/state.json").exists());
}

#[test]
fn ctrl_y_never_prints_into_the_screen_even_with_the_stdout_method() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("stdout");
    let mut spec = Spec::new(&[], &home, &root).env("KB_CLIPBOARD_METHOD", "stdout");
    spec.out = Some(out.clone());
    let mut s = Session::spawn(&spec);
    s.wait_for("32 entries");
    s.type_text("5xx auth");
    s.wait_for("5xx by authorizer");
    s.send(CTRL_Y);
    s.wait_for("Copy failed");
    assert_eq!(
        fs::read_to_string(&out).unwrap_or_default(),
        "",
        "nothing on stdout"
    );
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
    assert!(
        !root.join(".kb/state.json").exists(),
        "no use for a copy that did not happen"
    );
}

// ---- User Story 3 ----

#[test]
fn tab_filters_by_type_and_shows_the_chip_counts() {
    let (_t, home, root) = kb_home("search");
    let mut s = Session::spawn(&Spec::new(&[], &home, &root));
    s.wait_for("32 entries");
    s.type_text("latency");
    s.wait_until("matches", |t| {
        t.contains("/ 32") && t.contains("Latency p99")
    });
    let before = s.screen();
    assert!(before.contains("sumo") || before.contains("cw"));
    s.send(TAB); // sql
    s.send(TAB); // sumo
    s.send(TAB); // cw
    s.wait_until("only cw rows", |t| {
        let rows: Vec<&str> = t
            .lines()
            .skip(4)
            .take(20)
            .filter(|l| l.contains("│ "))
            .collect();
        t.lines().nth(2).is_some_and(|l| l.contains("cw "))
            && rows.iter().any(|l| l.contains("cw "))
            && !rows.iter().any(|l| l.contains("│ sumo"))
    });
    s.send(BACKTAB);
    s.send(BACKTAB);
    s.send(BACKTAB);
    s.wait_until("all types again", |t| t.contains("│ sumo"));
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn a_typed_tag_becomes_a_chip_and_backspace_removes_it() {
    let (_t, home, root) = kb_home("search");
    let mut s = Session::spawn(&Spec::new(&[], &home, &root));
    s.wait_for("32 entries");
    s.type_text("tag:prod ");
    s.wait_for("tag:prod ×");
    s.wait_until("fewer results", |t| {
        t.lines().nth(1).is_some_and(|l| !l.contains("32 / 32"))
    });
    let narrowed = s.rows()[1].clone();
    s.send(BACKSPACE);
    s.wait_until("chip gone", |t| !t.contains("tag:prod ×"));
    s.wait_for("32 / 32");
    assert_ne!(narrowed, s.rows()[1]);
    s.type_text("\"role cache\"");
    s.wait_for("Role cache refresh notes");
    s.wait_until("one match", |t| t.contains("1 / 32"));
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

// ---- User Story 4 ----

fn script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn ctrl_e_hands_the_file_to_the_editor_and_comes_back_with_the_change() {
    let (t, home, root) = kb_home("search");
    let marker = t.path().join("marker");
    let editor = script(
        t.path(),
        "ed",
        &format!(
            "echo \"$1\" > {m}\nsed -i.bak 's/^title: Cache$/title: Caching layers/' \"$1\"\nrm -f \"$1.bak\"",
            m = marker.display()
        ),
    );
    let mut spec = Spec::new(&[], &home, &root);
    spec = spec.env("EDITOR", editor.to_str().unwrap());
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    s.type_text("Cache");
    s.wait_until("the entry first", |t| {
        t.lines()
            .nth(4)
            .is_some_and(|l| l.contains("│ prompt") && l.contains("Cache"))
    });
    s.send(CTRL_E);
    s.wait_until("the editor ran", |_| marker.exists());
    s.wait_for("Caching layers");
    s.wait_until("the preview shows the new title too", |t| {
        t.lines()
            .nth(4)
            .is_some_and(|l| l.contains("│ Caching layers"))
    });
    assert_eq!(
        fs::read_to_string(&marker).unwrap().trim(),
        root.join("prompt/cache.md")
            .display()
            .to_string()
            .replace("/var/", "/private/var/")
    );
    assert!(s.alternate_screen(), "back in the UI");
    assert!(s.screen().contains("› Cache"), "the query is still there");
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn an_error_after_editing_is_reported_and_the_entry_leaves_the_list() {
    let (t, home, root) = kb_home("search");
    let editor = script(
        t.path(),
        "breaker",
        "sed -i.bak 's/^type: prompt$/type: nope/' \"$1\"\nrm -f \"$1.bak\"",
    );
    let mut spec = Spec::new(&[], &home, &root);
    spec = spec.env("EDITOR", editor.to_str().unwrap());
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    s.type_text("Cache");
    s.wait_until("the entry first", |t| {
        t.lines().nth(4).is_some_and(|l| l.contains("Cache"))
    });
    s.send(CTRL_E);
    s.wait_for("KB001");
    s.wait_until("entry gone", |t| {
        !t.contains("Cache warmup procedures") && t.contains("1 with errors")
            || t.contains("⚠ 1 with errors")
    });
    s.send(b"x");
    s.wait_until("notice cleared", |t| !t.contains("KB001"));
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn visual_wins_over_editor_and_a_missing_editor_does_not_close_the_ui() {
    let (t, home, root) = kb_home("search");
    let marker = t.path().join("which");
    let visual = script(
        t.path(),
        "vis",
        &format!("echo visual > {}", marker.display()),
    );
    let editor = script(
        t.path(),
        "edt",
        &format!("echo editor > {}", marker.display()),
    );
    let mut spec = Spec::new(&[], &home, &root);
    spec = spec
        .env("VISUAL", visual.to_str().unwrap())
        .env("EDITOR", editor.to_str().unwrap());
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    s.send(CTRL_E);
    // the shell creates the file before `echo` fills it: wait for the content
    s.wait_until("the editor ran", |_| {
        fs::read_to_string(&marker).is_ok_and(|t| !t.is_empty())
    });
    assert_eq!(fs::read_to_string(&marker).unwrap().trim(), "visual");
    s.wait_for("32 entries");
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
    // an editor that cannot be started: sh reports 127, the UI stays open
    let mut spec = Spec::new(&[], &home, &root);
    spec = spec.env("EDITOR", "/no/such/editor");
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    s.send(CTRL_E);
    std::thread::sleep(Duration::from_millis(300));
    s.wait_for("32 entries");
    assert!(s.is_running());
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

// ---- User Story 5 ----

#[test]
fn question_mark_shows_every_key_and_any_key_closes_it() {
    let (_t, home, root) = kb_home("search");
    let mut s = Session::spawn(&Spec::new(&[], &home, &root));
    s.wait_for("Enter pick");
    s.send(b"?");
    s.wait_for("any key closes");
    let screen = s.screen();
    for label in [
        "Enter",
        "Ctrl-Y",
        "Ctrl-E",
        "Ctrl-N",
        "Ctrl-Home",
        "Tab",
        "Shift-Tab",
        "Shift-PgDn",
        "Alt-P",
        "Ctrl-W",
        "Ctrl-U",
        "Ctrl-L",
        "F1",
        "Esc",
        "Ctrl-C",
        "Backspace",
    ] {
        assert!(
            screen.contains(label),
            "{label} is missing from the help:\n{screen}"
        );
    }
    s.send(b"z");
    s.wait_until("closed", |t| !t.contains("any key closes"));
    assert!(!s.screen().contains("› z"), "the closing key was not typed");
    s.type_text("fo");
    s.send(F1);
    s.wait_for("any key closes");
    s.send(ESC);
    s.wait_until("closed", |t| !t.contains("any key closes"));
    assert!(s.is_running(), "Esc only closed the overlay");
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

// ---- narrow terminals and resizing ----

#[test]
fn alt_p_zooms_the_preview_and_resizing_keeps_the_state() {
    let (_t, home, root) = kb_home("search");
    let mut spec = Spec::new(&[], &home, &root);
    spec.size = (80, 24);
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    assert!(
        s.screen().contains("results") && !s.screen().contains("preview"),
        "narrow: list only"
    );
    s.send(DOWN);
    s.send(DOWN);
    s.send(ALT_P);
    s.wait_until("zoomed", |t| {
        t.contains("preview") && !t.contains("results")
    });
    s.wait_for("Alt-P back");
    s.resize(50, 20);
    s.wait_for("Terminal too small");
    s.resize(80, 24);
    s.wait_until("back", |t| {
        t.contains("preview") && t.contains("Alt-P back")
    });
    s.send(ESC);
    s.wait_until("list again", |t| t.contains("results"));
    s.resize(120, 40);
    s.wait_until("wide", |t| t.contains("results") && t.contains("preview"));
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

// ---- mouse ----

/// An SGR mouse report for the wheel at (col, row), 1-based.
fn wheel_down(col: u16, row: u16) -> Vec<u8> {
    format!("\x1b[<65;{col};{row}M").into_bytes()
}

#[test]
fn the_wheel_is_ignored_by_default_and_works_with_mouse_true() {
    let (_t, home, root) = kb_home("search");
    let mut s = Session::spawn(&Spec::new(&[], &home, &root));
    s.wait_for("Enter pick");
    assert!(
        !s.raw_output().contains("\x1b[?1000h"),
        "mouse capture is off by default"
    );
    let first_preview = s.rows()[4].clone();
    s.send(&wheel_down(10, 10));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(s.rows()[4], first_preview, "the wheel did nothing");
    s.send(CTRL_C);
    s.wait_exit();

    write_config(&home, "[tui]\nmouse = true\n");
    let mut s = Session::spawn(&Spec::new(&[], &home, &root));
    s.wait_for("Enter pick");
    assert!(s.raw_output().contains("\x1b[?1000h"));
    let before = s.rows()[4].clone();
    assert_eq!(before, first_preview);
    s.send(&wheel_down(10, 10));
    s.wait_until("the selection moved", |t| {
        t.lines().nth(4).is_some_and(|l| l != before)
    });
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

// ---- NO_COLOR and themes ----

#[test]
fn no_color_uses_no_colors_at_all() {
    let (_t, home, root) = kb_home("search");
    let spec = Spec::new(&[], &home, &root).env("NO_COLOR", "1");
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    assert!(s.has_no_colors(), "some cell has a color:\n{}", s.screen());
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
    // an empty NO_COLOR does not count
    let spec = Spec::new(&[], &home, &root).env("NO_COLOR", "");
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    assert!(!s.has_no_colors());
    s.send(CTRL_C);
    s.wait_exit();
}

#[test]
fn the_theme_in_the_config_picks_the_selection_background() {
    use vt100::Color::Idx;
    let (_t, home, root) = kb_home("search");
    for (theme, bg) in [("dark", 237u8), ("light", 254)] {
        write_config(&home, &format!("theme = \"{theme}\"\n"));
        let mut s = Session::spawn(&Spec::new(&[], &home, &root));
        s.wait_for("Enter pick");
        assert_eq!(s.bg_at(4, 3), Idx(bg), "{theme}");
        s.send(CTRL_C);
        s.wait_exit();
    }
    // auto follows COLORFGBG
    write_config(&home, "theme = \"auto\"\n");
    let spec = Spec::new(&[], &home, &root).env("COLORFGBG", "0;15");
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    assert_eq!(s.bg_at(4, 3), Idx(254));
    s.send(CTRL_C);
    s.wait_exit();
}

// ---- SC-003: the terminal is back after every way out ----

#[derive(Clone, Copy, Debug)]
enum Way {
    Enter,
    Esc,
    CtrlC,
    Sigterm,
    Sighup,
    Panic,
}

fn kb_pid(shell_pid: u32) -> Option<String> {
    let out = std::process::Command::new("pgrep")
        .args(["-P", &shell_pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8(out.stdout)
        .ok()?
        .lines()
        .next()
        .map(str::to_string)
}

/// One run: start, leave by `way`, and check the exit code and that the terminal is restored.
fn one_run(way: Way, n: usize) -> Result<(), String> {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("out");
    let mut spec = Spec::new(&[], &home, &root);
    spec.out = Some(out.clone());
    spec.after = Some("stty -a".into());
    if matches!(way, Way::Panic) {
        spec = spec.env("KB_TUI_PANIC_AFTER_FIRST_FRAME", "1");
    }
    let mut s = Session::spawn(&spec);
    let want = match way {
        Way::Enter => {
            s.wait_for("Enter pick");
            s.send(ENTER);
            0
        }
        Way::Esc => {
            s.wait_for("Enter pick");
            s.send(ESC);
            130
        }
        Way::CtrlC => {
            s.wait_for("Enter pick");
            s.send(CTRL_C);
            130
        }
        Way::Sigterm | Way::Sighup => {
            s.wait_for("Enter pick");
            let shell = s.pid().ok_or("no pid")?;
            let pid = kb_pid(shell).ok_or("no kb process")?;
            let sig = if matches!(way, Way::Sigterm) {
                "TERM"
            } else {
                "HUP"
            };
            let _ = std::process::Command::new("kill")
                .arg(format!("-{sig}"))
                .arg(&pid)
                .status();
            if matches!(way, Way::Sigterm) {
                143
            } else {
                129
            }
        }
        Way::Panic => 2,
    };
    s.wait_until("the shell reported", |t| {
        t.contains("exit=") && t.contains("icanon")
    });
    let screen = s.screen();
    if !screen.contains(&format!("exit={want}")) {
        return Err(format!("run {n} ({way:?}): wanted exit={want}\n{screen}"));
    }
    if screen.contains("-icanon") || screen.contains("-echo ") {
        return Err(format!(
            "run {n} ({way:?}): the terminal is still raw\n{screen}"
        ));
    }
    if s.alternate_screen() || s.cursor_hidden() {
        return Err(format!("run {n} ({way:?}): the screen was not restored"));
    }
    if matches!(way, Way::Panic) && !screen.contains("KB_TUI_PANIC_AFTER_FIRST_FRAME") {
        return Err(format!(
            "run {n} ({way:?}): the panic message did not reach the normal screen\n{screen}"
        ));
    }
    Ok(())
}

#[test]
fn the_terminal_is_restored_after_a_hundred_runs_of_every_way_out() {
    // 5 ways in parallel, 20 runs each: 100 runs
    let mut ways = vec![Way::Enter, Way::Esc, Way::CtrlC, Way::Sigterm];
    if cfg!(debug_assertions) {
        // the panic switch exists only in debug builds
        ways.push(Way::Panic);
    }
    let failures: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = ways
            .iter()
            .map(|&way| {
                scope.spawn(move || {
                    (0..20)
                        .filter_map(|n| one_run(way, n).err())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    assert!(
        failures.is_empty(),
        "{} of 100 runs failed:\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn sighup_and_a_signal_during_the_editor_also_restore_the_terminal() {
    one_run(Way::Sighup, 0).unwrap();
    let (t, home, root) = kb_home("search");
    let started = t.path().join("started");
    let editor = script(
        t.path(),
        "slow",
        &format!("touch {}\nsleep 30", started.display()),
    );
    let mut spec = Spec::new(&[], &home, &root).env("EDITOR", editor.to_str().unwrap());
    spec.after = Some("stty -a".into());
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    s.send(CTRL_E);
    s.wait_until("the editor started", |_| started.exists());
    let shell = s.pid().unwrap();
    let kb = kb_pid(shell).expect("kb is running");
    // the editor is a child of kb; ending kb with SIGTERM must still leave a usable terminal
    let _ = std::process::Command::new("kill")
        .args(["-TERM", &kb])
        .status();
    s.wait_until("the shell reported", |t| {
        t.contains("exit=") && t.contains("icanon")
    });
    let screen = s.screen();
    assert!(
        !screen.contains("-icanon") && !s.alternate_screen(),
        "{screen}"
    );
    if let Some(editor_pid) = std::process::Command::new("pgrep")
        .args(["-f", "sleep 30"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
    {
        for pid in editor_pid.lines() {
            let _ = std::process::Command::new("kill").arg(pid).status();
        }
    }
}

// ---- command-line edges ----

#[test]
fn stdin_may_be_a_pipe_the_ui_still_reads_the_terminal() {
    let (t, home, root) = kb_home("search");
    let out = t.path().join("out");
    let mut spec = Spec::new(&[], &home, &root);
    spec.out = Some(out.clone());
    spec.stdin_text = Some("ignored".into());
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    s.type_text("5xx auth");
    s.wait_for("5xx by authorizer");
    s.send(ENTER);
    assert_eq!(s.wait_exit(), 0);
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        expected(&root, "sumo/5xx-by-authorizer", PrintMode::Payload)
    );
}

#[test]
fn filters_given_as_arguments_become_chips() {
    let (_t, home, root) = kb_home("search");
    let mut s = Session::spawn(&Spec::new(
        &["type:cw", "latency", "-tag:stage"],
        &home,
        &root,
    ));
    s.wait_for("Enter pick");
    let chips = s.rows()[2].clone();
    assert!(chips.contains("-tag:stage ×"), "{chips}");
    assert!(
        s.rows()[1].contains("latency") && !s.rows()[1].contains("type:cw"),
        "{}",
        s.rows()[1]
    );
    // the cw chip is the active one and the list holds only cw entries
    let list: String = s.rows()[4..30].join("\n");
    assert!(
        list.contains("cw ") && !list.contains("│ sumo") && !list.contains("│ sql"),
        "{list}"
    );
    s.send(TAB);
    s.wait_until("one type", |t| {
        t.lines().nth(2).is_some_and(|l| l.contains("cmd"))
    });
    assert!(!s.rows()[1].contains("type:"), "Tab replaced the type chip");
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn a_tag_chip_is_visible_at_eighty_columns() {
    let (_t, home, root) = kb_home("search");
    let mut spec = Spec::new(&[], &home, &root);
    spec.size = (80, 24);
    let mut s = Session::spawn(&spec);
    s.wait_for("Enter pick");
    s.type_text("tag:prod ");
    s.wait_for("tag:prod ×");
    assert!(s.rows()[1].contains("/ 32"), "the token left the text line");
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn tui_enter_primary_opens_link_and_copies_cmd() {
    let (_t, home, root) = kb_home("search");
    write_config(&home, "[tui]\nenter = \"primary\"\n");

    // Test 1: link entry opens via opener
    let mut spec1 = Spec::new(&["type:link"], &home, &root).unset("KB_TUI_ENTER");
    spec1.allow_clipboard = true;
    let mut s1 = Session::spawn(&spec1);
    s1.wait_for("Enter pick");
    s1.send(ENTER);
    assert_eq!(s1.wait_exit(), 0);
    let calls1 = s1.guarded_calls();
    assert!(
        calls1
            .iter()
            .any(|c| c.contains("open") && c.contains("http")),
        "expected browser open call in guarded calls: {calls1:?}"
    );

    // Test 2: cmd entry copies payload and never runs command
    let mut spec2 = Spec::new(&["type:cmd"], &home, &root).unset("KB_TUI_ENTER");
    spec2.allow_clipboard = true;
    let mut s2 = Session::spawn(&spec2);
    s2.wait_for("Enter pick");
    s2.send(ENTER);
    assert_eq!(s2.wait_exit(), 0);
    let calls2 = s2.guarded_calls();
    assert!(
        !calls2
            .iter()
            .any(|c| c.contains("open") && c.contains("http")),
        "cmd should never open URL: {calls2:?}"
    );
}
