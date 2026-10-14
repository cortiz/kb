//! Full-screen conflict screen for `Alt-S`: mine and theirs side by side (stacked when narrow),
//! the frontmatter differences, and the keys that decide the entry.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use kb_core::sync::{Conflict, ConflictKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Clear, Widget};

use super::layout::truncate;
use super::theme::Theme;

/// From this width on, mine and theirs are side by side.
pub const SIDE_BY_SIDE: u16 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictScreen {
    pub conflict: Conflict,
    /// 1-based position among the conflicts resolved in this sync.
    pub number: usize,
    pub confirm_abort: bool,
    /// Typing a new ID for "keep both": the text so far, and which ID was taken.
    pub new_id: Option<(String, String)>,
    /// What was wrong with the last merged text.
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictOutcome {
    Mine,
    Theirs,
    Both,
    Delete,
    Edit,
    Abort,
    /// A new ID for "keep both" was entered.
    NewId(String),
}

impl ConflictScreen {
    pub fn new(conflict: Conflict, number: usize) -> Self {
        Self {
            conflict,
            number,
            confirm_abort: false,
            new_id: None,
            message: None,
        }
    }

    pub fn key(&mut self, e: KeyEvent) -> Option<ConflictOutcome> {
        if let Some((text, _)) = &mut self.new_id {
            match e.code {
                KeyCode::Enter if !text.trim().is_empty() => {
                    return Some(ConflictOutcome::NewId(text.trim().to_string()));
                }
                KeyCode::Esc => return Some(ConflictOutcome::Abort),
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char(c) if !e.modifiers.contains(KeyModifiers::CONTROL) => text.push(c),
                _ => {}
            }
            return None;
        }
        if self.confirm_abort {
            match e.code {
                KeyCode::Char('y') => return Some(ConflictOutcome::Abort),
                _ => self.confirm_abort = false,
            }
            return None;
        }
        if e.modifiers.contains(KeyModifiers::CONTROL) {
            return (e.code == KeyCode::Char('c')).then_some(ConflictOutcome::Abort);
        }
        match e.code {
            KeyCode::Char('m') => Some(ConflictOutcome::Mine),
            KeyCode::Char('t') => Some(ConflictOutcome::Theirs),
            KeyCode::Char('b') => Some(ConflictOutcome::Both),
            KeyCode::Char('d') => Some(ConflictOutcome::Delete),
            KeyCode::Char('e') => Some(ConflictOutcome::Edit),
            KeyCode::Esc | KeyCode::Char('q') => {
                self.confirm_abort = true;
                None
            }
            _ => None,
        }
    }
}

fn frontmatter(text: &str) -> Vec<(String, String)> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Vec::new();
    }
    lines
        .take_while(|l| l.trim() != "---")
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// One line per frontmatter field that differs: `key: mine vs theirs`.
pub fn frontmatter_diff(mine: &str, theirs: &str) -> Vec<String> {
    let (m, t) = (frontmatter(mine), frontmatter(theirs));
    let get =
        |v: &[(String, String)], k: &str| v.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
    let mut keys: Vec<&String> = m.iter().chain(t.iter()).map(|(k, _)| k).collect();
    keys.dedup();
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for k in keys {
        if seen.contains(&k) {
            continue;
        }
        seen.push(k);
        let (a, b) = (get(&m, k), get(&t, k));
        if a != b {
            let show = |v: Option<String>| v.unwrap_or_else(|| "(none)".into());
            out.push(format!("{k}: {} vs {}", show(a), show(b)));
        }
    }
    out
}

fn kind_text(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::BothModified => "both modified",
        ConflictKind::ModifiedVsDeleted => "modified here, deleted there",
        ConflictKind::DeletedVsModified => "deleted here, modified there",
        ConflictKind::BothAdded => "both added",
        ConflictKind::RenamedDifferently => "renamed differently",
    }
}

fn pane(title: &str, text: Option<&str>, area: Rect, theme: &Theme, buf: &mut Buffer) {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(theme.border())
        .render(area, buf);
    let w = usize::from(area.width.saturating_sub(2));
    let Some(text) = text else {
        buf.set_string(area.x + 1, area.y + 1, "(deleted)", theme.muted());
        return;
    };
    for (i, line) in text
        .lines()
        .take(usize::from(area.height.saturating_sub(2)))
        .enumerate()
    {
        buf.set_string(
            area.x + 1,
            area.y + 1 + i as u16,
            truncate(line, w),
            theme.text(),
        );
    }
}

