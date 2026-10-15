use super::*;

fn ov(ctx: &str, cmd: &str, keys: &[&str], line: usize) -> KeyOverride {
    KeyOverride {
        context: ctx.into(),
        command: cmd.into(),
        keys: keys.iter().map(|s| s.to_string()).collect(),
        file: Some("config.toml".into()),
        line: Some(line),
        error: None,
    }
}

fn vim(o: &[KeyOverride]) -> EffectiveKeymap {
    EffectiveKeymap::build(Keymap::Vim, o)
}

#[test]
fn key_syntax_round_trips() {
    for (text, key) in [
        ("ctrl-o", Key::ctrl('o')),
        ("Ctrl-O", Key::ctrl('o')),
        ("alt-a", Key::new(Code::Char('a'), ALT)),
        ("ctrl-alt-x", Key::new(Code::Char('x'), CTRL | ALT)),
        ("shift-a", Key::plain('A')),
        ("G", Key::plain('G')),
        ("ñ", Key::plain('ñ')),
        ("-", Key::plain('-')),
        ("space", Key::plain(' ')),
        ("f12", Key::new(Code::F(12), 0)),
        ("shift-down", Key::new(Code::Down, SHIFT)),
        ("shift-tab", Key::new(Code::BackTab, 0)),
        ("pagedown", Key::new(Code::PageDown, 0)),
    ] {
        assert_eq!(parse_key(text), Ok(key), "{text}");
        assert_eq!(parse_key(&key.name()), Ok(key), "{text} -> {}", key.name());
    }
    for bad in [
        "",
        "ctrl-",
        "f13",
        "f0",
        "ctrl-ctrl-a",
        "shift-1",
        "enterr",
        "ab",
    ] {
        assert!(parse_key(bad).is_err(), "{bad}");
    }
    let seq = parse_seq("g g").unwrap();
    assert_eq!(seq.second, Some(Key::plain('g')));
    assert_eq!(seq.name(), "g g");
    assert!(parse_seq("a b c").is_err());
}

#[test]
fn presets_have_no_duplicates() {
    for k in [Keymap::Emacs, Keymap::Vim] {
        assert_eq!(duplicates(k), Vec::<String>::new(), "{k:?}");
    }
}

#[test]
fn every_preset_command_is_valid_where_it_is_bound() {
    for k in [Keymap::Emacs, Keymap::Vim] {
        for ctx in Context::ALL {
            for (cmd, _) in rows(k, ctx) {
                assert!(valid(k, ctx, cmd), "{k:?} {ctx:?} {cmd:?}");
            }
        }
    }
}

#[test]
fn command_names_round_trip() {
    for c in Command::ALL {
        assert_eq!(Command::from_name(c.name()), Some(*c));
    }
}

#[test]
fn every_emacs_binding_also_works_in_both_vim_modes() {
    let e = EffectiveKeymap::preset(Keymap::Emacs);
    let v = EffectiveKeymap::preset(Keymap::Vim);
    for (cmd, b) in e.context(Context::Search) {
        if cmd == Command::Cancel {
            continue; // Esc leaves search mode in vim
        }
        for k in &b.keys {
            assert_eq!(
                v.lookup(Context::Search, k.first),
                Some(cmd),
                "search {cmd:?}"
            );
        }
        if matches!(
            cmd,
            Command::Left
                | Command::Right
                | Command::LineStart
                | Command::LineEnd
                | Command::Backspace
                | Command::Delete
                | Command::DeleteWord
                | Command::ClearLine
        ) {
            continue; // line editing has no meaning outside the query
        }
        for k in &b.keys {
            let got = v.lookup(Context::Normal, k.first);
            // Ctrl-U is half a page in normal mode (FR-008); `?` stays help
            if k.first == Key::ctrl('u') {
                continue;
            }
            assert_eq!(got, Some(cmd), "normal {cmd:?} {}", k.name());
        }
    }
}

#[test]
fn every_ctrl_or_alt_action_has_a_single_key_in_normal_mode() {
    let v = EffectiveKeymap::preset(Keymap::Vim);
    for cmd in [
        Command::Copy,
        Command::CopyTemplate,
        Command::Open,
        Command::Run,
        Command::ActionMenu,
        Command::Edit,
        Command::AddEntry,
        Command::RenameEntry,
        Command::DeleteEntry,
        Command::Sync,
        Command::History,
        Command::TogglePreview,
    ] {
        assert!(
            v.keys(Context::Normal, cmd)
                .iter()
                .any(|k| k.second.is_none() && k.first.is_printable()),
            "{cmd:?}"
        );
    }
}

#[test]
fn the_scenario_from_the_spec() {
    let k = vim(&[
        ov("normal", "run", &["x", "!"], 5),
        ov("search", "action-menu", &["ctrl-p", "alt-a", "f2"], 8),
        ov("global", "history", &[], 11),
    ]);
    assert_eq!(k.warnings.len(), 1, "{:?}", k.warnings); // ctrl-p moved off `up`
    assert!(k.warnings[0].message.contains("ctrl-p"));
    assert_eq!(
        k.lookup(Context::Normal, Key::plain('x')),
        Some(Command::Run)
    );
    assert_eq!(
        k.lookup(Context::Normal, Key::plain('!')),
        Some(Command::Run)
    );
    assert_eq!(
        k.lookup(Context::Normal, Key::ctrl('g')),
        None,
        "replaced, not added"
    );
    assert_eq!(
        k.lookup(Context::Search, Key::ctrl('p')),
        Some(Command::ActionMenu)
    );
    assert_eq!(k.lookup(Context::Search, Key::ctrl('k')), Some(Command::Up));
    assert!(k.keys(Context::Search, Command::History).is_empty());
    assert!(k.keys(Context::Normal, Command::History).is_empty());
    assert_eq!(
        k.lookup(Context::Normal, Key::plain('o')),
        Some(Command::Open)
    );
    assert_eq!(k.active_overrides, 3);
}

