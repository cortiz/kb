//! MCP Query Engine: Search, retrieval, variable resolution, and prompt discovery.

use std::collections::{BTreeMap, HashMap};
use std::time::SystemTime;

use crate::config::Settings;
use crate::console::ConsoleService;
use crate::entry::{Entry, EntryType, TypeFields};
use crate::git::GitRunner;
use crate::mcp::review::ReviewStateTracker;
use crate::mcp::types::*;
use crate::query::Query;
use crate::search::{Kb, KbError};
use crate::usage::content_hash;
use crate::varres::{ResolutionTarget, Target, Value, Values, plan_vars, resolve_for_target};
use crate::vars::VarKind;

fn var_kind_str(k: VarKind) -> &'static str {
    match k {
        VarKind::Text => "text",
        VarKind::Number => "number",
        VarKind::Enum => "enum",
        VarKind::Secret => "secret",
    }
}

pub struct McpEngine<'a> {
    pub kb: &'a mut Kb,
    pub settings: &'a Settings,
    pub git: Option<&'a dyn GitRunner>,
}

impl<'a> McpEngine<'a> {
    pub fn new(kb: &'a mut Kb, settings: &'a Settings) -> Self {
        Self {
            kb,
            settings,
            git: None,
        }
    }

    pub fn with_git(kb: &'a mut Kb, settings: &'a Settings, git: &'a dyn GitRunner) -> Self {
        Self {
            kb,
            settings,
            git: Some(git),
        }
    }

    fn check_unreviewed(&self, id: &str) -> bool {
        if let Some(git) = self.git {
            ReviewStateTracker::is_unreviewed_with_git(self.kb.root(), id, git)
        } else {
            ReviewStateTracker::is_unreviewed(self.kb.root(), id)
        }
    }

