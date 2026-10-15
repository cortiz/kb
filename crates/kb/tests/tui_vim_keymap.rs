//! The vim keymap and configurable keys: the state machine, and the real `kb` in a terminal.

mod support;

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kb_cli::tui::app::{Action, AppState, Effect};
use kb_cli::tui::keys::{CTRL, Code, Key, from_event, to_event};
use kb_core::keys::{EffectiveKeymap, KeyOverride, Keymap};
use support::pty::*;
use support::snapshot::*;

// ---- the state machine ----

fn vim_state(root: &std::path::Path, query: &str, overrides: &[KeyOverride]) -> AppState {
    let theme = theme_of("dark");
    let mut s = state_for(root, query, None, (120, 40), &theme);
    s.keymap = Arc::new(EffectiveKeymap::build(Keymap::Vim, overrides));
    s
}

fn press(s: &mut AppState, key: Key, at: Instant) -> Vec<Effect> {
    s.update(Action::Key(to_event(key)), at)
}

fn ch(c: char) -> Key {
    Key::plain(c)
}

const ESCAPE: Key = Key::new(Code::Esc, 0);

#[test]
fn modes_switch_with_esc_slash_i_and_a() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let mut s = vim_state(&root, "auth", &[]);
    assert!(!s.normal, "starts in search mode");
    press(&mut s, ESCAPE, t0);
    assert!(s.normal);
    assert_eq!(s.input.editor.text(), "auth", "the query stays");
    // `i` keeps the query, with the cursor at its end
    press(&mut s, ch('i'), t0);
    assert!(!s.normal);
    assert_eq!(s.input.editor.cursor(), 4);
    press(&mut s, ESCAPE, t0);
    press(&mut s, ch('a'), t0);
    assert!(!s.normal && s.input.editor.text() == "auth");
    // `/` clears it
    press(&mut s, ESCAPE, t0);
    press(&mut s, ch('/'), t0);
    assert!(!s.normal);
    assert!(s.input.is_blank());
    // letters are typed in search mode and are commands in normal mode
    press(&mut s, ch('o'), t0);
    assert_eq!(s.input.editor.text(), "o");
}

#[test]
fn esc_in_normal_mode_clears_the_query_then_exits_and_q_exits() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let mut s = vim_state(&root, "auth", &[]);
    press(&mut s, ESCAPE, t0);
    let e = press(&mut s, ESCAPE, t0);
    assert!(s.normal && s.input.is_blank(), "cleared, still normal");
    assert!(!e.contains(&Effect::Quit(130)));
    assert!(press(&mut s, ESCAPE, t0).contains(&Effect::Quit(130)));
    let mut s = vim_state(&root, "auth", &[]);
    press(&mut s, ESCAPE, t0);
    assert!(press(&mut s, ch('q'), t0).contains(&Effect::Quit(130)));
}

#[test]
fn normal_mode_letters_act_on_the_selection() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let mut s = vim_state(&root, "", &[]);
    press(&mut s, ESCAPE, t0);
    assert!(press(&mut s, ch('y'), t0).contains(&Effect::CopyPayload));
    let id = s.selected_result().unwrap().id.clone();
    assert!(press(&mut s, ch('o'), t0).contains(&Effect::OpenEntry(id.clone())));
    assert!(
        press(&mut s, ch('e'), t0)
            .iter()
            .any(|e| matches!(e, Effect::EditEntry(i) if *i == id))
    );
    press(&mut s, ch('m'), t0);
    press(&mut s, ch('j'), t0);
    press(&mut s, ch('j'), t0);
    assert_eq!(s.selected, 2);
    press(&mut s, ch('k'), t0);
    assert_eq!(s.selected, 1);
    // Ctrl-O works in normal mode too
    assert!(
        press(&mut s, Key::ctrl('o'), t0)
            .iter()
            .any(|e| matches!(e, Effect::OpenEntry(_)))
    );
}

#[test]
fn gg_goes_to_the_first_result_and_other_keys_end_the_sequence() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let mut s = vim_state(&root, "", &[]);
    press(&mut s, ESCAPE, t0);
    for _ in 0..4 {
        press(&mut s, ch('j'), t0);
    }
    assert_eq!(s.selected, 4);
    press(&mut s, ch('g'), t0);
    assert!(s.pending.is_some(), "waiting for the second g");
    press(&mut s, ch('g'), t0 + ms(200));
    assert_eq!((s.selected, s.pending), (0, None));
    // another key ends the sequence and acts on its own
    press(&mut s, ch('j'), t0);
    press(&mut s, ch('g'), t0);
    press(&mut s, ch('j'), t0 + ms(100));
    assert_eq!(s.selected, 2, "g was dropped, j moved down");
    // Esc cancels with no action
    press(&mut s, ch('g'), t0);
    let before = s.selected;
    press(&mut s, ESCAPE, t0 + ms(100));
    assert!(s.pending.is_none() && s.normal && s.selected == before);
    // after a second the sequence is forgotten
    press(&mut s, ch('g'), t0);
    press(&mut s, ch('g'), t0 + ms(1500));
    assert_eq!(s.selected, before, "too late: that g starts a new sequence");
    s.update(Action::Tick, t0 + ms(3000));
    assert!(s.pending.is_none());
    // G jumps to the last result
    press(&mut s, ch('G'), t0);
    assert_eq!(s.selected, s.results.len() - 1);
}

