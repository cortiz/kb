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
        "↑↓ select · Enter pick · ^G run · ^E edit · Alt-A actions · ? help · Esc quit"
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

// ---- the variable form (screen 2) ----

mod form {
    use super::support::snapshot::*;
    use kb_cli::tui::app::AppState;
    use kb_cli::tui::form::{FormSpec, FormState, Then};
    use kb_cli::tui::keys::{ALT, CTRL, Code, Key};
    use kb_cli::tui::theme::Theme;
    use kb_core::search::Kb;

    fn open(
        root: &std::path::Path,
        id: &str,
        size: (u16, u16),
        theme: &Theme,
        env: &dyn Fn(&str) -> Option<String>,
        then: Then,
    ) -> AppState {
        let mut state = state_for(root, "", Some(id), size, theme);
        let kb = Kb::open(root).unwrap();
        let spec = FormSpec::load(&kb, id, env)
            .unwrap()
            .expect("an entry with variables");
        state.form = Some(Box::new(FormState::open(spec, then, None, None)));
        state
    }

    fn press(state: &mut AppState, code: Code, mods: u8) {
        state
            .form
            .as_mut()
            .unwrap()
            .handle_key(Key::new(code, mods));
    }

    fn type_text(state: &mut AppState, text: &str) {
        for c in text.chars() {
            press(state, Code::Char(c), 0);
        }
    }

    fn none(_: &str) -> Option<String> {
        None
    }

    const FIVE_XX: &str = "sumo/5xx-by-authorizer";

    #[test]
    fn wide_with_an_enum_a_number_and_a_live_preview() {
        let (_tmp, root) = fixture_copy("vars");
        let theme = theme_of("dark");
        let mut s = open(&root, FIVE_XX, (120, 40), &theme, &none, Then::Exit);
        press(&mut s, Code::Right, 0);
        press(&mut s, Code::Tab, 0);
        let shot = render(&s, &theme);
        assert!(
            shot.contains("field 2/2") && shot.contains("[prod]") && shot.contains("timeslice 15m"),
            "{shot}"
        );
        assert_snapshot("form-wide.120x40.dark", &shot);
        let light = theme_of("light");
        assert_snapshot("form-wide.120x40.light", &render(&s, &light));
        let plain = theme_of("nocolor");
        assert_snapshot("form-wide.120x40.nocolor", &render(&s, &plain));
    }

    #[test]
    fn narrow_puts_the_preview_under_the_fields() {
        let (_tmp, root) = fixture_copy("vars");
        let theme = theme_of("dark");
        let s = open(&root, FIVE_XX, (80, 24), &theme, &none, Then::Exit);
        let shot = render(&s, &theme);
        let rows: Vec<&str> = shot.lines().collect();
        let fields = rows.iter().position(|r| r.contains("minutes")).unwrap();
        let preview = rows.iter().position(|r| r.contains("Preview")).unwrap();
        assert!(fields < preview, "{shot}");
        assert_snapshot("form-narrow.80x24.dark", &shot);
    }

    #[test]
    fn an_invalid_field_shows_its_message_and_the_focus() {
        let (_tmp, root) = fixture_copy("vars");
        let theme = theme_of("dark");
        let mut s = open(&root, FIVE_XX, (200, 40), &theme, &none, Then::Exit);
        press(&mut s, Code::Right, 0);
        press(&mut s, Code::Tab, 0);
        press(&mut s, Code::Char('u'), CTRL);
        type_text(&mut s, "abc");
        press(&mut s, Code::Tab, 0);
        press(&mut s, Code::Enter, 0);
        let shot = render(&s, &theme);
        assert!(
            shot.contains("✗ expected a number like 30, -5 or 0.25"),
            "{shot}"
        );
        assert!(
            shot.contains("{{minutes}}"),
            "an invalid value stays a placeholder in the preview"
        );
        assert_snapshot("form-invalid.200x40.dark", &shot);
    }

    #[test]
    fn a_secret_from_the_environment_is_masked_everywhere() {
        let (_tmp, root) = fixture_copy("vars");
        let theme = theme_of("dark");
        let env = |n: &str| (n == "KB_TEST_TOKEN").then(|| "S3CR3T-MARKER".to_string());
        let s = open(&root, "cmd/curl-token", (120, 40), &theme, &env, Then::Exit);
        let shot = render(&s, &theme);
        assert!(shot.contains("•••• (from $KB_TEST_TOKEN)"), "{shot}");
        assert!(shot.contains("Bearer ••••"), "{shot}");
        assert!(!shot.contains("S3CR3T"));
        assert_snapshot("form-secret.120x40.dark", &shot);
    }