    /// Searches the KB index using M2 ranked syntax with type and tag filters.
    pub fn search(
        &mut self,
        query_str: &str,
        types: &[String],
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SearchResultView>, KbError> {
        let limit = if limit == 0 { 20 } else { limit.min(100) };
        let mut query = Query::parse(query_str);
        query.limit = limit;

        for t in types {
            if let Some(et) = EntryType::from_name(t) {
                if !query.types.contains(&et) {
                    query.types.push(et);
                }
            }
        }

        for tag in tags {
            if !query.tags.contains(tag) {
                query.tags.push(tag.clone());
            }
        }

        // Apply MCP exclusions
        for ex_tag in self.settings.mcp_exclude_tags() {
            if !query.not_tags.contains(ex_tag) {
                query.not_tags.push(ex_tag.clone());
            }
        }

        let outcome = self.kb.search(&query, SystemTime::now())?;
        let mut views = Vec::new();

        for res in outcome.results {
            let type_name = res.entry_type.name();
            if self.settings.is_type_excluded(type_name) {
                continue;
            }
            if res.tags.iter().any(|t| self.settings.is_tag_excluded(t)) {
                continue;
            }

            let unreviewed = res.unreviewed;
            views.push(SearchResultView {
                id: res.id,
                entry_type: type_name.to_string(),
                title: res.title,
                tags: res.tags,
                snippet: res.snippet.map(|s| s.text).unwrap_or_default(),
                score: res.score as f32,
                unreviewed,
            });
        }

        Ok(views)
    }

    /// Retrieves detailed entry data, optionally resolving variables and building action links.
    pub fn get(
        &mut self,
        id: &str,
        resolve: bool,
        vars: &HashMap<String, String>,
        _include_code: bool,
        include_full_notes: bool,
    ) -> Result<EntryView, KbError> {
        let entry = self.kb.entry(id)?;
        let type_name = entry.entry_type().map(|t| t.name()).unwrap_or("");
        if self.settings.is_type_excluded(type_name) {
            return Err(KbError::NotFound(id.to_string()));
        }
        if entry.tags().iter().any(|t| self.settings.is_tag_excluded(t)) {
            return Err(KbError::NotFound(id.to_string()));
        }

        if self.settings.mcp_record_usage() {
            let _ = self.kb.record_use(id, SystemTime::now());
        }

        let version = format!("{:016x}", content_hash(entry.source().text().as_bytes()));
        let unreviewed = self.check_unreviewed(id);

        // Variables declared
        let mut vars_map = BTreeMap::new();
        for v in entry.vars() {
            vars_map.insert(
                v.name.clone(),
                VarDeclarationView {
                    kind: var_kind_str(v.kind).to_string(),
                    default: v.default.clone(),
                    options: if v.options.is_empty() {
                        None
                    } else {
                        Some(v.options.clone())
                    },
                    description: v.description.clone(),
                },
            );
        }

        // Type-specific fields
        let mut fields_map = BTreeMap::new();
        match entry.fields() {
            TypeFields::Sql { dialect, database } => {
                if let Some(d) = dialect {
                    fields_map.insert("dialect".into(), serde_json::Value::String(d.clone()));
                }
                if let Some(db) = database {
                    fields_map.insert("database".into(), serde_json::Value::String(db.clone()));
                }
            }
            TypeFields::Sumo { range } => {
                if let Some(r) = range {
                    fields_map.insert("range".into(), serde_json::Value::String(r.clone()));
                }
            }
            TypeFields::Cw {
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
            TypeFields::Cmd { shell, cwd } => {
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
            TypeFields::Link { url } => {
                if let Some(u) = url {
                    fields_map.insert("url".into(), serde_json::Value::String(u.clone()));
                }
            }
            TypeFields::Code { code } => {
                if let Some(c) = code {
                    fields_map.insert(
                        "code".into(),
                        serde_json::json!({
                            "repo": c.repo,
                            "path": c.path,
                            "line": c.line,
                            "git_ref": c.git_ref,
                            "symbol": c.symbol,
                            "end_line": c.end_line,
                            "branch": c.branch,
                        }),
                    );
                }
            }
            TypeFields::Prompt | TypeFields::Unknown => {}
        }

        let payload_text = entry.payload().map(|p| p.text.clone()).unwrap_or_default();
        let mut notes_text = entry.notes().to_string();
        if !include_full_notes && notes_text.len() > 64 * 1024 {
            notes_text.truncate(64 * 1024);
            notes_text.push_str("\n\n[Notes truncated (>64 KB). Pass include_full_notes: true to retrieve complete notes]");
        }

        // Findings
        let parsed = crate::entry::parse_entry(
            id,
            entry.path(),
            entry.source().text().as_bytes().to_vec(),
        );
        let mut findings_view: Vec<FindingView> =
            parsed.findings.iter().map(FindingView::from_finding).collect();

        for v in entry.vars() {
            if v.kind == VarKind::Secret && v.env.is_none() {
                findings_view.push(FindingView {
                    rule: "secret_without_env".to_string(),
                    message: format!(
                        "Secret variable '{}' has no env source; placeholder was retained and must be supplied outside the agent",
                        v.name
                    ),
                    line: 0,
                    severity: "warning".to_string(),
                });
            }
        }

        // Used-in references
        let used_in = self.find_used_in(id);

        let mut resolved_payload = None;
        let mut resolved_fields = None;
        let mut links = None;

        if resolve {
            let (res_payload, res_fields, res_links) = self.resolve_entry(&entry, vars)?;
            resolved_payload = Some(res_payload);
            resolved_fields = Some(res_fields);
            links = Some(res_links);
        }

        Ok(EntryView {
            id: id.to_string(),
            entry_type: type_name.to_string(),
            title: entry.title().unwrap_or("").to_string(),
            tags: entry.tags().to_vec(),
            fields: fields_map,
            payload: payload_text,
            notes: notes_text,
            vars: vars_map,
            version,
            unreviewed,
            used_in,
            findings: findings_view,
            resolved_payload,
            resolved_fields,
            links,
        })
    }

    /// Resolves variables in payload and fields, strictly shielding secrets.
    fn resolve_entry(
        &self,
        entry: &Entry,
        vars: &HashMap<String, String>,
    ) -> Result<(String, BTreeMap<String, serde_json::Value>, EntryLinksView), KbError> {
        let plan = plan_vars(entry);
        let mut values = Values::new();

        // 1. Populate agent-supplied values
        for (k, v) in vars {
            values.insert(k.clone(), Value::new(v.clone(), false));
        }

        // 2. Pre-fill defaults for variables not supplied
        for pv in &plan.vars {
            let var = &pv.var;
            if !values.contains_key(&var.name) {
                if let Some(def) = &var.default {
                    values.insert(var.name.clone(), Value::new(def.clone(), false));
                }
            }
        }

        let resolved = resolve_for_target(&plan, &values, ResolutionTarget::Mcp)
            .map_err(|errors| {
                KbError::Unreadable(errors.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join(", "))
            })?;

        let resolved_payload = resolved
            .get(&Target::Payload)
            .map(|o| o.text.clone())
            .unwrap_or_default();

        let mut resolved_fields = BTreeMap::new();
        let mut console_url = None;
        let mut link_url = None;
        let permalink = None;

        match entry.fields() {
            TypeFields::Sumo { range } => {
                let res_range = resolved.get(&Target::Range).map(|o| o.text.clone()).or_else(|| range.clone());
                if let Some(r) = &res_range {
                    resolved_fields.insert("range".into(), serde_json::Value::String(r.clone()));
                }
                let base_url = self
                    .settings
                    .sumo_base_url()
                    .unwrap_or("https://service.sumologic.com");
                if let Ok(link) = ConsoleService::build_sumo_link(
                    base_url,
                    &resolved_payload,
                    res_range.as_deref(),
                ) {
                    console_url = Some(link.url);
                }
            }
            TypeFields::Cw {
                log_groups,
                range,
                region,
            } => {
                let mut res_log_groups = Vec::new();
                for (i, orig) in log_groups.iter().enumerate() {
                    let g = resolved
                        .get(&Target::LogGroup(i))
                        .map(|o| o.text.clone())
                        .unwrap_or_else(|| orig.clone());
                    res_log_groups.push(g);
                }
                resolved_fields.insert(
                    "log_groups".into(),
                    serde_json::Value::Array(
                        res_log_groups
                            .iter()
                            .map(|g| serde_json::Value::String(g.clone()))
                            .collect(),
                    ),
                );
                let res_range = resolved.get(&Target::Range).map(|o| o.text.clone()).or_else(|| range.clone());
                if let Some(r) = &res_range {
                    resolved_fields.insert("range".into(), serde_json::Value::String(r.clone()));
                }
                let default_reg = self.settings.aws_default_region().unwrap_or("us-east-1");
                let res_reg = region
                    .as_deref()
                    .unwrap_or(default_reg);
                resolved_fields.insert("region".into(), serde_json::Value::String(res_reg.to_string()));

                if let Ok(link) = ConsoleService::build_cw_link(
                    res_reg,
                    &res_log_groups,
                    &resolved_payload,
                    res_range.as_deref(),
                ) {
                    console_url = Some(link.url);
                }
            }
            TypeFields::Link { url } => {
                let res_u = resolved.get(&Target::Url).map(|o| o.text.clone()).or_else(|| url.clone());
                if let Some(u) = res_u {
                    resolved_fields.insert("url".into(), serde_json::Value::String(u.clone()));
                    link_url = Some(u);
                }
            }
            TypeFields::Cmd { shell, cwd } => {
                resolved_fields.insert(
                    "shell".into(),
                    serde_json::Value::String(match shell {
                        crate::entry::Shell::Sh => "sh".into(),
                        crate::entry::Shell::Bash => "bash".into(),
                        crate::entry::Shell::Zsh => "zsh".into(),
                        crate::entry::Shell::Fish => "fish".into(),
                    }),
                );
                let res_c = resolved.get(&Target::Cwd).map(|o| o.text.clone()).or_else(|| cwd.clone());
                if let Some(c) = res_c {
                    resolved_fields.insert("cwd".into(), serde_json::Value::String(c));
                }
            }
            TypeFields::Sql { dialect, database } => {
                if let Some(d) = dialect {
                    resolved_fields.insert("dialect".into(), serde_json::Value::String(d.clone()));
                }
                if let Some(db) = database {
                    resolved_fields.insert("database".into(), serde_json::Value::String(db.clone()));
                }
            }
            TypeFields::Code { code } => {
                if let Some(c) = code {
                    resolved_fields.insert(
                        "code".into(),
                        serde_json::json!({
                            "repo": c.repo,
                            "path": c.path,
                            "line": c.line,
                            "git_ref": c.git_ref,
                            "symbol": c.symbol,
                            "end_line": c.end_line,
                            "branch": c.branch,
                        }),
                    );
                }
            }
            TypeFields::Prompt | TypeFields::Unknown => {}
        }

        let links = EntryLinksView {
            console_url,
            link_url,
            permalink,
        };

        Ok((resolved_payload, resolved_fields, links))
    }

    /// Finds entries referencing this entry ID, respecting exclusion rules.
    pub(crate) fn find_used_in(&mut self, target_id: &str) -> Vec<String> {
        let mut used = Vec::new();
        let query = Query::parse(target_id);
        if let Ok(outcome) = self.kb.search(&query, SystemTime::now()) {
            for res in outcome.results {
                if res.id == target_id {
                    continue;
                }
                if self.settings.is_type_excluded(res.entry_type.name()) {
                    continue;
                }
                if res.tags.iter().any(|t| self.settings.is_tag_excluded(t)) {
                    continue;
                }
                used.push(res.id);
            }
        }
        used.sort();
        used.dedup();
        used
    }

    /// Lists all tags with frequency counts, excluding tags specified in `mcp.exclude_tags`.
    pub fn list_tags(&mut self) -> Result<Vec<(String, usize)>, KbError> {
        let all_tags = self.kb.list_tags()?;
        let mut filtered = Vec::new();
        for (tag, count) in all_tags {
            if !self.settings.is_tag_excluded(&tag) {
                filtered.push((tag, count));
            }
        }
        Ok(filtered)
    }

    /// Lists prompts available through MCP.
    pub fn list_prompts(&mut self) -> Result<Vec<PromptView>, KbError> {
        if !self.settings.mcp_prompts() {
            return Ok(Vec::new());
        }

        let mut query = Query::parse("");
        query.types.push(EntryType::Prompt);
        let outcome = self.kb.search(&query, SystemTime::now())?;

        let mut prompts = Vec::new();
        for res in outcome.results {
            if res.tags.iter().any(|t| self.settings.is_tag_excluded(t)) {
                continue;
            }

            if let Ok(entry) = self.kb.entry(&res.id) {
                // Exclude prompt entries that declare secrets
                if entry.vars().iter().any(|v| v.kind == VarKind::Secret) {
                    continue;
                }

                let mut args = Vec::new();
                for v in entry.vars() {
                    args.push(PromptArgumentView {
                        name: v.name.clone(),
                        description: v.description.clone(),
                        required: v.default.is_none(),
                    });
                }

                let prompt_name = res.id.to_lowercase().replace('/', "-");
                prompts.push(PromptView {
                    name: prompt_name,
                    description: res.title,
                    arguments: args,
                });
            }
        }

        Ok(prompts)
    }

    /// Resolves and returns prompt message text.
    pub fn get_prompt(
        &mut self,
        name: &str,
        args: &HashMap<String, String>,
    ) -> Result<String, KbError> {
        if !self.settings.mcp_prompts() {
            return Err(KbError::NotFound(format!("prompts disabled: {name}")));
        }

        let prompts = self.list_prompts()?;
        let _ = prompts
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| KbError::NotFound(format!("prompt '{name}' not found")))?;

        // Find matching prompt entry ID
        let mut query = Query::parse("");
        query.types.push(EntryType::Prompt);
        let outcome = self.kb.search(&query, SystemTime::now())?;

        for res in outcome.results {
            let prompt_name = res.id.to_lowercase().replace('/', "-");
            if prompt_name == name {
                let entry = self.kb.entry(&res.id)?;
                let (resolved_text, _, _) = self.resolve_entry(&entry, args)?;
                return Ok(resolved_text);
            }
        }

        Err(KbError::NotFound(format!("prompt '{name}' not found")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::search::Kb;
    use std::fs;
    use std::path::Path;

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for e in fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let dest = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy_dir(&e.path(), &dest);
            } else {
                fs::copy(e.path(), dest).unwrap();
            }
        }
    }

    fn fixture_kb() -> (tempfile::TempDir, Kb, Settings) {
        let temp = tempfile::tempdir().unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/search");
        copy_dir(&src, temp.path());
        let kb = Kb::open(temp.path()).unwrap();
        let settings = Settings::default();
        (temp, kb, settings)
    }

    #[test]
    fn test_mcp_engine_search_and_get() {
        let (_temp, mut kb, settings) = fixture_kb();
        let mut engine = McpEngine::new(&mut kb, &settings);

        // Search for authorizer query
        let results = engine.search("authorizer", &[], &[], 10).unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.id == "sumo/5xx-by-authorizer"));

        // Get authorizer entry
        let entry = engine
            .get("sumo/5xx-by-authorizer", false, &HashMap::new(), false, false)
            .unwrap();
        assert_eq!(entry.id, "sumo/5xx-by-authorizer");
        assert_eq!(entry.entry_type, "sumo");
        assert!(entry.payload.contains("500"));
        assert!(!entry.version.is_empty());
    }