#[test]
fn a_mode_override_beats_global() {
    let k = vim(&[
        ov("global", "refresh", &["f5"], 1),
        ov("normal", "refresh", &["r"], 2),
    ]);
    assert!(k.warnings.is_empty(), "{:?}", k.warnings);
    assert_eq!(
        k.lookup(Context::Search, Key::new(Code::F(5), 0)),
        Some(Command::Refresh)
    );
    assert_eq!(k.lookup(Context::Normal, Key::new(Code::F(5), 0)), None);
    assert_eq!(
        k.lookup(Context::Normal, Key::plain('r')),
        Some(Command::Refresh)
    );
}

#[test]
fn two_overrides_with_one_key_are_both_ignored() {
    let k = vim(&[
        ov("normal", "run", &["x"], 3),
        ov("normal", "edit", &["x"], 4),
    ]);
    assert_eq!(k.warnings.len(), 1);
    assert!(k.warnings[0].message.contains("several commands"));
    assert_eq!(
        k.lookup(Context::Normal, Key::plain('!')),
        Some(Command::Run)
    );
    assert_eq!(
        k.lookup(Context::Normal, Key::plain('e')),
        Some(Command::Edit)
    );
    assert_eq!(k.lookup(Context::Normal, Key::plain('x')), None);
    assert_eq!(k.active_overrides, 0);
}

#[test]
fn one_warning_per_kind_of_bad_override() {
    let bad = [
        ov("nowhere", "run", &["x"], 1),
        ov("normal", "runn", &["x"], 2),
        ov("normal", "run", &["ctrl-"], 3),
        ov("search", "run", &["x"], 4),
        ov("search", "run", &["g g"], 5),
        ov("normal", "quit", &["q"], 6),
        ov("global", "down", &["ctrl-c"], 7),
        ov("search", "help", &[], 8),
        ov("conflict", "quit", &["x"], 9),
        ov("action-menu", "down", &["x"], 10),
    ];
    let good = ov("normal", "open", &["x"], 11);
    let mut all = bad.to_vec();
    all.push(good);
    let k = vim(&all);
    assert_eq!(k.warnings.len(), bad.len(), "{:#?}", k.warnings);
    assert_eq!(
        k.lookup(Context::Normal, Key::plain('x')),
        Some(Command::Open)
    );
    assert_eq!(k.active_overrides, 1);
    assert!(
        k.keys(Context::Search, Command::Help).len() == 2,
        "help kept"
    );
    assert!(k.lookup(Context::Normal, Key::ctrl('c')) == Some(Command::Quit));
}

#[test]
fn emacs_ignores_normal_mode_overrides() {
    let k = EffectiveKeymap::build(Keymap::Emacs, &[ov("normal", "run", &["x"], 1)]);
    assert_eq!(k.warnings.len(), 1);
    assert!(k.warnings[0].message.contains("no normal mode"));
}

#[test]
fn a_shape_error_is_a_warning() {
    let mut o = ov("normal", "run", &[], 1);
    o.error = Some("expected a list of strings".into());
    let k = vim(&[o]);
    assert_eq!(k.warnings.len(), 1);
}

#[test]
fn translate_maps_user_keys_to_the_defaults_and_swallows_moved_ones() {
    let k = vim(&[ov("history", "down", &["n"], 1)]);
    assert_eq!(
        k.translate(Context::History, Key::plain('n')),
        Translated::Key(Key::new(Code::Down, 0))
    );
    assert_eq!(
        k.translate(Context::History, Key::plain('j')),
        Translated::Swallow
    );
    assert_eq!(
        k.translate(Context::History, Key::plain('z')),
        Translated::Key(Key::plain('z'))
    );
    assert_eq!(
        k.translate(Context::History, Key::new(Code::Esc, 0)),
        Translated::Key(Key::new(Code::Esc, 0))
    );
}

#[test]
fn sequences_are_detected() {
    let k = EffectiveKeymap::preset(Keymap::Vim);
    assert!(k.starts_sequence(Context::Normal, Key::plain('g')));
    assert!(!k.starts_sequence(Context::Normal, Key::plain('j')));
    assert_eq!(
        k.lookup_seq(Context::Normal, Key::plain('g'), Key::plain('g')),
        Some(Command::First)
    );
    assert_eq!(k.lookup(Context::Normal, Key::plain('g')), None);
}

#[test]
fn translate_leaves_untouched_default_keys_alone() {
    let k = vim(&[]);
    // `down` is an alias of `next-field` in the form; it must not turn into Tab
    assert_eq!(
        k.translate(Context::Form, Key::new(Code::Down, 0)),
        Translated::Key(Key::new(Code::Down, 0))
    );
}
