//! Note content can never take over the terminal: what reaches the pty from `kb show`, the UI and
//! the search output holds no escape sequence of a note's making.

mod support;

use std::fs;
use std::path::Path;

use support::pty::*;

/// The ESC sequences in `raw`, as written: `(kind, text)` where kind is the byte after ESC.
fn sequences(raw: &str) -> Vec<(char, String)> {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\u{1b}' {
            i += 1;
            continue;
        }
        let kind = chars.get(i + 1).copied().unwrap_or('\0');
        let mut j = i + 2;
        match kind {
            '[' => {
                while j < chars.len() && !('\u{40}'..='\u{7e}').contains(&chars[j]) {
                    j += 1;
                }
                j += 1;
            }
            ']' | 'P' | '_' | '^' | 'X' => {
                while j < chars.len()
                    && chars[j] != '\u{7}'
                    && !(chars[j] == '\u{1b}' && chars.get(j + 1) == Some(&'\\'))
                {
                    j += 1;
                }
                j += if chars.get(j) == Some(&'\u{7}') { 1 } else { 2 };
            }
            _ => {}
        }
        out.push((kind, chars[i..j.min(chars.len())].iter().collect()));
        i = j;
    }
    out
}

fn is_sgr(seq: &str) -> bool {
    seq.strip_prefix("\u{1b}[")
        .and_then(|s| s.strip_suffix('m'))
        .is_some_and(|params| params.chars().all(|c| c.is_ascii_digit() || c == ';'))
}

fn home_with_notes() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let (t, home, root) = kb_home("search");
    support::add_notes(&root);
    (t, home, root)
}

fn show_in_pty(home: &Path, root: &Path, id: &str, env: &[(&str, &str)]) -> String {
    let mut spec = Spec::new(&["show", id], home, root);
    for (k, v) in env {
        spec = spec.env(k, v);
    }
    let mut s = Session::spawn(&spec);
    assert_eq!(s.wait_exit(), 0, "{}", s.screen());
    s.raw_output()
}

#[test]
fn kb_show_on_a_terminal_emits_only_colors_for_a_hostile_note() {
    let (_t, home, root) = home_with_notes();
    let raw = show_in_pty(&home, &root, "note/malicious", &[]);
    let found = sequences(&raw);
    assert!(!found.is_empty(), "a terminal gets colors");
    for (kind, seq) in &found {
        assert!(
            *kind == '[' && is_sgr(seq),
            "an escape sequence that is not a color: {seq:?}"
        );
    }
    for needle in [
        "52;c;aGk=",
        "evil.example",
        "pwned",
        "2J",
        "\u{7}",
        "\u{9b}",
        "\u{9d}",
    ] {
        assert!(
            !raw.contains(needle),
            "{needle:?} reached the terminal:\n{raw:?}"
        );
    }
    assert!(raw.contains('␛'), "the sequences are shown as markers");
    assert!(raw.contains("<script>alert(1)</script>"), "HTML is text");
}

