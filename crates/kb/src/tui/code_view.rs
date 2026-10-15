//! Screen 3: Code View with syntax highlighting and drift status badge.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::{Block, Borders, Widget};
use std::path::{Path, PathBuf};

use kb_core::code::{CloneCache, CodeService, CodeSnippet, DriftReport, DriftStatus};
use kb_core::config::Settings;
use kb_core::entry::TypeFields;
use kb_core::search::Kb;

use super::app::AppState;
use super::highlight;
use super::keys::{self, Key};
use super::layout::truncate;
use super::theme::Theme;

#[derive(Debug, Clone)]
pub struct CodeViewState {
    pub id: String,
    pub title: String,
    pub repo: String,
    pub path: String,
    pub line: usize,
    pub end_line: Option<usize>,
    pub pinned_ref: String,
    pub branch: Option<String>,
    pub clone_path: Option<PathBuf>,
    pub drift: Option<DriftReport>,
    pub snippet: Option<CodeSnippet>,
    pub missing_clone: bool,
    pub permalink: Option<String>,
    pub default_open: String,
    pub repin_confirm: Option<DriftReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeViewEvent {
    None,
    Close,
    OpenDefault,
    OpenWeb,
    OpenVsCode,
    OpenIdea,
    CopyLocation,
    CopyPermalink,
    RepinPrompt,
    RepinConfirm(Box<DriftReport>),
    FetchAndRecheck,
    EditEntry,
}

impl CodeViewState {
    /// The re-pin question takes `y` / `n` / Esc as they are, whatever the keymap says.
    pub fn takes_raw_keys(&self) -> bool {
        self.repin_confirm.is_some()
    }

    pub fn handle_key(&mut self, key: Key) -> CodeViewEvent {
        if let Some(report) = self.repin_confirm.clone() {
            return match key.code {
                keys::Code::Char('y' | 'Y') if key.mods == 0 => {
                    self.repin_confirm = None;
                    CodeViewEvent::RepinConfirm(Box::new(report))
                }
                keys::Code::Char('n' | 'N') if key.mods == 0 => {
                    self.repin_confirm = None;
                    CodeViewEvent::None
                }
                keys::Code::Esc => {
                    self.repin_confirm = None;
                    CodeViewEvent::None
                }
                _ => CodeViewEvent::None,
            };
        }

        if key.mods == keys::CTRL && key.code == keys::Code::Char('e') {
            return CodeViewEvent::EditEntry;
        }

        match key.code {
            keys::Code::Esc => CodeViewEvent::Close,
            keys::Code::Enter => CodeViewEvent::OpenDefault,
            keys::Code::Char('g') if key.mods == 0 => CodeViewEvent::OpenWeb,
            keys::Code::Char('v') if key.mods == 0 => CodeViewEvent::OpenVsCode,
            keys::Code::Char('i') if key.mods == 0 => CodeViewEvent::OpenIdea,
            keys::Code::Char('y') if key.mods == 0 => CodeViewEvent::CopyLocation,
            keys::Code::Char('Y') if key.mods == 0 => CodeViewEvent::CopyPermalink,
            keys::Code::Char('u') if key.mods == 0 => match &self.drift {
                Some(drift) => match drift.status {
                    DriftStatus::Moved { .. }
                    | DriftStatus::Changed { .. }
                    | DriftStatus::Renamed { .. } => {
                        self.repin_confirm = Some(drift.clone());
                        CodeViewEvent::RepinPrompt
                    }
                    _ => CodeViewEvent::RepinPrompt,
                },
                None => CodeViewEvent::RepinPrompt,
            },
            keys::Code::Char('F') if key.mods == 0 => CodeViewEvent::FetchAndRecheck,
            _ => CodeViewEvent::None,
        }
    }
}

pub fn load_code_view_state(
    kb: &Kb,
    id: &str,
    settings: &Settings,
    fetch: bool,
) -> Result<CodeViewState, String> {
    let entry = kb.entry(id).map_err(|e| e.to_string())?;
    let (repo, path, line_u32, end_line_u32, pinned_ref, branch) = match entry.fields() {
        TypeFields::Code { code: Some(code) } => (
            code.repo.clone(),
            code.path.clone(),
            code.line,
            code.end_line,
            code.git_ref.clone(),
            code.branch.clone(),
        ),
        _ => return Err(format!("Entry '{id}' is not a valid code entry")),
    };
    let line = line_u32 as usize;
    let end_line = end_line_u32.map(|el| el as usize);

    use crate::platform::SystemGitRunner;
    let git_runner = SystemGitRunner;
    let root_path = Path::new(kb.root());
    let mut cache = CloneCache::load(root_path);
    let clone_opt = CodeService::find_clone(&repo, settings, &mut cache, &git_runner)
        .ok()
        .flatten();
    let _ = cache.save(root_path);

    let default_open = settings.code_default_open().to_string();

    let (clone_path, drift, snippet, missing_clone, permalink) = match clone_opt {
        Some(clone) => {
            if fetch {
                let _ = CodeService::fetch_remote(&clone, &git_runner);
            }
            let drift = CodeService::check_drift(&entry, &clone, &git_runner).ok();
            let snippet = CodeService::get_snippet(
                &clone,
                &pinned_ref,
                &path,
                line,
                end_line,
                3,
                8,
                &git_runner,
            )
            .ok();
            let permalink = CodeService::build_permalink(
                Some(&clone),
                &repo,
                &pinned_ref,
                &path,
                line,
                end_line,
                settings,
            )
            .ok();
            (Some(clone.local_path), drift, snippet, false, permalink)
        }
        None => {
            let permalink = CodeService::build_permalink(
                None,
                &repo,
                &pinned_ref,
                &path,
                line,
                end_line,
                settings,
            )
            .ok();
            (None, None, None, true, permalink)
        }
    };

    Ok(CodeViewState {
        id: id.to_string(),
        title: entry.title().unwrap_or(id).to_string(),
        repo,
        path,
        line,
        end_line,
        pinned_ref,
        branch,
        clone_path,
        drift,
        snippet,
        missing_clone,
        permalink,
        default_open,
        repin_confirm: None,
    })
}

pub fn draw_code_view(
    state: &AppState,
    cv: &CodeViewState,
    theme: &Theme,
    area: Rect,
    buf: &mut Buffer,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} (code) ", cv.title))
        .border_style(theme.border());
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.height < 5 || inner.width < 10 {
        return;
    }