    #[test]
    fn test_mcp_engine_list_tags_and_exclusions() {
        let (_temp, mut kb, mut settings) = fixture_kb();
        settings.mcp_exclude_tags_mut().push("lambda".into());
        let mut engine = McpEngine::new(&mut kb, &settings);

        let tags = engine.list_tags().unwrap();
        assert!(!tags.iter().any(|(t, _)| t == "lambda"));

        // Search excluding lambda
        let results = engine.search("authorizer", &[], &[], 10).unwrap();
        assert!(!results.iter().any(|r| r.tags.contains(&"lambda".to_string())));
    }

    #[test]
    fn test_mcp_engine_variable_resolution_and_secrets() {
        let temp = tempfile::tempdir().unwrap();
        let entry_dir = temp.path().join("sumo");
        fs::create_dir_all(&entry_dir).unwrap();
        let entry_file = entry_dir.join("test-query.md");
        fs::write(
            &entry_file,
            "---\nkb_format: 1\ntype: sumo\ntitle: Test query\ntags: [auth]\nvars:\n  env: { kind: choice, options: [dev, prod], default: dev }\n  token: { kind: secret, env: SUMO_TOKEN }\n---\n```\n_sourceCategory={{env}} token={{token}}\n```\n",
        ).unwrap();

        let mut kb = Kb::open(temp.path()).unwrap();
        let settings = Settings::default();
        let mut engine = McpEngine::new(&mut kb, &settings);

        // 1. Resolve with default env -> env=dev, token=$SUMO_TOKEN
        let view = engine.get("sumo/test-query", true, &HashMap::new(), false, false).unwrap();
        let res_payload = view.resolved_payload.unwrap();
        assert_eq!(res_payload.trim(), "_sourceCategory=dev token=$SUMO_TOKEN");

        // 2. Resolve with custom env -> env=prod
        let mut vars = HashMap::new();
        vars.insert("env".into(), "prod".into());
        let view2 = engine.get("sumo/test-query", true, &vars, false, false).unwrap();
        assert_eq!(view2.resolved_payload.unwrap().trim(), "_sourceCategory=prod token=$SUMO_TOKEN");

        // 3. Supplying secret value MUST fail
        let mut bad_vars = HashMap::new();
        bad_vars.insert("token".into(), "supersecret".into());
        let err = engine.get("sumo/test-query", true, &bad_vars, false, false);
        assert!(err.is_err());
    }