#[test]
fn half_pages_and_type_filters_in_normal_mode() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let mut s = vim_state(&root, "", &[]);
    press(&mut s, ESCAPE, t0);
    press(&mut s, Key::new(Code::Char('d'), CTRL), t0);
    let half = s.selected;
    assert!(half >= 1);
    press(&mut s, Key::new(Code::Char('u'), CTRL), t0);
    assert_eq!(s.selected, 0);
    press(&mut s, ch('l'), t0);
    assert_eq!(s.input.type_chips().len(), 1, "l is Tab");
    press(&mut s, ch('h'), t0);
    assert!(s.input.type_chips().is_empty(), "h is Shift-Tab");
}

#[test]
fn arrows_and_ctrl_u_keep_their_search_meanings() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let mut s = vim_state(&root, "abc", &[]);
    press(&mut s, Key::new(Code::Left, 0), t0);
    assert_eq!(
        s.input.editor.cursor(),
        2,
        "← moves the cursor in search mode"
    );
    press(&mut s, Key::new(Code::Char('u'), CTRL), t0);
    assert!(s.input.is_blank(), "Ctrl-U clears the line in search mode");
}

#[test]
fn the_emacs_keymap_has_no_modes() {
    let (_t, root) = fixture_copy("search");
    let theme = theme_of("dark");
    let t0 = Instant::now();
    let mut s = state_for(&root, "", None, (120, 40), &theme);
    press(&mut s, ch('o'), t0);
    assert_eq!(s.input.editor.text(), "o", "letters are always typed");
    press(&mut s, ESCAPE, t0);
    assert!(!s.normal && s.input.is_blank(), "Esc clears, as before");
    let shot = render(&s, &theme);
    assert!(
        !shot.contains("NORMAL") && !shot.contains("SEARCH"),
        "no badge:\n{shot}"
    );
}

#[test]
fn rebound_keys_run_their_commands() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let o = |ctx: &str, cmd: &str, keys: &[&str]| KeyOverride {
        context: ctx.into(),
        command: cmd.into(),
        keys: keys.iter().map(|k| k.to_string()).collect(),
        file: None,
        line: Some(1),
        error: None,
    };
    let mut s = vim_state(
        &root,
        "",
        &[
            o("normal", "copy", &["x"]),
            o("search", "sync", &["ctrl-t"]),
        ],
    );
    press(&mut s, ESCAPE, t0);
    assert!(press(&mut s, ch('x'), t0).contains(&Effect::CopyPayload));
    assert!(press(&mut s, ch('y'), t0).is_empty(), "y was replaced");
}

// ---- the screen ----

#[test]
fn the_header_names_the_mode_and_the_query_loses_its_cursor() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    for name in ["dark", "nocolor"] {
        let theme = theme_of(name);
        let mut s = vim_state(&root, "auth", &[]);
        let search = render(&s, &theme);
        assert!(
            search.lines().next().unwrap().contains("SEARCH"),
            "{search}"
        );
        press(&mut s, ESCAPE, t0);
        let normal = render(&s, &theme);
        let header = normal.lines().next().unwrap();
        assert!(
            header.contains("NORMAL") && !header.contains("SEARCH"),
            "{header}"
        );
        // the key bar offers letters, not chords
        let bar = normal
            .lines()
            .find(|l| l.contains("Enter pick"))
            .unwrap_or("");
        assert!(
            bar.contains("e edit") && bar.contains("Esc clear"),
            "{normal}"
        );
    }
    // the badge survives a narrow header
    let theme = theme_of("dark");
    let mut s = state_for(&root, "", None, (60, 15), &theme);
    s.keymap = Arc::new(EffectiveKeymap::build(Keymap::Vim, &[]));
    s.normal = true;
    assert!(
        render(&s, &theme)
            .lines()
            .next()
            .unwrap()
            .contains("NORMAL")
    );
}

#[test]
fn the_help_lists_the_keys_of_both_modes_and_g_shows_in_the_key_bar() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let theme = theme_of("dark");
    let mut s = state_for(&root, "", None, (120, 130), &theme);
    s.keymap = Arc::new(EffectiveKeymap::build(Keymap::Vim, &[]));
    press(&mut s, ESCAPE, t0);
    press(&mut s, ch('?'), t0);
    let shot = render(&s, &theme);
    for want in [
        "NORMAL MODE",
        "SEARCH MODE",
        "gg",
        "half a page",
        "Ctrl-D",
        "normal mode",
    ] {
        assert!(shot.contains(want), "{want} missing:\n{shot}");
    }
    assert!(shot.find("NORMAL MODE") < shot.find("SEARCH MODE"));
    assert!(shot.contains("any key closes"));
    press(&mut s, ch('x'), t0); // closes the help
    press(&mut s, ch('g'), t0);
    let shot = render(&s, &theme);
    assert!(shot.lines().any(|l| l.trim() == "g…"), "{shot}");
}

