//! Stored screens: every entry type at three sizes in three themes, plus the special states.

mod support;

use support::snapshot::*;

/// One entry of each type plus the CJK and emoji one, all from the `tui` fixture KB.
const ENTRIES: [(&str, &str); 8] = [
    ("sql", "sql/multi-dialect"),
    ("sumo", "sumo/long-query"),
    ("cw", "cw/with-fields"),
    ("cmd", "cmd/deploy-with-secret"),
    ("prompt", "prompt/summarize"),
    ("link", "link/long-url"),
    ("code", "code/with-symbol"),
    ("cjk", "prompt/cjk-and-emoji"),
];
const SIZES: [(u16, u16); 3] = [(80, 24), (120, 40), (200, 60)];
const THEMES: [&str; 3] = ["dark", "light", "nocolor"];

#[test]
fn every_entry_type_at_every_size_in_every_theme() {
    let (_tmp, root) = fixture_copy("tui");
    let mut cases = 0;
    for (label, id) in ENTRIES {
        for (w, h) in SIZES {
            for theme_name in THEMES {
                let theme = theme_of(theme_name);
                let mut state = state_for(&root, "", Some(id), (w, h), &theme);
                if w < 100 {
                    // narrow terminals show the preview full screen (Alt-P)
                    state.layout_mode = kb_cli::tui::app::LayoutMode::FullPreview;
                }
                let shot = render(&state, &theme);
                // Nothing may be wider than the screen, whatever the characters are.
                for line in shot.lines().take(usize::from(h)) {
                    assert!(
                        unicode_width::UnicodeWidthStr::width(line) <= usize::from(w),
                        "{label} {w}x{h}: {line:?}"
                    );
                }
                if label == "cjk" {
                    assert!(
                        shot.contains('…'),
                        "the long CJK title is cut with an ellipsis at {w}x{h}"
                    );
                }
                assert_snapshot(&format!("{label}.{w}x{h}.{theme_name}"), &shot);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 8 * 3 * 3, "a case was dropped");
}

#[test]
fn the_headline_search_screen() {
    let (_tmp, root) = fixture_copy("search");
    let theme = theme_of("dark");
    let state = state_for(&root, "5xx auth", None, (120, 40), &theme);
    let shot = render(&state, &theme);
    assert!(shot.contains("5xx by authorizer"));
    assert_snapshot("search-5xx-auth.120x40.dark", &shot);
}

#[test]
fn the_header_with_config_problems() {
    let (_tmp, root) = fixture_copy("search");
    let theme = theme_of("dark");
    let mut state = state_for(&root, "", None, (120, 40), &theme);
    state.options.config_problems = 2;
    let shot = render(&state, &theme);
    assert!(
        shot.contains("⚠ config: 2 problems (run kb doctor)"),
        "{shot}"
    );
    assert_snapshot("config-warning.120x40.dark", &shot);
}

#[test]
fn a_long_payload_scrolled_to_the_middle() {
    let (_tmp, root) = fixture_copy("tui");
    let theme = theme_of("dark");
    let mut state = state_for(&root, "", Some("sumo/long-query"), (120, 40), &theme);
    for _ in 0..3 {
        state.update(
            kb_cli::tui::app::Action::Key(ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::PageDown,
                ratatui::crossterm::event::KeyModifiers::SHIFT,
            )),
            std::time::Instant::now(),
        );
    }
    assert!(state.preview_scroll > 0);
    let shot = render(&state, &theme);
    assert!(shot.contains("↑ more"), "{shot}");
    assert_snapshot("sumo-scrolled.120x40.dark", &shot);
}

#[test]
fn empty_kb_and_no_results() {
    let tmp = tempfile::tempdir().unwrap();
    let theme = theme_of("dark");
    let state = state_for(tmp.path(), "", None, (120, 40), &theme);
    let shot = render(&state, &theme);
    assert!(shot.contains("No entries yet"));
    assert_snapshot("empty-kb.120x40.dark", &shot);
    let (_t2, root) = fixture_copy("search");
    let state = state_for(&root, "zzzznotaword", None, (120, 40), &theme);
    let shot = render(&state, &theme);
    assert!(shot.contains("No matches") && shot.contains("Search tips"));
    assert_snapshot("no-results.120x40.dark", &shot);
}

#[test]
fn the_chips_row() {
    let (_tmp, root) = fixture_copy("search");
    let theme = theme_of("dark");
    for (name, typed, size) in [
        ("chips-counts", "latency", (120, 40)),
        ("chips-type", "type:cw latency", (120, 40)),
        ("chips-tags", "tag:prod -tag:stage latency", (120, 40)),
        (
            "chips-overflow",
            "tag:aaaaaaaaaa tag:bbbbbbbbbb tag:cccccccccc tag:dddddddddd tag:eeeeeeeeee ",
            (80, 24),
        ),
    ] {
        let state = state_for(&root, typed, None, size, &theme);
        let shot = render(&state, &theme);
        assert_snapshot(&format!("{name}.{}x{}.dark", size.0, size.1), &shot);
        let chips_row = shot.lines().nth(2).unwrap();
        assert!(chips_row.contains("all "), "{chips_row}");
        match name {
            "chips-type" => assert!(
                !chips_row.contains("×"),
                "type chips have no ×: {chips_row}"
            ),
            "chips-tags" => assert!(
                chips_row.contains("tag:prod ×") && chips_row.contains("-tag:stage ×"),
                "{chips_row}"
            ),
            "chips-overflow" => assert!(unicode_width::UnicodeWidthStr::width(chips_row) <= 80),
            _ => {}
        }
    }
    // types with no results are dimmed, the active one is highlighted
    let state = state_for(&root, "type:cw latency", None, (120, 40), &theme);
    let shot = render(&state, &theme);
    let (text, styles) = shot.split_once("--- styles ---").unwrap();
    let row = text.lines().nth(2).unwrap();
    let col = row.find("cw ").unwrap();
    assert!(
        styles
            .lines()
            .any(|l| l.starts_with("2:") && l.contains("bg=24")),
        "the active chip is highlighted"
    );
    let sql_col = row.find("sql ").unwrap();
    assert!(
        styles
            .lines()
            .any(|l| l.starts_with(&format!("2:{}", sql_col - 1)) && l.contains("fg=240")),
        "an empty type is dimmed: {styles}"
    );
    let _ = col;
}

#[test]
fn the_help_overlay() {
    let (_tmp, root) = fixture_copy("search");
    for size in [(80, 24), (120, 40)] {
        for theme_name in THEMES {
            let theme = theme_of(theme_name);
            let mut state = state_for(&root, "", None, size, &theme);
            state.overlay = kb_cli::tui::app::Overlay::Help;
            let shot = render(&state, &theme);
            for label in [
                "Ctrl-E",
                "Alt-P",
                "Shift-PgUp",
                "Ctrl-Home",
                "F1",
                "Ctrl-W",
                "any key closes",
                "Shift-PgUp/PgDn",
            ] {
                assert!(shot.contains(label), "{label} missing at {size:?}:\n{shot}");
            }
            assert_snapshot(&format!("help.{}x{}.{theme_name}", size.0, size.1), &shot);
        }
    }
}

#[test]
fn the_key_bar_in_each_state() {
    let (_tmp, root) = fixture_copy("search");
    let theme = theme_of("dark");
    let bar = |state: &kb_cli::tui::app::AppState| {
        render(state, &theme)
            .lines()
            .nth(39)
            .unwrap()
            .trim()
            .to_string()
    };
    let results = state_for(&root, "", None, (120, 40), &theme);
    assert_eq!(
        bar(&results),
        "↑↓ select · Enter pick · Tab type · ^Y copy · ^E edit · ? help · Esc quit"
    );
    let typed = state_for(&root, "latency", None, (120, 40), &theme);
    assert!(bar(&typed).ends_with("Esc clear"), "{}", bar(&typed));
    let none = state_for(&root, "zzzznotaword", None, (120, 40), &theme);
    assert_eq!(bar(&none), "Esc clear · ? help");
    let empty = tempfile::tempdir().unwrap();
    let e = state_for(empty.path(), "", None, (120, 40), &theme);
    assert_eq!(bar(&e), "Esc quit · ? help");
    let mut long = state_for(&root, "", Some("sql/active-connections"), (120, 40), &theme);
    long.preview.as_mut().unwrap().payload = (0..80)
        .map(|i| {
            vec![kb_cli::tui::highlight::Span::new(
                format!("line {i}"),
                kb_cli::tui::theme::Class::Plain,
            )]
        })
        .collect();
    assert!(bar(&long).contains("S-↑↓ scroll"), "{}", bar(&long));
}

#[test]
fn narrow_full_preview_and_too_small_screens() {
    let (_tmp, root) = fixture_copy("tui");
    let theme = theme_of("dark");
    for (name, size, full) in [
        ("narrow-list", (80, 24), false),
        ("smallest-list", (60, 15), false),
        ("smallest-preview", (60, 15), true),
        ("too-small-width", (59, 15), false),
        ("too-small-height", (80, 14), false),
    ] {
        let mut state = state_for(&root, "", Some("cmd/deploy-with-secret"), size, &theme);
        if full {
            state.layout_mode = kb_cli::tui::app::LayoutMode::FullPreview;
        }
        let shot = render(&state, &theme);
        match name {
            "narrow-list" => assert!(shot.contains("results") && !shot.contains("preview")),
            "smallest-preview" => assert!(shot.contains("preview") && !shot.contains("results")),
            n if n.starts_with("too-small") => {
                assert!(shot.contains("Terminal too small"));
                assert_eq!(
                    shot.lines()
                        .take(usize::from(size.1))
                        .filter(|l| !l.trim().is_empty())
                        .count(),
                    1
                );
            }
            _ => {}
        }
        assert_snapshot(&format!("{name}.{}x{}.dark", size.0, size.1), &shot);
    }
}
