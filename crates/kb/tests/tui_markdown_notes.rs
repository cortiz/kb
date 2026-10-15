//! Notes in the preview pane: stored screens, and the real UI under a pseudo-terminal.

mod support;

use std::fs;

use support::pty::*;
use support::snapshot::*;

const WIDE_TABLE: &str = "---\nkb_format: 1\ntype: note\ntitle: Ports and owners\ntags: [ops]\n---\n\n# Ports and owners\n\n| Service | Port | Owner | Notes |\n|:--------|:----:|------:|:------|\n| authorizer | 8443 | platform-security | terminates TLS and checks the role cache before forwarding |\n| billing-api | 9090 | payments | rate limited per tenant, see the runbook for the limits |\n\nAfter the table.\n";

/// The `tui` fixture KB with the sample notes and the entry they link to.
fn notes_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let (tmp, root) = fixture_copy("tui");
    support::add_notes(&root);
    fs::create_dir_all(root.join("sumo")).unwrap();
    fs::copy(
        support::fixtures().join("search/sumo/5xx-by-authorizer.md"),
        root.join("sumo/5xx-by-authorizer.md"),
    )
    .unwrap();
    fs::write(root.join("note/ports.md"), WIDE_TABLE).unwrap();
    (tmp, root)
}

#[test]
fn a_note_in_every_theme_and_size() {
    let (_tmp, root) = notes_fixture();
    for (w, h) in [(80u16, 24u16), (120, 40), (200, 60)] {
        for theme_name in ["dark", "light", "nocolor"] {
            let theme = theme_of(theme_name);
            let mut state = state_for(&root, "", Some("note/role-cache-ttl"), (w, h), &theme);
            if w < 100 {
                state.layout_mode = kb_cli::tui::app::LayoutMode::FullPreview;
            }
            let shot = render(&state, &theme);
            for line in shot.lines().take(usize::from(h)) {
                assert!(
                    unicode_width::UnicodeWidthStr::width(line) <= usize::from(w),
                    "{w}x{h}: {line:?}"
                );
            }
            if w == 200 {
                for want in [
                    "┌─",
                    "[sumo] 5xx by authorizer",
                    "▌ ⚠ Warning",
                    "☑ Read the runbook",
                ] {
                    assert!(shot.contains(want), "{want:?} missing:\n{shot}");
                }
            }
            assert_snapshot(&format!("note.{w}x{h}.{theme_name}"), &shot);
        }
    }
}

#[test]
fn a_wide_table_wraps_inside_a_narrow_pane() {
    let (_tmp, root) = notes_fixture();
    let theme = theme_of("dark");
    let state = state_for(&root, "", Some("note/ports"), (100, 40), &theme);
    let shot = render(&state, &theme);
    let rows: Vec<&str> = shot.lines().take(40).collect();
    let corner = rows
        .iter()
        .position(|r| r.contains('┌') && r.contains('┬'))
        .expect("the table top");
    let bottom = rows
        .iter()
        .position(|r| r.contains('└') && r.contains('┴'))
        .expect("the table bottom");
    assert!(
        bottom - corner > 5,
        "the cells wrapped onto several lines:\n{shot}"
    );
    assert!(
        rows[corner].contains('┐') && rows[bottom].contains('┘'),
        "the table is whole:\n{shot}"
    );
    assert_snapshot("note-table.100x40.dark", &shot);
}

fn kb_with_notes() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let (t, home, root) = kb_home("search");
    support::add_notes(&root);
    fs::write(root.join("note/ports.md"), WIDE_TABLE).unwrap();
    (t, home, root)
}

#[test]
fn the_preview_pane_draws_a_note_and_scrolls_it_smoothly() {
    let (_t, home, root) = kb_with_notes();
    let mut spec = Spec::new(&[], &home, &root);
    spec.size = (160, 44);
    let mut s = Session::spawn(&spec);
    s.wait_for("entries");
    s.type_text("role cache ttl");
    s.wait_for("Role cache TTL and Redis architecture");
    s.wait_for("[sumo] 5xx by authorizer");
    s.wait_for("↓ more"); // drawn last: the whole frame, not a half-drawn one
    let screen = s.screen();
    for want in [
        "┌────",
        "│ Role Revoke",
        "▌ ⚠ Warning",
        "☐ Check the hit ratio",
    ] {
        assert!(screen.contains(want), "{want:?} missing:\n{screen}");
    }
    assert!(
        screen.contains("↓ more"),
        "the note is longer than the pane:\n{screen}"
    );
    // each Shift-Down moves the view by one line
    let pane = |rows: &[String]| -> Vec<String> {
        rows.iter()
            .skip(5)
            .take(30)
            .map(|r| r.chars().skip(80).collect::<String>())
            .collect()
    };
    let before = s.rows();
    s.send(SHIFT_DOWN);
    s.wait_until("one line of scroll", |screen| {
        screen.contains("↑ more") && pane(&s.rows()) != pane(&before)
    });
    let after = s.rows();
    assert_ne!(pane(&before), pane(&after));
    for _ in 0..30 {
        s.send(SHIFT_DOWN);
    }
    s.wait_for("Footnote text.");
    s.send(CTRL_C);
}

#[test]
fn code_blocks_are_framed_and_a_hostile_note_is_shown_inert() {
    let (_t, home, root) = kb_with_notes();
    let mut spec = Spec::new(&[], &home, &root);
    spec.size = (160, 60);
    let mut s = Session::spawn(&spec);
    s.wait_for("entries");
    s.type_text("hostile");
    s.wait_for("Hostile note");
    s.wait_for("<script>alert(1)</script>");
    let screen = s.screen();
    assert!(screen.contains('␛'), "{screen}");
    assert!(screen.contains("<div onclick"), "{screen}");
    s.send(CTRL_C);
    let (_t2, home2, root2) = kb_with_notes();
    let mut spec = Spec::new(&[], &home2, &root2);
    spec.size = (160, 60);
    let mut s = Session::spawn(&spec);
    s.wait_for("entries");
    s.type_text("role cache ttl");
    s.wait_for("┌─ python ─");
    s.wait_for("│ import redis");
    s.send(CTRL_C);
}