    let mut y = inner.y;
    let width_u = usize::from(inner.width);

    // 1. Header line 1: Repo • Path • Ref
    let path_str = if let Some(el) = cv.end_line {
        format!("{}:{}-{}", cv.path, cv.line, el)
    } else {
        format!("{}:{}", cv.path, cv.line)
    };
    let short_ref = if cv.pinned_ref.len() > 7 {
        &cv.pinned_ref[..7]
    } else {
        &cv.pinned_ref
    };
    let meta_line = format!(
        "Repo: {}  •  Path: {}  •  Ref: {}",
        cv.repo, path_str, short_ref
    );
    buf.set_string(
        inner.x,
        y,
        truncate(&meta_line, width_u),
        theme.text().add_modifier(Modifier::BOLD),
    );
    y += 1;

    // 2. Header line 2: Status
    let (status_text, status_style) = if cv.missing_clone {
        (
            "Status: unknown (no local clone found)".to_string(),
            theme.warning(),
        )
    } else if let Some(drift) = &cv.drift {
        match &drift.status {
            DriftStatus::Current => (
                format!("Status: current (matches branch {})", drift.tracking_branch),
                theme.matched(),
            ),
            DriftStatus::Moved { new_line, .. } => (
                format!(
                    "Status: moved (now at line {new_line} on {})",
                    drift.tracking_branch
                ),
                theme.warning(),
            ),
            DriftStatus::Changed { diff_summary, .. } => (
                format!(
                    "Status: changed ({diff_summary} on {})",
                    drift.tracking_branch
                ),
                theme.warning(),
            ),
            DriftStatus::Renamed {
                new_path, new_line, ..
            } => (
                format!(
                    "Status: renamed (now at {new_path}:{new_line} on {})",
                    drift.tracking_branch
                ),
                theme.warning(),
            ),
            DriftStatus::Gone { reason } => (format!("Status: gone ({reason})"), theme.warning()),
            DriftStatus::Unknown { reason } => {
                (format!("Status: unknown ({reason})"), theme.warning())
            }
        }
    } else {
        ("Status: unknown".to_string(), theme.muted())
    };

