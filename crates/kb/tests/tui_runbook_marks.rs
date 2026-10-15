//! TUI tests for mark-entry and new-runbook-from-marks.

mod support;

use std::sync::Arc;
use std::time::Instant;

use kb_cli::tui::app::{Action, AppState, Effect, ListPrompt};
use kb_cli::tui::keys::{Code, Key, to_event};
use kb_cli::tui::view;
use kb_core::keys::{EffectiveKeymap, Keymap};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use support::snapshot::*;

fn vim_state(root: &std::path::Path, query: &str) -> AppState {
    let theme = theme_of("dark");
    let mut s = state_for(root, query, None, (120, 40), &theme);
    s.keymap = Arc::new(EffectiveKeymap::build(Keymap::Vim, &[]));
    s
}

fn press(s: &mut AppState, key: Key, at: Instant) -> Vec<Effect> {
    s.update(Action::Key(to_event(key)), at)
}

fn ch(c: char) -> Key {
    Key::plain(c)
}

const ESCAPE: Key = Key::new(Code::Esc, 0);
const ENTER: Key = Key::new(Code::Enter, 0);

#[test]
fn mark_entry_toggles_and_preserves_order() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let mut s = vim_state(&root, "");
    press(&mut s, ESCAPE, t0); // enter normal mode

    assert!(s.marks.is_empty());

    // Select first entry and mark it with 'v'
    let id1 = s.selected_result().unwrap().id.clone();
    press(&mut s, ch('v'), t0);
    assert_eq!(s.marks, vec![id1.clone()]);

    // Move down and mark second entry
    press(&mut s, ch('j'), t0);
    let id2 = s.selected_result().unwrap().id.clone();
    press(&mut s, ch('v'), t0);
    assert_eq!(s.marks, vec![id1.clone(), id2.clone()]);

    // Move back up and toggle first entry (unmark)
    press(&mut s, ch('k'), t0);
    press(&mut s, ch('v'), t0);
    assert_eq!(s.marks, vec![id2.clone()]);
}

#[test]
fn new_runbook_from_marks_flow() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let mut s = vim_state(&root, "");
    press(&mut s, ESCAPE, t0);

    // Try 'B' with no marks -> keybar message
    press(&mut s, ch('B'), t0);
    assert!(s.prompt.is_none());
    assert_eq!(
        s.message.as_ref().map(|m| m.text.as_str()),
        Some("mark entries first with Alt-M (or v)")
    );

    // Mark two entries
    let id1 = s.selected_result().unwrap().id.clone();
    press(&mut s, ch('v'), t0);
    press(&mut s, ch('j'), t0);
    let id2 = s.selected_result().unwrap().id.clone();
    press(&mut s, ch('v'), t0);

    // Press 'B' -> prompts for runbook title
    press(&mut s, ch('B'), t0);
    assert!(matches!(s.prompt, Some(ListPrompt::NewRunbook { .. })));
    assert_eq!(s.prompt.as_ref().unwrap().text(), "New runbook title: ▌");

    // Type title and press Enter
    for c in "Deploy service".chars() {
        press(&mut s, ch(c), t0);
    }
    let effects = press(&mut s, ENTER, t0);
    assert!(s.prompt.is_none());
    assert_eq!(
        effects,
        vec![Effect::CreateRunbookFromMarks {
            title: "Deploy service".to_string(),
            entries: vec![id1, id2],
        }]
    );
    assert!(s.marks.is_empty(), "marks taken by runbook creation");
}

#[test]
fn esc_in_normal_mode_with_empty_query_clears_marks() {
    let (_t, root) = fixture_copy("search");
    let t0 = Instant::now();
    let mut s = vim_state(&root, "");
    press(&mut s, ESCAPE, t0);

    // Mark an entry
    press(&mut s, ch('v'), t0);
    assert!(!s.marks.is_empty());
    assert!(s.input.is_blank());

    // Esc clears marks and does NOT quit
    let effects = press(&mut s, ESCAPE, t0);
    assert!(s.marks.is_empty());
    assert!(!effects.contains(&Effect::Quit(130)));

    // Next Esc quits
    let effects = press(&mut s, ESCAPE, t0);
    assert_eq!(effects, vec![Effect::Quit(130)]);
}

#[test]
fn mark_indicators_rendered_in_view() {
    let (_t, root) = fixture_copy("search");
    let theme = theme_of("dark");
    let mut s = vim_state(&root, "");
    let id = s.results[0].id.clone();
    s.marks.push(id);

    let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
    view::draw(&s, &theme, Rect::new(0, 0, 80, 24), &mut buf);

    let mut content = String::new();
    for y in 0..24 {
        for x in 0..80 {
            content.push_str(buf[(x, y)].symbol());
        }
        content.push('\n');
    }
    assert!(content.contains("✓1"), "buffer should contain ✓1 mark indicator");
}
