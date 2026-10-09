//! The preview of one entry, built on the worker and independent of any theme.

use std::collections::{HashMap, VecDeque};

use kb_core::entry::{Entry, EntryType, TypeFields};
use kb_core::payload::payload_pieces;
use kb_core::vars::VarKind;

use super::highlight::{Span, highlight, lang_for};
use super::markdown::{Block, render_notes};
use crate::output::utc;

#[derive(Debug, Clone, PartialEq)]
pub struct Meta {
    pub entry_type: EntryType,
    pub tags: Vec<String>,
    pub use_count: u64,
    /// The entry's `updated:` value, else the file's modification date as `YYYY-MM-DD`.
    pub edited: Option<String>,
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
    pub fields: Vec<(String, String)>,
    pub notes: Vec<Block>,
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
    PreviewModel {
        id: entry.id().to_string(),
        title: entry.title().unwrap_or("").to_string(),
        path: entry.path().to_string(),
        meta: ty.map(|entry_type| Meta {
            entry_type,
            tags: entry.tags().to_vec(),
            use_count,
            edited,
        }),
        url,
        code,
        payload,
        vars,
        fields: fields_of(entry),
        notes: render_notes(entry.notes().trim()),
        mtime: modified.unwrap_or(0),
    }
}

/// Recently built previews, keyed by entry and modification time.
#[derive(Debug, Default)]
pub struct PreviewCache {
    map: HashMap<(String, i64), PreviewModel>,
    order: VecDeque<(String, i64)>,
}

const CACHE_SIZE: usize = 64;

impl PreviewCache {
    pub fn get(&self, id: &str, mtime: i64) -> Option<&PreviewModel> {
        self.map.get(&(id.to_string(), mtime))
    }

    pub fn put(&mut self, model: PreviewModel) {
        let key = (model.id.clone(), model.mtime);
        if self.map.insert(key.clone(), model).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > CACHE_SIZE {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
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
                .all(|s| matches!(s.class, Class::Plain | Class::Variable))
        );
        assert!(
            m.payload
                .iter()
                .flatten()
                .any(|s| s.class == Class::Variable)
        );
    }

    #[test]
    fn the_cache_keys_on_mtime_and_is_bounded() {
        let mut c = PreviewCache::default();
        let model = |id: &str, mtime| PreviewModel {
            id: id.to_string(),
            mtime,
            ..PreviewModel::default()
        };
        c.put(model("a", 1));
        assert!(c.get("a", 1).is_some() && c.get("a", 2).is_none() && c.get("b", 1).is_none());
        for i in 0..100 {
            c.put(model(&format!("e{i}"), 1));
        }
        assert_eq!(c.len(), 64);
        assert!(c.get("a", 1).is_none(), "the oldest entries were dropped");
        assert!(c.get("e99", 1).is_some() && !c.is_empty());
    }
}