    buf.set_string(inner.x, y, truncate(&status_text, width_u), status_style);
    y += 1;

    // 3. Separator line
    draw_separator(inner.x, y, inner.width, theme, buf);
    y += 1;

    // 4. Code snippet box or missing clone message
    let bottom_reserved = 3; // separator + 2-line keybar
    let snippet_max_y = inner.bottom().saturating_sub(bottom_reserved);

    if cv.missing_clone {
        let msg = format!(
            "No local clone found for repo '{}'. Configure [code.repos] or code.search_paths.",
            cv.repo
        );
        buf.set_string(inner.x, y, truncate(&msg, width_u), theme.warning());
    } else if let Some(snippet) = &cv.snippet {
        let ext = Path::new(&cv.path).extension().and_then(|s| s.to_str());
        let raw_lines: Vec<String> = snippet.lines.iter().map(|(_, l)| l.clone()).collect();
        let highlighted =
            highlight::highlight_code_snippet(&raw_lines, ext, state.options.highlight);

        for (i, (line_num, _)) in snippet.lines.iter().enumerate() {
            if y >= snippet_max_y {
                break;
            }
            let is_target = *line_num == snippet.target_line
                || snippet
                    .target_end_line
                    .is_some_and(|el| *line_num >= snippet.target_line && *line_num <= el);

            let gutter = format!("{:>4} │ ", line_num);
            let marker = if is_target { "▶ " } else { "  " };

            let marker_style = if is_target {
                theme.matched().add_modifier(Modifier::BOLD)
            } else {
                theme.muted()
            };

            buf.set_string(inner.x, y, &gutter, theme.muted());
            let mut curr_x = inner.x + gutter.len() as u16;

            buf.set_string(curr_x, y, marker, marker_style);
            curr_x += marker.len() as u16;

            if let Some(tok_spans) = highlighted.get(i) {
                for span in tok_spans {
                    let mut style = theme.class_style(span.class);
                    if is_target {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    let avail = (inner.right().saturating_sub(curr_x)) as usize;
                    if avail == 0 {
                        break;
                    }
                    let txt = truncate(&span.text, avail);
                    buf.set_string(curr_x, y, &txt, style);
                    curr_x += txt.len() as u16;
                }
            }
            y += 1;
        }
    }

    // 5. Separator line before footer
    let sep_y = inner.bottom().saturating_sub(3);
    draw_separator(inner.x, sep_y, inner.width, theme, buf);

    // 6. Footer key bar (2 lines)
    let bar_y1 = inner.bottom().saturating_sub(2);
    let bar_y2 = inner.bottom().saturating_sub(1);
    if let Some(m) = &state.message {
        let style = if m.kind == super::app::MessageKind::Warning {
            theme.warning()
        } else {
            theme.text()
        };
        buf.set_string(inner.x, bar_y1, truncate(&m.text, width_u), style);
        let keybar2 = "[F] fetch  [Ctrl-E] edit  [Esc] back";
        buf.set_string(inner.x, bar_y2, truncate(keybar2, width_u), theme.muted());
    } else if cv.repin_confirm.is_some() {
        let prompt = "Confirm re-pin? [y] yes  [n/Esc] cancel";
        buf.set_string(
            inner.x,
            bar_y1,
            truncate(prompt, width_u),
            theme.warning().add_modifier(Modifier::BOLD),
        );
    } else {
        let keybar1 = format!(
            "[Enter] open ({})  [g] web  [v] vscode  [i] idea  [y] path:line  [Y] permalink  [u] repin",
            cv.default_open
        );
        let keybar2 = "[F] fetch  [Ctrl-E] edit  [Esc] back";
        buf.set_string(inner.x, bar_y1, truncate(&keybar1, width_u), theme.muted());
        buf.set_string(inner.x, bar_y2, truncate(keybar2, width_u), theme.muted());
    }
}

fn draw_separator(x: u16, y: u16, width: u16, theme: &Theme, buf: &mut Buffer) {
    let sep = "─".repeat(usize::from(width));
    buf.set_string(x, y, sep, theme.border());
}