    #[test]
    fn markers_for_raw_and_undeclared_variables() {
        let (_tmp, root) = fixture_copy("vars");
        let theme = theme_of("dark");
        let s = open(
            &root,
            "cmd/escape-none",
            (120, 40),
            &theme,
            &none,
            Then::Exit,
        );
        assert!(render(&s, &theme).contains("raw"));
        let s = open(
            &root,
            "cmd/undeclared",
            (120, 40),
            &theme,
            &none,
            Then::Exit,
        );
        assert!(render(&s, &theme).contains("undeclared"));
    }

    #[test]
    fn a_quoted_value_is_shown_with_its_quotes() {
        let (_tmp, root) = fixture_copy("vars");
        let theme = theme_of("dark");
        let mut s = open(
            &root,
            "cmd/git-checkout",
            (120, 40),
            &theme,
            &none,
            Then::Exit,
        );
        type_text(&mut s, "feature/x; rm -rf ~");
        let shot = render(&s, &theme);
        assert!(
            shot.contains("git checkout 'feature/x; rm -rf ~'"),
            "{shot}"
        );
    }

    #[test]
    fn many_fields_scroll_and_the_preview_stays() {
        let (tmp, root) = fixture_copy("vars");
        let vars: String = (1..=9)
            .map(|i| format!("  v{i}: {{ kind: text, default: d{i} }}\n"))
            .collect();
        let payload: String = (1..=9).map(|i| format!("{{{{v{i}}}}} ")).collect();
        std::fs::write(
            root.join("cmd/many.md"),
            format!("---\nkb_format: 1\ntype: cmd\ntitle: Many\nvars:\n{vars}---\n\n```\necho {payload}\n```\n"),
        )
        .unwrap();
        let theme = theme_of("dark");
        let mut s = open(&root, "cmd/many", (120, 15), &theme, &none, Then::Exit);
        for _ in 0..8 {
            press(&mut s, Code::Tab, 0);
        }
        let shot = render(&s, &theme);
        assert!(
            shot.contains("field 9/9") && shot.contains("▸ v9"),
            "{shot}"
        );
        assert!(
            !shot.contains(" v1 "),
            "the first fields scrolled out:\n{shot}"
        );
        assert!(shot.contains("Preview") && shot.contains("d9"), "{shot}");
        assert_snapshot("form-scrolled.120x15.dark", &shot);
        drop(tmp);
    }

    #[test]
    fn recent_values_are_listed_under_a_text_field() {
        let (_tmp, root) = fixture_copy("vars");
        std::fs::create_dir_all(root.join(".kb")).unwrap();
        std::fs::write(
            root.join(".kb/state.json"),
            r#"{"state_version":1,"usage":{},"vars":{"recent":{"name":["api","db"]}}}"#,
        )
        .unwrap();
        let theme = theme_of("dark");
        let mut s = open(&root, "cmd/repeat", (120, 40), &theme, &none, Then::Exit);
        press(&mut s, Code::Down, ALT);
        let shot = render(&s, &theme);
        assert!(shot.contains("↳ api") && shot.contains("↳ db"), "{shot}");
        assert_snapshot("form-suggestions.120x40.dark", &shot);
    }

    #[test]
    fn variables_used_only_outside_the_payload_show_in_the_preview() {
        let (_tmp, root) = fixture_copy("vars");
        let theme = theme_of("dark");
        let mut s = open(&root, "cw/log-groups", (120, 40), &theme, &none, Then::Exit);
        assert!(render(&s, &theme).contains("log_groups[0]: /aws/lambda/{{svc}}"));
        type_text(&mut s, "api");
        let shot = render(&s, &theme);
        assert!(shot.contains("log_groups[0]: /aws/lambda/api"), "{shot}");
        assert!(shot.contains("fields @timestamp"), "{shot}");
        assert_snapshot("form-fields.120x40.dark", &shot);
        let s = open(&root, "cmd/fields", (120, 40), &theme, &none, Then::Exit);
        assert!(render(&s, &theme).contains("cwd: /tmp"));
        let s = open(&root, "sumo/range", (120, 40), &theme, &none, Then::Exit);
        assert!(render(&s, &theme).contains("range: -15m"));
    }
}

// ---- the add form (screen 4) ----

mod add_form {
    use super::*;
    use kb_cli::tui::add::AddState;
    use kb_cli::tui::app::{Action, AppState, Options};
    use kb_cli::tui::keys::{CTRL, Code, Key};
    use kb_core::author::{Draft, DraftOptions, DraftSource};
    use std::time::{Duration, UNIX_EPOCH};