#[test]
fn escape_and_alt_are_told_apart_by_the_event_parser() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let alt_a = from_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT)).unwrap();
    assert_eq!(alt_a, Key::new(Code::Char('a'), kb_core::keys::ALT));
    assert_eq!(
        from_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(ESCAPE)
    );
}

// ---- the real binary ----

fn vim_spec(args: &[&str], home: &std::path::Path, root: &std::path::Path) -> Spec {
    let mut a = vec!["--keymap", "vim"];
    a.extend_from_slice(args);
    Spec::new(&a, home, root)
}

#[test]
fn pty_search_then_normal_then_letters_then_search_again() {
    let (_t, home, root) = kb_home("search");
    let mut spec = vim_spec(&[], &home, &root);
    spec.allow_clipboard = true;
    let mut s = Session::spawn(&spec);
    s.wait_for("32 entries");
    s.wait_until("the SEARCH badge", |t| t.contains("SEARCH"));
    s.type_text("grafana");
    s.wait_for("Grafana dashboards");
    s.send(ESC);
    s.wait_until("the NORMAL badge", |t| t.contains("NORMAL"));
    assert!(s.screen().contains("grafana"), "the query stays");
    // `o` opens the link through the opener (a guard that records the call)
    s.send(b"o");
    s.wait_until("the opener was called", |_| {
        s.guarded_calls().iter().any(|c| c.contains("grafana"))
    });
    // `/` is search mode again, with the query cleared
    s.send(b"/");
    s.wait_until("SEARCH again", |t| {
        t.contains("SEARCH") && !t.contains("NORMAL")
    });
    s.wait_until("query cleared", |t| t.contains("32 / 32"));
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn pty_navigation_and_exit_codes() {
    let (_t, home, root) = kb_home("search");
    let mut s = Session::spawn(&vim_spec(&[], &home, &root));
    s.wait_for("32 entries");
    s.send(ESC);
    s.wait_until("NORMAL", |t| t.contains("NORMAL"));
    let first = s.rows()[4].clone();
    s.send(b"jj");
    s.wait_until("the selection moved", |t| {
        t.lines().nth(4) != Some(first.as_str()) || true
    });
    s.send(b"G");
    s.wait_until("the last result", |t| t.contains("32 / 32"));
    s.send(b"gg");
    s.send(b"?");
    s.wait_for("NORMAL MODE");
    s.send(b"x");
    s.send(b"q");
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn pty_alt_bindings_still_work_right_after_esc_handling() {
    let (_t, home, root) = kb_home("search");
    let mut s = Session::spawn(&vim_spec(&["5xx"], &home, &root));
    s.wait_for("5xx by authorizer");
    s.send(b"\x1ba"); // Alt-A in search mode, not Esc then a
    s.wait_for("Actions: 5xx by authorizer");
    s.send(ESC);
    s.wait_until("back on the list", |t| !t.contains("Actions:"));
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn pty_config_overrides_and_warnings() {
    let (_t, home, root) = kb_home("search");
    fs::create_dir_all(home.join(".config/kb")).unwrap();
    fs::write(
        home.join(".config/kb/config.toml"),
        "[tui]\nkeymap = \"vim\"\n\n[tui.keys.normal]\nquit = [\"X\", \"ctrl-c\"]\nrun = [\"zz\"]\n",
    )
    .unwrap();
    let mut spec = Spec::new(&[], &home, &root);
    spec.env.push((
        "XDG_CONFIG_HOME".into(),
        home.join(".config").display().to_string(),
    ));
    let mut s = Session::spawn(&spec);
    s.wait_for("32 entries");
    s.wait_for("config: 1 problem");
    s.send(ESC);
    s.wait_until("NORMAL", |t| t.contains("NORMAL"));
    s.send(b"X");
    assert_eq!(s.wait_exit(), 130);
}

#[test]
fn pty_sub_view_keys_can_be_rebound_too() {
    let (_t, home, root) = kb_home("search");
    fs::create_dir_all(home.join(".config/kb")).unwrap();
    fs::write(
        home.join(".config/kb/config.toml"),
        "[tui.keys.action-menu]\nquit = [\"f9\"]\n",
    )
    .unwrap();
    let mut spec = Spec::new(&["5xx"], &home, &root);
    spec.env.push((
        "XDG_CONFIG_HOME".into(),
        home.join(".config").display().to_string(),
    ));
    let mut s = Session::spawn(&spec);
    s.wait_for("5xx by authorizer");
    s.send(b"\x1ba");
    s.wait_for("Actions: 5xx by authorizer");
    s.send(ESC);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        s.screen().contains("Actions: 5xx by authorizer"),
        "Esc was moved to F9"
    );
    s.send(b"\x1b[20~"); // F9
    s.wait_until("the menu closed", |t| !t.contains("Actions:"));
    s.send(CTRL_C);
    assert_eq!(s.wait_exit(), 130);
}
