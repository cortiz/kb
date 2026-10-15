//! MCP write path: `create_entry` and `update_entry` with validation, secret shielding,
//! format-preserving edits, content-hash concurrency checks, and git commit attribution.

use std::collections::BTreeMap;
use std::fs;

use crate::author::SystemClock;
use crate::author::detect::{Confidence, detect};
use crate::author::draft::{Draft, DraftFields, DraftSource, PathChoice, VarDecl, slugify};
use crate::entry::EntryType;
use crate::lint::Severity;
use crate::mcp::audit::log_mcp_operation;
use crate::mcp::engine::McpEngine;
use crate::mcp::review::ReviewStateTracker;
use crate::mcp::types::*;
use crate::search::KbError;
use crate::usage::content_hash;
use crate::vars::{Brace, VarKind, scan_refs};

pub fn compute_unified_diff(path: &str, old_text: &str, new_text: &str) -> String {
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();
    let n = old_lines.len();
    let m = new_lines.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..n {
        for j in 0..m {
            if old_lines[i] == new_lines[j] {
                dp[i + 1][j + 1] = dp[i][j] + 1;
            } else {
                dp[i + 1][j + 1] = dp[i + 1][j].max(dp[i][j + 1]);
            }
        }
    }
    let mut diff_lines = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old_lines[i - 1] == new_lines[j - 1] {
            diff_lines.push(format!(" {}", old_lines[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            diff_lines.push(format!("+{}", new_lines[j - 1]));
            j -= 1;
        } else if i > 0 {
            diff_lines.push(format!("-{}", old_lines[i - 1]));
            i -= 1;
        }
    }
    diff_lines.reverse();
    let mut out = format!("--- a/{path}\n+++ b/{path}\n@@ -1,{n} +1,{m} @@\n");
    for line in diff_lines {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// `bytes` (a rendered entry) with its `created:` line set to `created`.
fn keep_created(bytes: &[u8], created: &str) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    let Some(end) = text.find("\n---\n").map(|i| i + 1) else {
        return bytes.to_vec();
    };
    let (front, rest) = text.split_at(end);
    let front: Vec<String> = front
        .split('\n')
        .map(|l| {
            if l.starts_with("created:") {
                format!("created: {created}")
            } else {
                l.to_string()
            }
        })
        .collect();
    format!("{}{rest}", front.join("\n")).into_bytes()
}

fn parse_draft_fields(
    entry_type: EntryType,
    fields: &BTreeMap<String, serde_json::Value>,
) -> DraftFields {
    match entry_type {
        EntryType::Sql => DraftFields::Sql {
            dialect: fields
                .get("dialect")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            database: fields
                .get("database")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        },
        EntryType::Sumo => DraftFields::Sumo {
            range: fields
                .get("range")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        },
        EntryType::Cw => DraftFields::Cw {
            log_groups: fields
                .get("log_groups")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            range: fields
                .get("range")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            region: fields
                .get("region")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        },
        EntryType::Cmd => DraftFields::Cmd {
            shell: fields
                .get("shell")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            cwd: fields
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        },
        EntryType::Link => DraftFields::Link {
            url: fields
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        },
        EntryType::Code => DraftFields::Code {
            repo: fields
                .get("repo")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            git_ref: fields
                .get("git_ref")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            path: fields
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            line: fields.get("line").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
            end_line: fields
                .get("end_line")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32),
            symbol: fields
                .get("symbol")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        },
        EntryType::Runbook => {
            let steps = fields
                .get("steps")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            DraftFields::Runbook { steps }
        }
        EntryType::Prompt | EntryType::Note => DraftFields::None,
    }
}

impl<'a> McpEngine<'a> {
    /// Creates a new KB entry via the validated M6 save path with agent attribution.
    pub fn create_entry(
        &mut self,
        req: &CreateEntryRequest,
        client_name: Option<&str>,
    ) -> Result<CreateEntryResponse, KbError> {
        // Determine entry type (explicit, a note when `content` is given, or detected)
        let (entry_type, detection) = if let Some(t_str) = &req.entry_type {
            let ty = EntryType::from_name(t_str)
                .ok_or_else(|| KbError::Unreadable(format!("unknown entry type '{t_str}'")))?;
            let mut det = detect(&req.payload);
            det.ty = ty;
            (ty, det)
        } else if req.content.is_some() {
            let mut det = detect(&req.payload);
            det.ty = EntryType::Note;
            (EntryType::Note, det)
        } else {
            let det = detect(&req.payload);
            if det.confidence == Confidence::Low {
                return Err(KbError::Unreadable(
                    "could not detect entry type with confidence; please specify 'type' explicitly"
                        .into(),
                ));
            }
            (det.ty, det)
        };

        // A note is all Markdown; its title may be the first `# heading`
        let is_note = entry_type == EntryType::Note;
        let is_runbook = entry_type == EntryType::Runbook;
        let note_body = is_note.then(|| {
            req.content
                .clone()
                .or_else(|| req.notes.clone().filter(|n| !n.trim().is_empty()))
                .unwrap_or_else(|| req.payload.clone())
        });
        if is_note && !req.vars.is_empty() {
            return Err(KbError::Unreadable(
                "notes do not support variables (remove 'vars')".into(),
            ));
        }
        if !is_note && !is_runbook && req.payload.is_empty() {
            return Err(KbError::Unreadable("'payload' is required".into()));
        }
        let derived_title = match (&note_body, req.title.trim().is_empty()) {
            (Some(body), true) => crate::entry::note_title_from_body(body),
            _ => None,
        };
        let title = derived_title.as_deref().unwrap_or(req.title.trim());
        if title.is_empty() {
            return Err(KbError::Unreadable(if is_note {
                "a note needs a 'title', or 'content' that starts with a level-1 heading (# Title)"
                    .into()
            } else {
                "title cannot be empty".into()
            }));
        }
        if title.chars().count() > 120 {
            return Err(KbError::Unreadable("title exceeds 120 characters".into()));
        }

        // Check mcp.write_types filter
        let allowed_types = self.settings.mcp_write_types();
        if !allowed_types.is_empty() && !allowed_types.iter().any(|t| t == entry_type.name()) {
            return Err(KbError::Unreadable(format!(
                "entry type '{}' is not permitted by mcp.write_types",
                entry_type.name()
            )));
        }

        // Validate custom path or generate default ID
        let id = if let Some(custom_path) = &req.path {
            let path_str = custom_path.trim().trim_end_matches(".md");
            if path_str.starts_with('/')
                || path_str.contains("..")
                || path_str.split('/').any(|c| c.starts_with('.'))
            {
                return Err(KbError::Unreadable(format!(
                    "invalid custom path '{custom_path}': must be relative with no '..' and no hidden folders"
                )));
            }
            path_str.to_string()
        } else {
            let slug = slugify(title);
            if slug.is_empty() {
                return Err(KbError::Unreadable(
                    "cannot generate a valid path from title; specify 'path' explicitly".into(),
                ));
            }
            self.settings
                .path_template()
                .render(entry_type, &slug, &req.tags)
        };

        // Check if ID is already taken
        if self.kb.id_taken(&id) {
            return Err(KbError::Unreadable(format!(
                "entry '{id}' already exists; use update or specify a different path"
            )));
        }

        // Parse variable declarations and auto-declare undeclared payload placeholders
        let mut vars_map: BTreeMap<String, VarDecl> = BTreeMap::new();
        for (name, decl) in &req.vars {
            let kind = match decl.kind.as_str() {
                "number" => VarKind::Number,
                "enum" | "choice" => VarKind::Enum,
                "secret" => VarKind::Secret,
                _ => VarKind::Text,
            };
            vars_map.insert(
                name.clone(),
                VarDecl {
                    name: name.clone(),
                    kind,
                    default: decl.default.clone(),
                    options: decl.options.clone().unwrap_or_default(),
                },
            );
        }

        // Auto-declaration of undeclared placeholders (a note has none: `{{x}}` is text there)
        for brace in scan_refs(if is_note { "" } else { &req.payload }) {
            if let Brace::Ref { name, .. } = brace
                && !vars_map.contains_key(&name)
            {
                vars_map.insert(
                    name.clone(),
                    VarDecl {
                        name: name.clone(),
                        kind: VarKind::Text,
                        default: None,
                        options: Vec::new(),
                    },
                );
            }
        }

        let draft_vars: Vec<VarDecl> = vars_map.into_values().collect();
        let fields = if is_runbook {
            let steps = req
                .steps
                .clone()
                .or_else(|| req.fields.get("steps").and_then(|v| v.as_array()).cloned())
                .unwrap_or_default();
            if steps.is_empty() {
                return Err(KbError::Unreadable(
                    "a runbook must have at least one step".into(),
                ));
            }
            DraftFields::Runbook { steps }
        } else {
            parse_draft_fields(entry_type, &req.fields)
        };

        let draft = Draft {
            source: DraftSource::Stdin,
            entry_type,
            detection,
            title: title.to_string(),
            tags: req.tags.clone(),
            fields,
            body: if is_note || is_runbook {
                String::new()
            } else {
                req.payload.clone()
            },
            notes: note_body
                .clone()
                .unwrap_or_else(|| req.notes.clone().unwrap_or_default()),
            vars: draft_vars,
            suggestions: Vec::new(),
            path: PathChoice::Fixed(id.clone()),
            path_template: self.settings.path_template(),
            type_chosen: true,
        };

        let mut check = draft.check(&id, &SystemClock);

        if is_runbook {
            let parsed_entry =
                crate::entry::parse_entry(&id, &format!("{id}.md"), check.bytes.clone());
            if let Some(entry) = parsed_entry.entry {
                let rb_findings = crate::runbook::lint::check_runbooks(
                    self.kb.root(),
                    &[entry],
                    &std::collections::HashSet::new(),
                );
                check.findings.extend(rb_findings);
            }
        }

        // Validate findings: any error blocks the write
        let mut finding_views: Vec<FindingView> = Vec::new();
        let mut has_error = false;
        let mut secret_err = None;

        for f in &check.findings {
            let is_sec = (60..=65).contains(&f.code.number());
            if is_sec {
                secret_err = Some(if is_note {
                    format!(
                        "Error: SecretDetected - Note content contains sensitive credentials (rule {} on line {}).",
                        f.code, f.line
                    )
                } else {
                    format!(
                        "refusing to write entry with likely secrets (Rule {} on line {})",
                        f.code, f.line
                    )
                });
            }
            if f.severity == Severity::Error {
                has_error = true;
            }
            finding_views.push(FindingView::from_finding(f));
        }

        if let Some(sec_msg) = secret_err {
            return Err(KbError::Unreadable(sec_msg));
        }
        if has_error {
            let err_msgs: Vec<String> = finding_views
                .iter()
                .filter(|f| f.severity == "error")
                .map(|f| format!("{}: {}", f.rule, f.message))
                .collect();
            return Err(KbError::Unreadable(format!(
                "entry failed validation: {}",
                err_msgs.join("; ")
            )));
        }

        let version = format!("{:016x}", content_hash(&check.bytes));

        // Dry run mode
        if req.dry_run {
            let content_str =
                String::from_utf8(check.bytes).map_err(|e| KbError::Unreadable(e.to_string()))?;
            return Ok(CreateEntryResponse {
                id,
                version,
                findings: finding_views,
                dry_run_content: Some(content_str),
            });
        }

        // Atomic file write & index refresh
        self.kb
            .create_entry(&check)
            .map_err(|e| KbError::Unreadable(e.to_string()))?;

        // Git auto-commit with agent trailers
        if let Some(git) = self.git {
            let is_git = git
                .run_git(self.kb.root(), &["rev-parse", "--is-inside-work-tree"])
                .is_ok_and(|out| out.trim() == "true");

            if is_git && self.settings.git_auto_commit() {
                let mut commit_msg = format!("kb: create {id}");
                commit_msg.push_str("\n\nKb-Source: mcp");
                if let Some(client) = client_name {
                    commit_msg.push_str(&format!("\nKb-Client: {client}"));
                }
                let rel_file = format!("{id}.md");
                let _ = git.run_git(self.kb.root(), &["add", "-A", "--", &rel_file]);
                let _ = git.run_git(
                    self.kb.root(),
                    &["commit", "--only", "-m", &commit_msg, "--", &rel_file],
                );
            }
        }

        // Mark as unreviewed
        let _ = ReviewStateTracker::mark_unreviewed(self.kb.root(), &id);

        // Audit log
        let _ = log_mcp_operation(self.kb.root(), client_name, "create", &id, "ok", None);

        Ok(CreateEntryResponse {
            id,
            version,
            findings: finding_views,
            dry_run_content: None,
        })
    }

    /// Updates an existing KB entry with format-preserving edits, content-hash concurrency check,
    /// and git attribution.
    pub fn update_entry(
        &mut self,
        req: &UpdateEntryRequest,
        client_name: Option<&str>,
    ) -> Result<UpdateEntryResponse, KbError> {
        let entry = self.kb.entry(&req.id)?;
        let entry_type = entry.entry_type().ok_or_else(|| {
            KbError::Unreadable(format!("entry '{}' has missing or invalid type", req.id))
        })?;

        // Check mcp.write_types filter
        let allowed_types = self.settings.mcp_write_types();
        if !allowed_types.is_empty() && !allowed_types.iter().any(|t| t == entry_type.name()) {
            return Err(KbError::Unreadable(format!(
                "entry type '{}' is not permitted by mcp.write_types",
                entry_type.name()
            )));
        }

        // Concurrency check: verify SHA-256 / content hash matches
        let existing_text = entry.source().text();
        let current_version = format!("{:016x}", content_hash(existing_text.as_bytes()));
        if req.version != current_version {
            return Err(KbError::Unreadable(format!(
                "version mismatch: entry has changed on disk (current version: {current_version})"
            )));
        }

        // Construct updated values
        let is_note = entry_type == EntryType::Note;
        let is_runbook = entry_type == EntryType::Runbook;
        if is_note {
            let c = &req.changes;
            if c.payload.is_some() {
                return Err(KbError::Unreadable(
                    "a note has no payload: use 'content' for its Markdown".into(),
                ));
            }
            if c.vars.is_some() || c.set_vars.is_some() || c.remove_vars.is_some() {
                return Err(KbError::Unreadable("notes do not support variables".into()));
            }
        }
        // a note whose title comes from its `# heading` keeps doing so
        let title_is_written = entry
            .tree()
            .get("title")
            .is_some_and(|n| !n.is_null() && n.as_str().is_none_or(|s| !s.trim().is_empty()));
        let new_title = match req.changes.title.as_deref() {
            Some(t) => t,
            None if is_note && !title_is_written => "",
            None => entry.title().unwrap_or(""),
        };
        let new_payload = req
            .changes
            .payload
            .clone()
            .unwrap_or_else(|| entry.payload().map(|p| p.text.clone()).unwrap_or_default());
        let new_notes = if is_note {
            req.changes
                .content
                .clone()
                .or_else(|| req.changes.notes.clone())
                .unwrap_or_else(|| entry.notes().to_string())
        } else {
            req.changes
                .notes
                .clone()
                .unwrap_or_else(|| entry.notes().to_string())
        };

        // Tags mutation: full replacement or add/remove deltas
        let mut tags = entry.tags().to_vec();
        if let Some(full_tags) = &req.changes.tags {
            tags = full_tags.clone();
        } else {
            if let Some(add) = &req.changes.add_tags {
                for t in add {
                    if !tags.contains(t) {
                        tags.push(t.clone());
                    }
                }
            }
            if let Some(rem) = &req.changes.remove_tags {
                tags.retain(|t| !rem.contains(t));
            }
        }

        // Variables mutation: full replacement or set/remove deltas
        let mut vars_map: BTreeMap<String, VarDecl> = BTreeMap::new();
        for v in entry.vars() {
            vars_map.insert(
                v.name.clone(),
                VarDecl {
                    name: v.name.clone(),
                    kind: v.kind,
                    default: v.default.clone(),
                    options: v.options.clone(),
                },
            );
        }

        if let Some(full_vars) = &req.changes.vars {
            vars_map.clear();
            for (name, decl) in full_vars {
                let kind = match decl.kind.as_str() {
                    "number" => VarKind::Number,
                    "enum" | "choice" => VarKind::Enum,
                    "secret" => VarKind::Secret,
                    _ => VarKind::Text,
                };
                vars_map.insert(
                    name.clone(),
                    VarDecl {
                        name: name.clone(),
                        kind,
                        default: decl.default.clone(),
                        options: decl.options.clone().unwrap_or_default(),
                    },
                );
            }
        } else {
            if let Some(set) = &req.changes.set_vars {
                for (name, decl) in set {
                    let kind = match decl.kind.as_str() {
                        "number" => VarKind::Number,
                        "enum" | "choice" => VarKind::Enum,
                        "secret" => VarKind::Secret,
                        _ => VarKind::Text,
                    };
                    vars_map.insert(
                        name.clone(),
                        VarDecl {
                            name: name.clone(),
                            kind,
                            default: decl.default.clone(),
                            options: decl.options.clone().unwrap_or_default(),
                        },
                    );
                }
            }
            if let Some(rem) = &req.changes.remove_vars {
                for r in rem {
                    vars_map.remove(r);
                }
            }
        }

        // Auto-declare undeclared variable placeholders in payload
        for brace in scan_refs(&new_payload) {
            if let Brace::Ref { name, .. } = brace
                && !vars_map.contains_key(&name)
            {
                vars_map.insert(
                    name.clone(),
                    VarDecl {
                        name: name.clone(),
                        kind: VarKind::Text,
                        default: None,
                        options: Vec::new(),
                    },
                );
            }
        }

        let draft_vars: Vec<VarDecl> = vars_map.into_values().collect();

        // Fields: merge changes into existing fields
        let mut fields_map = BTreeMap::new();
        match entry.fields() {
            crate::entry::TypeFields::Sql { dialect, database } => {
                if let Some(d) = dialect {
                    fields_map.insert("dialect".into(), serde_json::Value::String(d.clone()));
                }
                if let Some(db) = database {
                    fields_map.insert("database".into(), serde_json::Value::String(db.clone()));
                }
            }
            crate::entry::TypeFields::Sumo { range } => {
                if let Some(r) = range {
                    fields_map.insert("range".into(), serde_json::Value::String(r.clone()));
                }
            }
            crate::entry::TypeFields::Cw {
                log_groups,
                range,
                region,
            } => {
                fields_map.insert(
                    "log_groups".into(),
                    serde_json::Value::Array(
                        log_groups
                            .iter()
                            .map(|g| serde_json::Value::String(g.clone()))
                            .collect(),
                    ),
                );
                if let Some(r) = range {
                    fields_map.insert("range".into(), serde_json::Value::String(r.clone()));
                }
                if let Some(reg) = region {
                    fields_map.insert("region".into(), serde_json::Value::String(reg.clone()));
                }
            }
            crate::entry::TypeFields::Cmd { shell, cwd } => {
                fields_map.insert(
                    "shell".into(),
                    serde_json::Value::String(match shell {
                        crate::entry::Shell::Sh => "sh".into(),
                        crate::entry::Shell::Bash => "bash".into(),
                        crate::entry::Shell::Zsh => "zsh".into(),
                        crate::entry::Shell::Fish => "fish".into(),
                    }),
                );
                if let Some(c) = cwd {
                    fields_map.insert("cwd".into(), serde_json::Value::String(c.clone()));
                }
            }
            crate::entry::TypeFields::Link { url } => {
                if let Some(u) = url {
                    fields_map.insert("url".into(), serde_json::Value::String(u.clone()));
                }
            }
            crate::entry::TypeFields::Code { code } => {
                if let Some(c) = code {
                    fields_map.insert("repo".into(), serde_json::Value::String(c.repo.clone()));
                    fields_map.insert("path".into(), serde_json::Value::String(c.path.clone()));
                    fields_map.insert(
                        "git_ref".into(),
                        serde_json::Value::String(c.git_ref.clone()),
                    );
                    fields_map.insert("line".into(), serde_json::json!(c.line));
                    if let Some(el) = c.end_line {
                        fields_map.insert("end_line".into(), serde_json::json!(el));
                    }
                    if let Some(s) = &c.symbol {
                        fields_map.insert("symbol".into(), serde_json::Value::String(s.clone()));
                    }
                }
            }
            crate::entry::TypeFields::Prompt
            | crate::entry::TypeFields::Note
            | crate::entry::TypeFields::Unknown => {}
            crate::entry::TypeFields::Runbook { steps } => {
                let step_vals: Vec<serde_json::Value> = steps
                    .iter()
                    .map(|step| {
                        let mut obj = serde_json::Map::new();
                        if step.id_explicit || !step.id.is_empty() {
                            obj.insert("id".into(), serde_json::Value::String(step.id.clone()));
                        }
                        obj.insert(
                            "title".into(),
                            serde_json::Value::String(step.title.clone()),
                        );
                        match &step.target {
                            crate::runbook::StepTarget::Entry(e) => {
                                obj.insert("entry".into(), serde_json::Value::String(e.clone()));
                            }
                            crate::runbook::StepTarget::Do(d) => {
                                obj.insert("do".into(), serde_json::Value::String(d.clone()));
                            }
                        }
                        if let Some(note) = &step.note {
                            obj.insert("note".into(), serde_json::Value::String(note.clone()));
                        }
                        if step.optional {
                            obj.insert("optional".into(), serde_json::Value::Bool(true));
                        }
                        if step.confirm {
                            obj.insert("confirm".into(), serde_json::Value::Bool(true));
                        }
                        if !step.vars.is_empty() {
                            let mut vars_map = serde_json::Map::new();
                            for (k, v) in &step.vars {
                                let val = match v {
                                    crate::runbook::StepVarBinding::RunbookVar(r) => {
                                        serde_json::Value::String(format!("{{{{{}}}}}", r))
                                    }
                                    crate::runbook::StepVarBinding::Literal(lit) => lit.clone(),
                                };
                                vars_map.insert(k.clone(), val);
                            }
                            obj.insert("vars".into(), serde_json::Value::Object(vars_map));
                        }
                        serde_json::Value::Object(obj)
                    })
                    .collect();
                fields_map.insert("steps".into(), serde_json::Value::Array(step_vals));
            }
        }

        if let Some(ch_fields) = &req.changes.fields {
            for (k, v) in ch_fields {
                fields_map.insert(k.clone(), v.clone());
            }
        }
        if let Some(new_steps) = &req.changes.steps {
            fields_map.insert("steps".into(), serde_json::Value::Array(new_steps.clone()));
        }

        let fields = parse_draft_fields(entry_type, &fields_map);

        let update_detection = detect(&new_payload);
        let draft = Draft {
            source: DraftSource::Stdin,
            entry_type,
            detection: update_detection,
            title: new_title.to_string(),
            tags,
            fields,
            body: if is_note || is_runbook {
                String::new()
            } else {
                new_payload
            },
            notes: new_notes,
            vars: draft_vars,
            suggestions: Vec::new(),
            path: PathChoice::Fixed(req.id.clone()),
            path_template: self.settings.path_template(),
            type_chosen: true,
        };

        let mut check = draft.check(&req.id, &SystemClock);
        // an update changes `updated`, not when the entry was made
        if let Some(created) = entry.created() {
            check.bytes = keep_created(&check.bytes, created);
        }

        if is_runbook {
            let parsed_entry =
                crate::entry::parse_entry(&req.id, &format!("{}.md", req.id), check.bytes.clone());
            if let Some(entry) = parsed_entry.entry {
                let rb_findings = crate::runbook::lint::check_runbooks(
                    self.kb.root(),
                    &[entry],
                    &std::collections::HashSet::new(),
                );
                check.findings.extend(rb_findings);
            }
        }

        // Validate findings
        let mut finding_views: Vec<FindingView> = Vec::new();
        let mut has_error = false;
        let mut secret_err = None;

        for f in &check.findings {
            let is_sec = (60..=65).contains(&f.code.number());
            if is_sec {
                secret_err = Some(if is_note {
                    format!(
                        "Error: SecretDetected - Note content contains sensitive credentials (rule {} on line {}).",
                        f.code, f.line
                    )
                } else {
                    format!(
                        "refusing to update entry with likely secrets (Rule {} on line {})",
                        f.code, f.line
                    )
                });
            }
            if f.severity == Severity::Error {
                has_error = true;
            }
            finding_views.push(FindingView::from_finding(f));
        }

        if let Some(sec_msg) = secret_err {
            return Err(KbError::Unreadable(sec_msg));
        }
        if has_error {
            let err_msgs: Vec<String> = finding_views
                .iter()
                .filter(|f| f.severity == "error")
                .map(|f| format!("{}: {}", f.rule, f.message))
                .collect();
            return Err(KbError::Unreadable(format!(
                "entry update failed validation: {}",
                err_msgs.join("; ")
            )));
        }

        let new_text = String::from_utf8(check.bytes.clone())
            .map_err(|e| KbError::Unreadable(e.to_string()))?;
        let new_version = format!("{:016x}", content_hash(new_text.as_bytes()));
        let used_in = self.find_used_in(&req.id);

        // Dry run mode: unified diff
        if req.dry_run {
            let diff = compute_unified_diff(entry.path(), existing_text, &new_text);
            return Ok(UpdateEntryResponse {
                id: req.id.clone(),
                version: new_version,
                findings: finding_views,
                used_in,
                dry_run_diff: Some(diff),
            });
        }

        // Atomic write
        let entry_file = self.kb.root().join(format!("{}.md", req.id));
        let tmp_file = self
            .kb
            .root()
            .join(format!("{}.md.tmp-{}", req.id, std::process::id()));
        fs::write(&tmp_file, &new_text).map_err(|e| KbError::Unreadable(e.to_string()))?;
        fs::rename(&tmp_file, &entry_file).map_err(|e| KbError::Unreadable(e.to_string()))?;

        let _ = self.kb.refresh();

        // Git auto-commit with agent trailers
        if let Some(git) = self.git {
            let is_git = git
                .run_git(self.kb.root(), &["rev-parse", "--is-inside-work-tree"])
                .is_ok_and(|out| out.trim() == "true");

            if is_git && self.settings.git_auto_commit() {
                let mut commit_msg = format!("kb: update {}", req.id);
                commit_msg.push_str("\n\nKb-Source: mcp");
                if let Some(client) = client_name {
                    commit_msg.push_str(&format!("\nKb-Client: {client}"));
                }
                let rel_file = format!("{}.md", req.id);
                let _ = git.run_git(self.kb.root(), &["add", "-A", "--", &rel_file]);
                let _ = git.run_git(
                    self.kb.root(),
                    &["commit", "--only", "-m", &commit_msg, "--", &rel_file],
                );
            }
        }

        // Mark as unreviewed
        let _ = ReviewStateTracker::mark_unreviewed(self.kb.root(), &req.id);

        // Audit log
        let _ = log_mcp_operation(self.kb.root(), client_name, "update", &req.id, "ok", None);

        Ok(UpdateEntryResponse {
            id: req.id.clone(),
            version: new_version,
            findings: finding_views,
            used_in,
            dry_run_diff: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::search::Kb;

    fn temp_kb() -> (tempfile::TempDir, Kb, Settings) {
        let temp = tempfile::tempdir().unwrap();
        let kb = Kb::open(temp.path()).unwrap();
        let settings = Settings::default();
        (temp, kb, settings)
    }

    #[test]
    fn test_create_entry_dry_run_and_commit() {
        let (temp, mut kb, settings) = temp_kb();
        let mut engine = McpEngine::new(&mut kb, &settings);

        let mut req = CreateEntryRequest {
            entry_type: Some("sumo".into()),
            title: "5xx Spike".into(),
            tags: vec!["auth".into()],
            payload: "_sourceCategory={{env}}/api | count by status".into(),
            notes: Some("Check error spike".into()),
            content: None,
            vars: BTreeMap::new(),
            fields: BTreeMap::new(),
            steps: None,
            path: None,
            dry_run: true,
        };

        // 1. Dry run should not write file to disk
        let res = engine.create_entry(&req, Some("test-client")).unwrap();
        assert_eq!(res.id, "sumo/5xx-spike");
        assert!(res.dry_run_content.is_some());
        assert!(!temp.path().join("sumo/5xx-spike.md").exists());

        // 2. Real create should write file and mark unreviewed
        req.dry_run = false;
        let res2 = engine.create_entry(&req, Some("test-client")).unwrap();
        assert_eq!(res2.id, "sumo/5xx-spike");
        assert!(temp.path().join("sumo/5xx-spike.md").exists());
        assert!(ReviewStateTracker::is_unreviewed(
            temp.path(),
            "sumo/5xx-spike"
        ));

        // 3. Creating existing ID should fail
        let err = engine.create_entry(&req, Some("test-client"));
        assert!(err.is_err());
    }

    #[test]
    fn test_create_entry_auto_declares_placeholders_and_rejects_secrets() {
        let (temp, mut kb, settings) = temp_kb();
        let mut engine = McpEngine::new(&mut kb, &settings);

        // 1. Auto-declare undeclared placeholder {{env}} as text
        let req = CreateEntryRequest {
            entry_type: Some("cmd".into()),
            title: "Tail Logs".into(),
            tags: vec!["ops".into()],
            payload: "kubectl logs -f deployment/{{service}}".into(),
            notes: None,
            content: None,
            vars: BTreeMap::new(),
            fields: BTreeMap::new(),
            steps: None,
            path: None,
            dry_run: false,
        };
        let res = engine.create_entry(&req, None).unwrap();
        let file_content = fs::read_to_string(temp.path().join(format!("{}.md", res.id))).unwrap();
        assert!(file_content.contains("service: { kind: text }"));

        // 2. Reject secrets in payload
        let bad_req = CreateEntryRequest {
            entry_type: Some("cmd".into()),
            title: "Leak Secret".into(),
            tags: vec![],
            payload: "export AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            notes: None,
            content: None,
            vars: BTreeMap::new(),
            fields: BTreeMap::new(),
            steps: None,
            path: None,
            dry_run: false,
        };
        let err = engine.create_entry(&bad_req, None);
        assert!(err.is_err());
    }

    #[test]
    fn test_update_entry_version_concurrency_and_dry_run() {
        let (temp, mut kb, settings) = temp_kb();
        let mut engine = McpEngine::new(&mut kb, &settings);

        let create_req = CreateEntryRequest {
            entry_type: Some("sumo".into()),
            title: "Active Users".into(),
            tags: vec!["stats".into()],
            payload: "_sourceCategory=web | count by user".into(),
            notes: Some("Initial notes".into()),
            content: None,
            vars: BTreeMap::new(),
            fields: BTreeMap::new(),
            steps: None,
            path: None,
            dry_run: false,
        };
        let created = engine.create_entry(&create_req, None).unwrap();

        // 1. Update with wrong version hash must fail
        let bad_update = UpdateEntryRequest {
            id: created.id.clone(),
            version: "wrongversionhash".into(),
            changes: ChangeSet {
                notes: Some("New notes".into()),
                content: None,
                ..Default::default()
            },
            dry_run: false,
        };
        let err = engine.update_entry(&bad_update, None);
        assert!(err.is_err());

        // 2. Update with dry_run returns diff and does not write
        let dry_update = UpdateEntryRequest {
            id: created.id.clone(),
            version: created.version.clone(),
            changes: ChangeSet {
                add_tags: Some(vec!["prod".into()]),
                notes: Some("Updated notes".into()),
                content: None,
                ..Default::default()
            },
            dry_run: true,
        };
        let dry_res = engine.update_entry(&dry_update, None).unwrap();
        assert!(dry_res.dry_run_diff.is_some());
        let diff = dry_res.dry_run_diff.unwrap();
        assert!(diff.contains("+Updated notes"));

        // 3. Real update applies changes and returns new version
        let mut real_update = dry_update;
        real_update.dry_run = false;
        let real_res = engine.update_entry(&real_update, None).unwrap();
        assert_ne!(real_res.version, created.version);

        let updated_content =
            fs::read_to_string(temp.path().join(format!("{}.md", created.id))).unwrap();
        assert!(updated_content.contains("Updated notes"));
        assert!(updated_content.contains("prod"));
    }
}