    const CW: &str = "fields @timestamp, @message\n| filter @message like /{{fn}}/\n| stats pct(@duration, 99) by bin({{minutes}}m)";

    fn open(
        text: &str,
        size: (u16, u16),
        theme_name: &str,
    ) -> (AppState, kb_cli::tui::theme::Theme) {
        let theme = theme_of(theme_name);
        let opts = DraftOptions {
            title: Some("Authorizer p99 latency".into()),
            tags: vec!["lambda".into()],
            ..Default::default()
        };
        let draft = Draft::from_text(text, DraftSource::Clipboard, &opts);
        let mut state = AppState::new(Options::default(), size, "", "~/kb".into());
        let now = UNIX_EPOCH + Duration::from_secs(1_791_547_200);
        let tags = vec![("lambda".to_string(), 3), ("auth".to_string(), 2)];
        state.add = Some(Box::new(AddState::open(
            draft,
            tags,
            None,
            now,
            theme.is_color(),
        )));
        (state, theme)
    }

    fn press(state: &mut AppState, code: Code, mods: u8) {
        let ev = match code {
            Code::Char(c) => {
                let m = if mods & CTRL != 0 {
                    ratatui::crossterm::event::KeyModifiers::CONTROL
                } else {
                    ratatui::crossterm::event::KeyModifiers::NONE
                };
                ratatui::crossterm::event::KeyEvent::new(
                    ratatui::crossterm::event::KeyCode::Char(c),
                    m,
                )
            }
            Code::Esc => ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Esc,
                ratatui::crossterm::event::KeyModifiers::NONE,
            ),
            _ => unreachable!(),
        };
        let _ = Key::new(code, mods);
        state.update(Action::Key(ev), std::time::Instant::now());
    }

    #[test]
    fn wide_in_three_themes() {
        for t in ["dark", "light", "nocolor"] {
            let (state, theme) = open(CW, (120, 40), t);
            let shot = render(&state, &theme);
            assert!(
                shot.contains("New entry · from clipboard · detected cw (high)"),
                "{shot}"
            );
            assert!(
                shot.contains("{{fn}}") && shot.contains("Saves to"),
                "{shot}"
            );
            assert_snapshot(&format!("add-form.120x40.{t}"), &shot);
        }
    }

    #[test]
    fn suggestions_show_under_the_variables() {
        let (mut state, theme) = open(
            "_sourceCategory=prod/lambda/authorizer | timeslice 5m | where region = \"us-east-1\"",
            (120, 40),
            "dark",
        );
        state.add.as_mut().unwrap().focus = kb_cli::tui::add::Field::Variables;
        let shot = render(&state, &theme);
        assert!(
            shot.contains("suggest  prod → {{env}}  enum   [space]"),
            "{shot}"
        );
        assert_snapshot("add-form-suggestions.120x40.dark", &shot);
    }

    #[test]
    fn narrow() {
        let (state, theme) = open(CW, (80, 30), "dark");
        assert_snapshot("add-form.80x30.dark", &render(&state, &theme));
    }

    #[test]
    fn link_shows_fields_instead_of_a_body() {
        let (state, theme) = open("https://example.com/runbooks/auth", (120, 40), "dark");
        let shot = render(&state, &theme);
        assert!(shot.contains("Link") && !shot.contains("Body"), "{shot}");
        assert_snapshot("add-form-link.120x40.dark", &shot);
    }

    #[test]
    fn a_secret_is_highlighted_and_explained() {
        let (mut state, theme) = open(
            "aws s3 ls # AKIAIOSFODNN7EXAMPLE\nkubectl get pods -n prod",
            (120, 40),
            "dark",
        );
        press(&mut state, Code::Char('s'), CTRL);
        let shot = render(&state, &theme);
        assert!(shot.contains("KB060"), "{shot}");
        assert_snapshot("add-form-secret.120x40.dark", &shot);
    }

    #[test]
    fn preview_and_the_discard_prompt() {
        let (mut state, theme) = open(CW, (120, 40), "dark");
        press(&mut state, Code::Char('p'), CTRL);
        assert_snapshot("add-form-preview.120x40.dark", &render(&state, &theme));
        press(&mut state, Code::Esc, 0);
        state.add.as_mut().unwrap().changed = true;
        press(&mut state, Code::Esc, 0);
        let shot = render(&state, &theme);
        assert!(shot.contains("Discard this entry? (y/N)"), "{shot}");
        assert_snapshot("add-form-discard.120x40.dark", &shot);
    }
}