pub fn draw_conflict(screen: &ConflictScreen, theme: &Theme, area: Rect, buf: &mut Buffer) {
    Clear.render(area, buf);
    if area.width < 40 || area.height < 12 {
        buf.set_string(area.x, area.y, "terminal too small", theme.warning());
        return;
    }
    let c = &screen.conflict;
    let title = format!(
        " Conflict {}: {} ({}) ",
        screen.number,
        c.id,
        kind_text(c.kind)
    );
    buf.set_string(
        area.x,
        area.y,
        truncate(&title, usize::from(area.width)),
        theme.warning(),
    );

    let diff = frontmatter_diff(
        c.mine.as_deref().unwrap_or(""),
        c.theirs.as_deref().unwrap_or(""),
    );
    let diff_rows = (diff.len() as u16 + 1).min(5);
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(3 + diff_rows),
    );
    let (mine, theirs) = if area.width >= SIDE_BY_SIDE {
        let half = body.width / 2;
        (
            Rect::new(body.x, body.y, half, body.height),
            Rect::new(body.x + half, body.y, body.width - half, body.height),
        )
    } else {
        let half = body.height / 2;
        (
            Rect::new(body.x, body.y, body.width, half),
            Rect::new(body.x, body.y + half, body.width, body.height - half),
        )
    };
    pane("MINE (local)", c.mine.as_deref(), mine, theme, buf);
    pane("THEIRS (remote)", c.theirs.as_deref(), theirs, theme, buf);

    let mut y = body.y + body.height;
    buf.set_string(area.x, y, "Frontmatter differences:", theme.muted());
    for line in diff.iter().take(usize::from(diff_rows) - 1) {
        y += 1;
        buf.set_string(
            area.x + 2,
            y,
            truncate(line, usize::from(area.width) - 2),
            theme.text(),
        );
    }
    if diff.is_empty() {
        buf.set_string(area.x + 2, y + 1, "(none)", theme.muted());
    }

    let w = usize::from(area.width);
    let footer = if let Some((text, taken)) = &screen.new_id {
        format!("{taken} exists; save theirs as: {text}_   [Enter] ok  [Esc] abort")
    } else if screen.confirm_abort {
        "Abort sync and restore repository? (y/n)".to_string()
    } else if let Some(m) = &screen.message {
        m.lines().next().unwrap_or("").to_string()
    } else {
        "[m] Mine  [t] Theirs  [e] Edit merged  [b] Keep both  [d] Delete  [Esc] Abort".to_string()
    };
    buf.set_string(
        area.x,
        area.y + area.height - 1,
        truncate(&footer, w),
        theme.warning(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn conflict() -> Conflict {
        Conflict {
            id: "a".into(),
            rel_path: "a.md".into(),
            kind: ConflictKind::BothModified,
            mine: Some("---\ntitle: One\ntags: [x]\n---\nmine\n".into()),
            theirs: Some("---\ntitle: Two\ntags: [x]\n---\ntheirs\n".into()),
            base: None,
        }
    }

    fn press(c: KeyCode) -> KeyEvent {
        let mut e = KeyEvent::new(c, KeyModifiers::NONE);
        e.kind = KeyEventKind::Press;
        e
    }

    fn text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn frontmatter_diff_lists_only_changed_fields() {
        let c = conflict();
        assert_eq!(
            frontmatter_diff(c.mine.as_deref().unwrap(), c.theirs.as_deref().unwrap()),
            ["title: One vs Two"]
        );
    }

    #[test]
    fn keys_choose_and_escape_asks_first() {
        let mut s = ConflictScreen::new(conflict(), 1);
        assert_eq!(
            s.key(press(KeyCode::Char('m'))),
            Some(ConflictOutcome::Mine)
        );
        assert_eq!(
            s.key(press(KeyCode::Char('b'))),
            Some(ConflictOutcome::Both)
        );
        assert_eq!(s.key(press(KeyCode::Esc)), None);
        assert!(s.confirm_abort);
        assert_eq!(s.key(press(KeyCode::Char('n'))), None);
        assert!(!s.confirm_abort, "any other key cancels the question");
        s.key(press(KeyCode::Esc));
        assert_eq!(
            s.key(press(KeyCode::Char('y'))),
            Some(ConflictOutcome::Abort)
        );
    }

    #[test]
    fn new_id_prompt_collects_text() {
        let mut s = ConflictScreen::new(conflict(), 1);
        s.new_id = Some((String::new(), "a-conflict-mac".into()));
        for c in "ab".chars() {
            s.key(press(KeyCode::Char(c)));
        }
        s.key(press(KeyCode::Backspace));
        assert_eq!(
            s.key(press(KeyCode::Enter)),
            Some(ConflictOutcome::NewId("a".into()))
        );
    }

    #[test]
    fn wide_is_side_by_side_and_narrow_is_stacked() {
        let theme = Theme::no_color();
        let s = ConflictScreen::new(conflict(), 1);
        let draw = |w, h| {
            let area = Rect::new(0, 0, w, h);
            let mut buf = Buffer::empty(area);
            draw_conflict(&s, &theme, area, &mut buf);
            text(&buf)
        };
        let wide = draw(130, 24);
        let row = wide.lines().find(|l| l.contains("MINE")).unwrap();
        assert!(row.contains("THEIRS"), "same row when wide");
        let narrow = draw(80, 24);
        assert!(
            !narrow
                .lines()
                .find(|l| l.contains("MINE"))
                .unwrap()
                .contains("THEIRS")
        );
        assert!(narrow.contains("title: One vs Two"));
        assert!(narrow.contains("[m] Mine"));
    }
}
