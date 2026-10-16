//! The preview of one entry, built on the worker and independent of any theme.

use kb_core::entry::{Entry, EntryType, TypeFields};
use kb_core::payload::payload_pieces;
use kb_core::vars::VarKind;

use super::highlight::{Span, highlight, lang_for};
use super::markdown::{Ctx, Lookup, Notes, render_markdown};
use crate::output::utc;

#[derive(Debug, Clone, PartialEq)]
pub struct Meta {
    pub entry_type: EntryType,
    pub tags: Vec<String>,
    pub use_count: u64,
    /// The entry's `updated:` value, else the file's modification date as `YYYY-MM-DD`.
    pub edited: Option<String>,
    pub unreviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarRow {
    pub name: String,
    pub kind: &'static str,
    pub options: Vec<String>,
    pub default: Option<String>,
    pub description: Option<String>,
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeLine {
    pub repo: String,
    pub path: String,
    pub line: u32,
    pub end_line: Option<u32>,
    pub git_ref: String,
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepPreviewStatus {
    Missing,
    Errors,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepPreviewTarget {
    Entry {
        id: String,
        entry_type: Option<EntryType>,
        title: Option<String>,
    },
    Manual {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepPreview {
    pub position: usize,
    pub id: String,
    pub title: String,
    pub target: StepPreviewTarget,
    pub optional: bool,
    pub confirm: bool,
    pub bindings: Vec<(String, String)>,
    pub status: Option<StepPreviewStatus>,
    pub note: Option<String>,
}

pub struct StepTargetInfo {
    pub entry_type: Option<EntryType>,
    pub title: Option<String>,
    pub has_errors: bool,
    pub unreviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PreviewModel {
    pub id: String,
    pub title: String,
    pub path: String,
    pub meta: Option<Meta>,
    pub url: Option<String>,
    pub code: Option<CodeLine>,
    pub payload: Vec<Vec<Span>>,
    pub vars: Vec<VarRow>,
    pub steps: Vec<StepPreview>,
    pub step_issue_counts: Option<(usize, usize, usize)>,
    pub used_in: Vec<String>,
    /// Run mode: the active runs of this runbook (set by the app, not cached).
    pub runs: Vec<String>,
    pub fields: Vec<(String, String)>,
    pub notes: Notes,
    pub mtime: i64,
}

fn kind_name(k: VarKind) -> &'static str {
    match k {
        VarKind::Text => "text",
        VarKind::Number => "number",
        VarKind::Enum => "enum",
        VarKind::Secret => "secret",
    }
}

fn fields_of(entry: &Entry) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut put = |k: &str, v: &Option<String>| {
        if let Some(v) = v {
            out.push((k.to_string(), v.clone()));
        }
    };
    match entry.fields() {
        TypeFields::Sql { dialect, database } => {
            put("dialect", dialect);
            put("database", database);
        }
        TypeFields::Sumo { range } => put("range", range),
        TypeFields::Cw {
            log_groups,
            range,
            region,
        } => {
            if !log_groups.is_empty() {
                put("log groups", &Some(log_groups.join(", ")));
            }
            put("range", range);
            put("region", region);
        }
        TypeFields::Cmd { shell, cwd } => {
            let name = match shell {
                kb_core::entry::Shell::Sh => "sh",
                kb_core::entry::Shell::Bash => "bash",
                kb_core::entry::Shell::Zsh => "zsh",
                kb_core::entry::Shell::Fish => "fish",
            };
            put("shell", &Some(name.to_string()));
            put("cwd", cwd);
        }
        TypeFields::Link { .. }
        | TypeFields::Code { .. }
        | TypeFields::Prompt
        | TypeFields::Note
        | TypeFields::Runbook { .. }
        | TypeFields::Unknown => {}
    }
    out
}

/// Builds the preview. `modified` is the file's mtime in unix seconds (from the search result);
/// `highlight_syntax` is false for `NO_COLOR`.
pub fn build_preview(
    entry: &Entry,
    modified: Option<i64>,
    use_count: u64,
    highlight_syntax: bool,
) -> PreviewModel {
    build_preview_with(entry, modified, use_count, highlight_syntax, None)
}

/// [`build_preview`] with a way to look up the entries a note links to.
pub fn build_preview_with(
    entry: &Entry,
    modified: Option<i64>,
    use_count: u64,
    highlight_syntax: bool,
    lookup: Option<Lookup<'_>>,
) -> PreviewModel {
    build_preview_full(
        entry,
        modified,
        use_count,
        highlight_syntax,
        lookup,
        None,
        Vec::new(),
    )
}

/// Type alias for looking up runbook step target info.
pub type StepLookup<'a> = &'a dyn Fn(&str) -> Option<StepTargetInfo>;

/// Builds preview with full lookup capabilities for runbook steps and backlinks ("used in").
pub fn build_preview_full(
    entry: &Entry,
    modified: Option<i64>,
    use_count: u64,
    highlight_syntax: bool,
    lookup: Option<Lookup<'_>>,
    step_lookup: Option<StepLookup<'_>>,
    used_in: Vec<String>,
) -> PreviewModel {
    let ty = entry.entry_type();
    let edited = entry
        .updated()
        .map(str::to_string)
        .or_else(|| modified.map(|m| utc(m)[..10].to_string()));
    let url = match entry.fields() {
        TypeFields::Link { url } => url.clone(),
        _ => None,
    };
    let code = match entry.fields() {
        TypeFields::Code { code: Some(c) } => Some(CodeLine {
            repo: c.repo.clone(),
            path: c.path.clone(),
            line: c.line,
            end_line: c.end_line,
            git_ref: c.git_ref.clone(),
            symbol: c.symbol.clone(),
        }),
        _ => None,
    };
    let payload = match ty {
        Some(t) if t.has_payload() => {
            highlight(lang_for(t), &payload_pieces(entry), highlight_syntax)
        }
        _ => Vec::new(),
    };
    let vars = entry
        .vars()
        .iter()
        .map(|v| VarRow {
            name: v.name.clone(),
            kind: kind_name(v.kind),
            options: v.options.clone(),
            default: if v.kind == VarKind::Secret {
                None
            } else {
                v.default.clone()
            },
            description: v.description.clone(),
            secret: v.kind == VarKind::Secret,
        })
        .collect();

    let mut steps_preview = Vec::new();
    let mut step_issue_counts = None;

    if let TypeFields::Runbook { steps } = entry.fields() {
        let mut missing_cnt = 0;
        let mut error_cnt = 0;
        let mut agent_cnt = 0;

        for (idx, step) in steps.iter().enumerate() {
            let position = idx + 1;
            let id = kb_core::terminal::display(&step.id).into_owned();
            let title = kb_core::terminal::display(&step.title).into_owned();
            let note = step
                .note
                .as_deref()
                .map(|n| kb_core::terminal::display(n).into_owned());
            let optional = step.optional;
            let confirm = step.confirm;

            let mut bindings = Vec::new();
            for (var_name, binding) in &step.vars {
                let val_str = match binding {
                    kb_core::runbook::StepVarBinding::RunbookVar(rb_var) => {
                        format!("{{{{{rb_var}}}}}")
                    }
                    kb_core::runbook::StepVarBinding::Literal(val) => match val {
                        serde_json::Value::String(s) => s.clone(),
                        _ => val.to_string(),
                    },
                };
                bindings.push((var_name.clone(), val_str));
            }

            let (target, status) = match &step.target {
                kb_core::runbook::StepTarget::Do(text) => {
                    let sanitized_do = kb_core::terminal::display(text).into_owned();
                    (StepPreviewTarget::Manual { text: sanitized_do }, None)
                }
                kb_core::runbook::StepTarget::Entry(target_id) => {
                    let sanitized_id = kb_core::terminal::display(target_id).into_owned();
                    if let Some(resolver) = step_lookup {
                        if let Some(info) = resolver(target_id) {
                            if info.has_errors {
                                error_cnt += 1;
                            }
                            if info.unreviewed {
                                agent_cnt += 1;
                            }
                            let status = if info.has_errors {
                                Some(StepPreviewStatus::Errors)
                            } else if info.unreviewed {
                                Some(StepPreviewStatus::Agent)
                            } else {
                                None
                            };
                            (
                                StepPreviewTarget::Entry {
                                    id: sanitized_id,
                                    entry_type: info.entry_type,
                                    title: info
                                        .title
                                        .map(|t| kb_core::terminal::display(&t).into_owned()),
                                },
                                status,
                            )
                        } else {
                            missing_cnt += 1;
                            (
                                StepPreviewTarget::Entry {
                                    id: sanitized_id,
                                    entry_type: None,
                                    title: None,
                                },
                                Some(StepPreviewStatus::Missing),
                            )
                        }
                    } else {
                        (
                            StepPreviewTarget::Entry {
                                id: sanitized_id,
                                entry_type: None,
                                title: None,
                            },
                            None,
                        )
                    }
                }
            };

            steps_preview.push(StepPreview {
                position,
                id,
                title,
                target,
                optional,
                confirm,
                bindings,
                status,
                note,
            });
        }

        if missing_cnt > 0 || error_cnt > 0 || agent_cnt > 0 {
            step_issue_counts = Some((missing_cnt, error_cnt, agent_cnt));
        }
    }

    PreviewModel {
        id: entry.id().to_string(),
        title: kb_core::terminal::display(entry.title().unwrap_or("")).to_string(),
        path: entry.path().to_string(),
        meta: ty.map(|entry_type| Meta {
            entry_type,
            tags: entry.tags().to_vec(),
            use_count,
            edited,
            unreviewed: false,
        }),
        url,
        code,
        payload,
        vars,
        steps: steps_preview,
        step_issue_counts,
        used_in,
        runs: Vec::new(),
        fields: fields_of(entry),
        notes: Notes::new(render_markdown(
            entry.notes().trim(),
            &Ctx {
                color: highlight_syntax,
                entry_links: ty == Some(EntryType::Note),
                lookup,
            },
        )),
        mtime: modified.unwrap_or(0),
    }
}

impl PreviewModel {
    /// A rough size in bytes, for bounding the cache.
    pub fn approx_bytes(&self) -> usize {
        let payload: usize = self
            .payload
            .iter()
            .map(|l| l.iter().map(|s| s.text.len() + 48).sum::<usize>() + 24)
            .sum();
        let steps_size: usize = self.steps.iter().map(|s| s.title.len() + 64).sum();
        let used_in_size: usize = self.used_in.iter().map(|u| u.len() + 16).sum();
        256 + payload + self.notes.approx_bytes() + steps_size + used_in_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Class;
    use kb_core::entry::parse_entry;

    fn load(dir: &str, id: &str) -> Entry {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(dir)
            .join(format!("{id}.md"));
        parse_entry(id, &format!("{id}.md"), std::fs::read(p).unwrap())
            .entry
            .unwrap()
    }

    fn payload_text(m: &PreviewModel) -> String {
        m.payload
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_sumo_entry_has_payload_meta_and_notes() {
        let e = load("search", "sumo/5xx-by-authorizer");
        let m = build_preview(&e, Some(1_790_000_000), 3, true);
        assert_eq!(m.title, "5xx by authorizer");
        let meta = m.meta.clone().unwrap();
        assert_eq!((meta.entry_type, meta.use_count), (EntryType::Sumo, 3));
        assert_eq!(meta.tags, vec!["auth", "lambda"]);
        assert_eq!(
            meta.edited.as_deref(),
            Some("2026-09-30"),
            "the entry's updated: wins over the file time"
        );
        assert!(payload_text(&m).starts_with("_sourceCategory=prod/api"));
        assert!(m.payload[0].iter().any(|s| s.class == Class::Keyword));
        assert!(!m.notes.is_empty());
    }

    #[test]
    fn edited_falls_back_to_the_file_date() {
        let e = load("search", "sql/orders-by-status");
        let m = build_preview(&e, Some(1_790_000_000), 0, true);
        assert_eq!(m.meta.unwrap().edited.as_deref(), Some("2026-09-21"));
        let m = build_preview(&e, None, 0, true);
        assert_eq!(m.meta.unwrap().edited, None);
    }

    #[test]
    fn variables_secret_and_escaped_braces() {
        let e = load("tui", "cmd/deploy-with-secret");
        let m = build_preview(&e, None, 0, true);
        let names: Vec<&str> = m.vars.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["env", "region", "token"]);
        let env = &m.vars[0];
        assert_eq!((env.kind, env.default.as_deref()), ("enum", Some("stage")));
        assert_eq!(env.options, vec!["prod", "stage"]);
        let region = &m.vars[1];
        assert_eq!(
            region.description.as_deref(),
            Some("AWS region for the rollout")
        );
        let token = &m.vars[2];
        assert!(token.secret && token.default.is_none() && token.kind == "secret");
        let text = payload_text(&m);
        assert!(
            text.contains("{{env}}") && text.contains("{{literal}}"),
            "{text}"
        );
        let marked: Vec<&str> = m
            .payload
            .iter()
            .flatten()
            .filter(|s| s.class == Class::Variable)
            .map(|s| s.text.as_str())
            .collect();
        assert!(marked.contains(&"{{env}}") && marked.contains(&"{{region}}"));
        assert!(!marked.contains(&"{{literal}}"));
        assert!(
            m.fields
                .contains(&("shell".to_string(), "bash".to_string()))
        );
        assert!(m.fields.iter().any(|(k, _)| k == "cwd"));
    }

    #[test]
    fn link_code_and_type_fields() {
        let m = build_preview(&load("tui", "link/long-url"), None, 0, true);
        assert!(
            m.url
                .as_deref()
                .unwrap()
                .starts_with("https://wiki.example.com/engineering")
        );
        assert!(m.payload.is_empty());
        let m = build_preview(&load("tui", "code/with-symbol"), None, 0, true);
        let c = m.code.unwrap();
        assert_eq!(
            (c.repo.as_str(), c.path.as_str(), c.line, c.end_line),
            ("acme/api", "src/retry/handler.rs", 120, Some(168))
        );
        assert_eq!(
            (c.git_ref.as_str(), c.symbol.as_deref()),
            ("main", Some("RetryHandler::handle"))
        );
        let m = build_preview(&load("tui", "sql/multi-dialect"), None, 0, true);
        assert_eq!(
            m.fields,
            vec![
                ("dialect".into(), "postgres".into()),
                ("database".into(), "orders".into())
            ]
        );
        let m = build_preview(&load("tui", "cw/with-fields"), None, 0, true);
        assert!(m.fields.contains(&(
            "log groups".to_string(),
            "/aws/lambda/api, /aws/lambda/worker".to_string()
        )));
        assert!(
            m.fields
                .contains(&("region".to_string(), "eu-west-1".to_string()))
        );
        assert!(m.fields.contains(&("range".to_string(), "-1h".to_string())));
    }

    #[test]
    fn markdown_notes_are_parsed() {
        use crate::tui::markdown::Block;
        let m = build_preview(&load("tui", "sumo/long-query"), None, 0, true);
        assert!(
            m.notes
                .iter()
                .any(|b| matches!(b, Block::List { ordered: false, .. }))
        );
        assert!(
            m.notes
                .iter()
                .any(|b| matches!(b, Block::List { ordered: true, .. }))
        );
        assert!(m.payload.len() >= 60);
    }

    #[test]
    fn no_color_skips_the_grammar_but_keeps_variables() {
        let m = build_preview(&load("tui", "cmd/deploy-with-secret"), None, 0, false);
        assert!(
            m.payload
                .iter()
                .flatten()
                .any(|s| s.class == Class::Variable)
        );
    }

    #[test]
    fn runbook_preview_steps_and_used_in_rendering() {
        use crate::tui::theme::Theme;
        use crate::tui::view::render_preview;

        let runbook_yaml = r#"---
kb_format: 1
type: runbook
title: Deploy authorizer to prod
tags: [deploy, lambda]
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
steps:
  - id: baseline
    title: Check error rate
    entry: sumo/5xx-by-authorizer
    vars: { minutes: 30 }
  - id: deploy
    title: Deploy to env
    entry: cmd/deploy-service
    confirm: true
  - id: rollback
    title: Roll back if p99 high
    do: "Revert the deployment"
    optional: true
---

Runbook notes here.
"#;
        let entry = parse_entry(
            "runbook/deploy-authorizer",
            "runbook/deploy-authorizer.md",
            runbook_yaml.as_bytes().to_vec(),
        )
        .entry
        .unwrap();

        let step_lookup = |target: &str| {
            if target == "sumo/5xx-by-authorizer" {
                Some(StepTargetInfo {
                    entry_type: Some(EntryType::Sumo),
                    title: Some("5xx by authorizer".into()),
                    has_errors: false,
                    unreviewed: true,
                })
            } else if target == "cmd/deploy-service" {
                Some(StepTargetInfo {
                    entry_type: Some(EntryType::Cmd),
                    title: Some("Deploy service".into()),
                    has_errors: true,
                    unreviewed: false,
                })
            } else {
                None
            }
        };

        let model =
            build_preview_full(&entry, None, 1, false, None, Some(&step_lookup), Vec::new());
        assert_eq!(model.steps.len(), 3);
        assert_eq!(model.step_issue_counts, Some((0, 1, 1))); // (missing=0, errors=1, unreviewed=1)

        let buf = render_preview(&model, &Theme::dark(), 100);
        let rendered_text: String = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            rendered_text.contains("steps"),
            "should contain steps divider: {rendered_text}"
        );
        assert!(
            rendered_text.contains("1. [sumo] Check error rate"),
            "{rendered_text}"
        );
        assert!(rendered_text.contains("(minutes=30)"), "{rendered_text}");
        assert!(
            rendered_text.contains("2. [cmd] Deploy to env"),
            "{rendered_text}"
        );
        assert!(rendered_text.contains("[confirm]"), "{rendered_text}");
        assert!(rendered_text.contains("[agent]"), "{rendered_text}");
        assert!(rendered_text.contains("[⚠ errors]"), "{rendered_text}");
        assert!(
            rendered_text.contains("3. [manual] Roll back if p99 high"),
            "{rendered_text}"
        );
        assert!(rendered_text.contains("[optional]"), "{rendered_text}");

        // Now test used_in on an entry
        let sumo_entry = load("search", "sumo/5xx-by-authorizer");
        let used_in = vec![
            "Deploy authorizer to prod".to_string(),
            "Incident response".to_string(),
        ];
        let entry_model = build_preview_full(&sumo_entry, None, 5, false, None, None, used_in);
        assert_eq!(entry_model.used_in.len(), 2);

        let entry_buf = render_preview(&entry_model, &Theme::dark(), 100);
        let entry_text: String = (0..entry_buf.area.height)
            .map(|y| {
                (0..entry_buf.area.width)
                    .map(|x| entry_buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            entry_text.contains("used in"),
            "should contain used in divider: {entry_text}"
        );
        assert!(
            entry_text.contains("• Deploy authorizer to prod"),
            "{entry_text}"
        );
        assert!(entry_text.contains("• Incident response"), "{entry_text}");
    }
}