#[test]
fn hyperlinks_are_kb_s_own_and_only_for_safe_schemes_and_supporting_terminals() {
    let (_t, home, root) = home_with_notes();
    fs::write(
        root.join("note/web.md"),
        "---\nkb_format: 1\ntype: note\ntitle: Web\n---\n\n# Web\n\nSee [the docs](https://example.com/docs?a=1), [mail](mailto:ops@example.com), <https://auto.example.com/x>, [bad](javascript:alert(1)), [data](data:text/html,x), [file](file:///etc/passwd) and [spoof](https://a\u{7}b.example).\n",
    )
    .unwrap();
    let supported = show_in_pty(&home, &root, "note/web", &[("TERM_PROGRAM", "iTerm.app")]);
    for url in [
        "https://example.com/docs?a=1",
        "mailto:ops@example.com",
        "https://auto.example.com/x",
    ] {
        assert!(
            supported.contains(&format!("\u{1b}]8;;{url}\u{1b}\\")),
            "{url} should be a hyperlink:\n{supported:?}"
        );
    }
    assert!(
        supported.contains("the docs\u{1b}]8;;\u{1b}\\"),
        "{supported:?}"
    );
    for bad in ["javascript", "data:", "file://", "a\u{7}b"] {
        assert!(
            !supported.contains(&format!("]8;;{bad}")),
            "{bad} became a link:\n{supported:?}"
        );
    }
    // every OSC is one of kb's hyperlink sequences
    for (kind, seq) in sequences(&supported) {
        assert!(
            (kind == '[' && is_sgr(&seq)) || (kind == ']' && seq.starts_with("\u{1b}]8;;")),
            "{seq:?}"
        );
    }
    // a terminal that does not advertise support gets no hyperlinks, and the text is the same
    let plain = show_in_pty(
        &home,
        &root,
        "note/web",
        &[("TERM_PROGRAM", "Apple_Terminal")],
    );
    assert!(!plain.contains("]8;"), "{plain:?}");
    assert!(plain.contains("the docs"));
    // and a pipe never gets one, even when the terminal is known to support them
    let piped = String::from_utf8(
        support::kb_in(&root)
            .args(["show", "note/web"])
            .env("TERM_PROGRAM", "iTerm.app")
            .env("FORCE_HYPERLINK", "1")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert!(!piped.contains('\u{1b}'), "{piped:?}");
}

#[test]
fn the_ui_draws_a_hostile_note_without_passing_its_sequences_on() {
    let (_t, home, root) = home_with_notes();
    let mut spec = Spec::new(&[], &home, &root);
    spec.size = (160, 60);
    let mut s = Session::spawn(&spec);
    s.wait_for("entries");
    s.type_text("hostile");
    s.wait_for("<script>alert(1)</script>");
    let screen = s.screen();
    assert!(screen.contains('␛'), "{screen}");
    let raw = s.raw_output();
    for (kind, seq) in sequences(&raw) {
        assert!(
            !matches!(kind, ']' | 'P' | '_' | '^' | 'X'),
            "a string sequence reached the terminal: {seq:?}"
        );
    }
    for needle in ["52;c;aGk=", "evil.example", "pwned"] {
        assert!(!raw.contains(needle), "{needle:?} reached the terminal");
    }
    s.send(CTRL_C);
}

#[test]
fn titles_tags_and_snippets_in_search_output_are_defused() {
    let (_t, home, root) = home_with_notes();
    fs::write(
        root.join("note/evil.md"),
        "---\nkb_format: 1\ntype: note\ntitle: \"Evil \\e[2J \\x1b]52;c;aGk=\\x07 title\"\ntags: [\"t\\e[31mag\", plain]\n---\n\n# Body\n\nneedle \u{1b}[2J \u{1b}]0;pwn\u{7} text\n",
    )
    .unwrap();
    for args in [
        vec!["search", "needle"],
        vec!["search", "needle", "--json"],
        vec!["search", "needle", "--ids"],
    ] {
        let out = support::kb_in(&root).args(&args).output().unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains("note/evil"), "{args:?}: {text}");
        assert!(
            text.chars().all(|c| c == '\n' || !c.is_control()),
            "{args:?}: control characters in {text:?}"
        );
    }
    // the UI list and header too
    let mut s = Session::spawn(&Spec::new(&[], &home, &root));
    s.wait_for("entries");
    s.type_text("needle");
    s.wait_for("Evil");
    let raw = s.raw_output();
    for (kind, seq) in sequences(&raw) {
        assert!(!matches!(kind, ']' | 'P' | '_' | '^' | 'X'), "{seq:?}");
    }
    assert!(!raw.contains("52;c;aGk="), "{raw:?}");
    s.send(CTRL_C);
}

#[test]
fn a_file_named_with_an_escape_sequence_is_not_an_entry() {
    let (_t, _home, root) = home_with_notes();
    let evil = root.join("note/evil\u{1b}]52;c;aGk=\u{7}.md");
    fs::write(
        &evil,
        "---\nkb_format: 1\ntype: note\ntitle: Named badly\n---\n\n# Named badly\n",
    )
    .unwrap();
    let out = support::kb_in(&root)
        .args(["search", "named", "badly", "--ids"])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        !text.contains("badly") && !text.contains('\u{1b}'),
        "{text:?}"
    );
    let lint = support::kb_in(&root).arg("lint").output().unwrap();
    assert!(!String::from_utf8(lint.stdout).unwrap().contains('\u{1b}'));
}