    #[test]
    fn test_mcp_engine_list_and_get_prompts() {
        let temp = tempfile::tempdir().unwrap();
        let prompt_dir = temp.path().join("prompt");
        fs::create_dir_all(&prompt_dir).unwrap();

        // 1. Valid prompt with variables
        let valid_prompt = prompt_dir.join("investigate-5xx.md");
        fs::write(
            &valid_prompt,
            "---\nkb_format: 1\ntype: prompt\ntitle: Investigate 5xx spike\ntags: [incident]\nvars:\n  service: { kind: text, description: \"Service name\" }\n  window: { kind: text, default: \"30m\" }\n---\n\n```\nPlease investigate 5xx errors for service {{service}} over the last {{window}}.\n```\n",
        ).unwrap();

        // 2. Secret prompt (declares secret variable -> must be excluded)
        let secret_prompt = prompt_dir.join("secret-prompt.md");
        fs::write(
            &secret_prompt,
            "---\nkb_format: 1\ntype: prompt\ntitle: Secret prompt\nvars:\n  api_key: { kind: secret, env: API_KEY }\n---\n\n```\nPrompt with {{api_key}}.\n```\n",
        ).unwrap();

        // 3. Excluded tag prompt
        let excluded_prompt = prompt_dir.join("internal-only.md");
        fs::write(
            &excluded_prompt,
            "---\nkb_format: 1\ntype: prompt\ntitle: Internal prompt\ntags: [internal]\n---\n\n```\nInternal prompt body.\n```\n",
        ).unwrap();

        let mut kb = Kb::open(temp.path()).unwrap();
        let mut settings = Settings::default();
        settings.mcp_exclude_tags_mut().push("internal".into());

        let mut engine = McpEngine::new(&mut kb, &settings);

        // List prompts
        let prompts = engine.list_prompts().unwrap();
        assert_eq!(prompts.len(), 1);
        let p = &prompts[0];
        assert_eq!(p.name, "prompt-investigate-5xx");
        assert_eq!(p.description, "Investigate 5xx spike");
        assert_eq!(p.arguments.len(), 2);

        let svc_arg = p.arguments.iter().find(|a| a.name == "service").unwrap();
        assert!(svc_arg.required);
        assert_eq!(svc_arg.description.as_deref(), Some("Service name"));

        let win_arg = p.arguments.iter().find(|a| a.name == "window").unwrap();
        assert!(!win_arg.required);

        // Get prompt
        let mut args = HashMap::new();
        args.insert("service".into(), "authorizer".into());
        let res = engine.get_prompt("prompt-investigate-5xx", &args).unwrap();
        assert_eq!(
            res.trim(),
            "Please investigate 5xx errors for service authorizer over the last 30m."
        );

        // Excluded prompt should not be retrievable
        assert!(engine.get_prompt("prompt-secret-prompt", &HashMap::new()).is_err());
        assert!(engine.get_prompt("prompt-internal-only", &HashMap::new()).is_err());

        // When prompts disabled in settings
        let mut disabled_settings = Settings::default();
        disabled_settings.set_text("mcp.prompts", "false");
        let mut disabled_engine = McpEngine::new(&mut kb, &disabled_settings);
        assert!(disabled_engine.list_prompts().unwrap().is_empty());
        assert!(disabled_engine.get_prompt("prompt-investigate-5xx", &args).is_err());
    }
}

