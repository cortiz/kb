//! Model Context Protocol (MCP) server implementation for kb.
//! Communicates via newline-delimited JSON-RPC 2.0 messages over stdio.

use clap::{Args, Subcommand};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::ExitCode;
use kb_core::config::{Loaded, Settings};
use kb_core::mcp::*;
use kb_core::search::Kb;
use serde::{Deserialize, Serialize};

#[derive(Args, Debug, Clone, Default)]
pub struct McpArgs {
    /// Enable write tools (create and update) for this session, overriding config.
    #[arg(long, conflicts_with = "read_only")]
    pub allow_write: bool,

    /// Force write tools off for this session, overriding config.
    #[arg(long, conflicts_with = "allow_write")]
    pub read_only: bool,

    #[command(subcommand)]
    pub command: Option<McpSubcommand>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum McpSubcommand {
    /// Print configuration snippets to register kb with MCP clients.
    Setup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: serde_json::Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

pub struct McpServer {
    pub root: std::path::PathBuf,
    pub settings: Settings,
    pub write_allowed: bool,
    pub prompts_enabled: bool,
    pub client_name: Option<String>,
    kb: Option<Kb>,
}

fn get_tools_list(write_allowed: bool) -> serde_json::Value {
    let mut tools = vec![
        serde_json::json!({
            "name": "search",
            "description": "Search the knowledge base index using ranked query syntax, type filters, and tag filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query keywords or syntax" },
                    "types": { "type": "array", "items": { "type": "string" }, "description": "Filter by entry types (cmd, sumo, cw, sql, link, code, prompt, runbook)" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Filter by tags" },
                    "limit": { "type": "integer", "default": 20, "maximum": 100, "description": "Maximum results to return" }
                }
            }
        }),
        serde_json::json!({
            "name": "get",
            "description": "Retrieve detailed entry data by ID, optionally resolving declared variables and generating action links.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": {
                    "id": { "type": "string", "description": "Entry ID" },
                    "resolve": { "type": "boolean", "default": false, "description": "Whether to resolve variables and generate links" },
                    "vars": { "type": "object", "description": "Variable key-value assignments for resolution" },
                    "include_code": { "type": "boolean", "default": false, "description": "Include code snippets for code entries" },
                    "include_full_notes": { "type": "boolean", "default": false, "description": "Include full notes without truncation" }
                }
            }
        }),
        serde_json::json!({
            "name": "list_tags",
            "description": "List all tags in the knowledge base with their frequency counts.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
    ];

    if write_allowed {
        tools.push(serde_json::json!({
            "name": "create",
            "description": "Create a new entry in the knowledge base through the validated save path.",
            "inputSchema": {
                "type": "object",
                "required": ["title", "payload"],
                "properties": {
                    "type": { "type": "string", "description": "Entry type (detected if omitted)" },
                    "title": { "type": "string", "description": "Entry title (max 120 chars)" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Entry tags" },
                    "payload": { "type": "string", "description": "Code or query payload" },
                    "notes": { "type": "string", "description": "Markdown body notes" },
                    "vars": { "type": "object", "description": "Variable declarations" },
                    "fields": { "type": "object", "description": "Type-specific fields (url, shell, range, etc.)" },
                    "path": { "type": "string", "description": "Optional custom relative path/ID" },
                    "dry_run": { "type": "boolean", "default": false, "description": "Preview output without saving" }
                }
            }
        }));
        tools.push(serde_json::json!({
            "name": "update",
            "description": "Update an existing entry with format-preserving edits and content-hash concurrency validation.",
            "inputSchema": {
                "type": "object",
                "required": ["id", "version", "changes"],
                "properties": {
                    "id": { "type": "string", "description": "Target entry ID" },
                    "version": { "type": "string", "description": "SHA-256 content hash of file as read" },
                    "changes": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "payload": { "type": "string" },
                            "notes": { "type": "string" },
                            "fields": { "type": "object" },
                            "tags": { "type": "array", "items": { "type": "string" } },
                            "add_tags": { "type": "array", "items": { "type": "string" } },
                            "remove_tags": { "type": "array", "items": { "type": "string" } },
                            "vars": { "type": "object" },
                            "set_vars": { "type": "object" },
                            "remove_vars": { "type": "array", "items": { "type": "string" } }
                        }
                    },
                    "dry_run": { "type": "boolean", "default": false, "description": "Preview diff without saving" }
                }
            }
        }));
    }

    serde_json::json!({ "tools": tools })
}

impl McpServer {
    pub fn new(root: &Path, write_allowed: bool, prompts_enabled: bool) -> Self {
        Self::with_settings(root, Settings::default(), write_allowed, prompts_enabled)
    }

    pub fn with_settings(
        root: &Path,
        settings: Settings,
        write_allowed: bool,
        prompts_enabled: bool,
    ) -> Self {
        Self {
            root: root.to_path_buf(),
            settings,
            write_allowed,
            prompts_enabled,
            client_name: None,
            kb: None,
        }
    }

    pub fn ensure_kb(&mut self) -> Result<(), String> {
        if self.kb.is_none() {
            let kb = Kb::open(&self.root)
                .map_err(|e| format!("failed to open KB at {:?}: {}", self.root, e))?;
            self.kb = Some(kb);
        }
        Ok(())
    }

    pub fn handle_request(&mut self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = req.id;
        match req.method.as_str() {
            "initialize" => {
                let id = id.unwrap_or(serde_json::Value::Null);
                if let Some(params) = &req.params {
                    if let Some(client_info) = params.get("clientInfo") {
                        if let Some(name) = client_info.get("name").and_then(|v| v.as_str()) {
                            self.client_name = Some(name.to_string());
                        }
                    }
                }
                let mut capabilities = serde_json::json!({
                    "tools": {}
                });
                if self.prompts_enabled {
                    capabilities["prompts"] = serde_json::json!({});
                }
                let result = serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": capabilities,
                    "serverInfo": {
                        "name": "kb",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                });
                Some(JsonRpcResponse::success(id, result))
            }
            "notifications/initialized" => {
                // Client acknowledgment notification: no response
                None
            }
            "ping" => {
                let id = id.unwrap_or(serde_json::Value::Null);
                Some(JsonRpcResponse::success(id, serde_json::json!({})))
            }
            "tools/list" => {
                let id = id.unwrap_or(serde_json::Value::Null);
                Some(JsonRpcResponse::success(id, get_tools_list(self.write_allowed)))
            }
            "tools/call" => {
                let id = id.unwrap_or(serde_json::Value::Null);
                let params = match &req.params {
                    Some(p) => p,
                    None => {
                        return Some(JsonRpcResponse::error(
                            id,
                            -32602,
                            "Missing params for tools/call",
                        ));
                    }
                };
                let tool_name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));

                match tool_name {
                    "search" => {
                        let query = arguments
                            .get("query")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let types: Vec<String> = arguments
                            .get("types")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let tags: Vec<String> = arguments
                            .get("tags")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let limit = arguments
                            .get("limit")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(20) as usize;

                        if let Err(e) = self.ensure_kb() {
                            return Some(JsonRpcResponse::success(
                                id,
                                serde_json::json!({
                                    "content": [{ "type": "text", "text": format!("Error opening KB: {e}") }],
                                    "isError": true
                                }),
                            ));
                        }
                        let kb = self.kb.as_mut().unwrap();
                        let mut engine = McpEngine::with_git(kb, &self.settings, &crate::platform::SystemGitRunner);
                        match engine.search(query, &types, &tags, limit) {
                            Ok(results) => {
                                if self.settings.mcp_log_reads() {
                                    let _ = log_mcp_operation(
                                        &self.root,
                                        self.client_name.as_deref(),
                                        "search",
                                        query,
                                        "ok",
                                        None,
                                    );
                                }
                                let data = serde_json::json!({ "results": results });
                                let summary = if results.is_empty() {
                                    format!("No entries found for query '{query}'")
                                } else {
                                    let lines: Vec<String> = results
                                        .iter()
                                        .take(5)
                                        .map(|r| {
                                            format!(
                                                "- {} ({}): {} [score: {:.2}]",
                                                r.id, r.entry_type, r.title, r.score
                                            )
                                        })
                                        .collect();
                                    format!("Found {} entries:\n{}", results.len(), lines.join("\n"))
                                };
                                Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [
                                            { "type": "text", "text": serde_json::to_string_pretty(&data).unwrap() },
                                            { "type": "text", "text": summary }
                                        ],
                                        "isError": false
                                    }),
                                ))
                            }
                            Err(err) => {
                                if self.settings.mcp_log_reads() {
                                    let _ = log_mcp_operation(
                                        &self.root,
                                        self.client_name.as_deref(),
                                        "search",
                                        query,
                                        "error",
                                        Some("search_error"),
                                    );
                                }
                                Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [{ "type": "text", "text": format!("Search error: {err}") }],
                                        "isError": true
                                    }),
                                ))
                            }
                        }
                    }
                    "get" => {
                        let entry_id = match arguments.get("id").and_then(|v| v.as_str()) {
                            Some(id) if !id.trim().is_empty() => id.trim(),
                            _ => {
                                return Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [{ "type": "text", "text": "Error: 'id' parameter is required" }],
                                        "isError": true
                                    }),
                                ));
                            }
                        };
                        let resolve = arguments
                            .get("resolve")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let include_code = arguments
                            .get("include_code")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let include_full_notes = arguments
                            .get("include_full_notes")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        let mut vars_map = HashMap::new();
                        if let Some(obj) = arguments.get("vars").and_then(|v| v.as_object()) {
                            for (k, v) in obj {
                                if let Some(s) = v.as_str() {
                                    vars_map.insert(k.clone(), s.to_string());
                                } else {
                                    vars_map.insert(k.clone(), v.to_string());
                                }
                            }
                        }

                        if let Err(e) = self.ensure_kb() {
                            return Some(JsonRpcResponse::success(
                                id,
                                serde_json::json!({
                                    "content": [{ "type": "text", "text": format!("Error opening KB: {e}") }],
                                    "isError": true
                                }),
                            ));
                        }
                        let kb = self.kb.as_mut().unwrap();
                        let mut engine = McpEngine::with_git(kb, &self.settings, &crate::platform::SystemGitRunner);
                        match engine.get(entry_id, resolve, &vars_map, include_code, include_full_notes) {
                            Ok(view) => {
                                if self.settings.mcp_log_reads() {
                                    let _ = log_mcp_operation(
                                        &self.root,
                                        self.client_name.as_deref(),
                                        "get",
                                        entry_id,
                                        "ok",
                                        None,
                                    );
                                }
                                let data = serde_json::to_value(&view).unwrap();
                                let summary = format!(
                                    "Entry: {}\nType: {}\nTitle: {}\nTags: {}\nVersion: {}",
                                    view.id,
                                    view.entry_type,
                                    view.title,
                                    view.tags.join(", "),
                                    view.version,
                                );
                                Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [
                                            { "type": "text", "text": serde_json::to_string_pretty(&data).unwrap() },
                                            { "type": "text", "text": summary }
                                        ],
                                        "isError": false
                                    }),
                                ))
                            }
                            Err(err) => {
                                if self.settings.mcp_log_reads() {
                                    let _ = log_mcp_operation(
                                        &self.root,
                                        self.client_name.as_deref(),
                                        "get",
                                        entry_id,
                                        "error",
                                        Some("get_error"),
                                    );
                                }
                                Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [{ "type": "text", "text": format!("Get error: {err}") }],
                                        "isError": true
                                    }),
                                ))
                            }
                        }
                    }
                    "list_tags" => {
                        if let Err(e) = self.ensure_kb() {
                            return Some(JsonRpcResponse::success(
                                id,
                                serde_json::json!({
                                    "content": [{ "type": "text", "text": format!("Error opening KB: {e}") }],
                                    "isError": true
                                }),
                            ));
                        }
                        let kb = self.kb.as_mut().unwrap();
                        let mut engine = McpEngine::with_git(kb, &self.settings, &crate::platform::SystemGitRunner);
                        match engine.list_tags() {
                            Ok(tags) => {
                                if self.settings.mcp_log_reads() {
                                    let _ = log_mcp_operation(
                                        &self.root,
                                        self.client_name.as_deref(),
                                        "list_tags",
                                        "",
                                        "ok",
                                        None,
                                    );
                                }
                                let tags_view: Vec<serde_json::Value> = tags
                                    .iter()
                                    .map(|(t, c)| serde_json::json!({ "name": t, "count": c }))
                                    .collect();
                                let data = serde_json::json!({ "tags": tags_view });
                                let summary = format!("Total tags: {}", tags.len());
                                Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [
                                            { "type": "text", "text": serde_json::to_string_pretty(&data).unwrap() },
                                            { "type": "text", "text": summary }
                                        ],
                                        "isError": false
                                    }),
                                ))
                            }
                            Err(err) => {
                                if self.settings.mcp_log_reads() {
                                    let _ = log_mcp_operation(
                                        &self.root,
                                        self.client_name.as_deref(),
                                        "list_tags",
                                        "",
                                        "error",
                                        Some("list_tags_error"),
                                    );
                                }
                                Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [{ "type": "text", "text": format!("List tags error: {err}") }],
                                        "isError": true
                                    }),
                                ))
                            }
                        }
                    }
                    "create" => {
                        if !self.write_allowed {
                            return Some(JsonRpcResponse::success(
                                id,
                                serde_json::json!({
                                    "content": [{
                                        "type": "text",
                                        "text": "Write tools are disabled. Start server with --allow-write or set mcp.write = true in config."
                                    }],
                                    "isError": true
                                }),
                            ));
                        }
                        let create_req: CreateEntryRequest = match serde_json::from_value(arguments) {
                            Ok(r) => r,
                            Err(e) => {
                                return Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [{ "type": "text", "text": format!("Invalid create arguments: {e}") }],
                                        "isError": true
                                    }),
                                ));
                            }
                        };
                        if let Err(e) = self.ensure_kb() {
                            return Some(JsonRpcResponse::success(
                                id,
                                serde_json::json!({
                                    "content": [{ "type": "text", "text": format!("Error opening KB: {e}") }],
                                    "isError": true
                                }),
                            ));
                        }
                        let kb = self.kb.as_mut().unwrap();
                        let mut engine = McpEngine::with_git(kb, &self.settings, &crate::platform::SystemGitRunner);
                        match engine.create_entry(&create_req, self.client_name.as_deref()) {
                            Ok(resp) => {
                                let data = serde_json::to_value(&resp).unwrap();
                                let summary = if let Some(preview) = &resp.dry_run_content {
                                    format!("Dry run preview for {}:\n{}", resp.id, preview)
                                } else {
                                    format!("Created entry '{}' (version: {})", resp.id, resp.version)
                                };
                                Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [
                                            { "type": "text", "text": serde_json::to_string_pretty(&data).unwrap() },
                                            { "type": "text", "text": summary }
                                        ],
                                        "isError": false
                                    }),
                                ))
                            }
                            Err(err) => {
                                let _ = log_mcp_operation(
                                    &self.root,
                                    self.client_name.as_deref(),
                                    "create",
                                    create_req.path.as_deref().unwrap_or(""),
                                    "error",
                                    Some("create_error"),
                                );
                                Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [{ "type": "text", "text": format!("Create error: {err}") }],
                                        "isError": true
                                    }),
                                ))
                            }
                        }
                    }
                    "update" => {
                        if !self.write_allowed {
                            return Some(JsonRpcResponse::success(
                                id,
                                serde_json::json!({
                                    "content": [{
                                        "type": "text",
                                        "text": "Write tools are disabled. Start server with --allow-write or set mcp.write = true in config."
                                    }],
                                    "isError": true
                                }),
                            ));
                        }
                        let update_req: UpdateEntryRequest = match serde_json::from_value(arguments) {
                            Ok(r) => r,
                            Err(e) => {
                                return Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [{ "type": "text", "text": format!("Invalid update arguments: {e}") }],
                                        "isError": true
                                    }),
                                ));
                            }
                        };
                        if let Err(e) = self.ensure_kb() {
                            return Some(JsonRpcResponse::success(
                                id,
                                serde_json::json!({
                                    "content": [{ "type": "text", "text": format!("Error opening KB: {e}") }],
                                    "isError": true
                                }),
                            ));
                        }
                        let kb = self.kb.as_mut().unwrap();
                        let mut engine = McpEngine::with_git(kb, &self.settings, &crate::platform::SystemGitRunner);
                        match engine.update_entry(&update_req, self.client_name.as_deref()) {
                            Ok(resp) => {
                                let data = serde_json::to_value(&resp).unwrap();
                                let summary = if let Some(diff) = &resp.dry_run_diff {
                                    format!("Dry run diff for {}:\n{}", resp.id, diff)
                                } else {
                                    let mut s = format!("Updated entry '{}' (version: {})", resp.id, resp.version);
                                    if !resp.used_in.is_empty() {
                                        s.push_str(&format!("\nWarning: used in runbooks: {}", resp.used_in.join(", ")));
                                    }
                                    s
                                };
                                Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [
                                            { "type": "text", "text": serde_json::to_string_pretty(&data).unwrap() },
                                            { "type": "text", "text": summary }
                                        ],
                                        "isError": false
                                    }),
                                ))
                            }
                            Err(err) => {
                                let _ = log_mcp_operation(
                                    &self.root,
                                    self.client_name.as_deref(),
                                    "update",
                                    &update_req.id,
                                    "error",
                                    Some("update_error"),
                                );
                                Some(JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({
                                        "content": [{ "type": "text", "text": format!("Update error: {err}") }],
                                        "isError": true
                                    }),
                                ))
                            }
                        }
                    }
                    _ => {
                        Some(JsonRpcResponse::success(
                            id,
                            serde_json::json!({
                                "content": [{ "type": "text", "text": format!("Unknown tool: {tool_name}") }],
                                "isError": true
                            }),
                        ))
                    }
                }
            }
            "prompts/list" => {
                let id = id.unwrap_or(serde_json::Value::Null);
                if !self.prompts_enabled {
                    return Some(JsonRpcResponse::error(
                        id,
                        -32601,
                        "Prompts capability is disabled",
                    ));
                }
                if let Err(e) = self.ensure_kb() {
                    return Some(JsonRpcResponse::error(
                        id,
                        -32603,
                        format!("Error opening KB: {e}"),
                    ));
                }
                let kb = self.kb.as_mut().unwrap();
                let mut engine = McpEngine::with_git(kb, &self.settings, &crate::platform::SystemGitRunner);
                match engine.list_prompts() {
                    Ok(prompts) => {
                        let prompts_json: Vec<serde_json::Value> = prompts
                            .iter()
                            .map(|p| {
                                let args: Vec<serde_json::Value> = p
                                    .arguments
                                    .iter()
                                    .map(|a| {
                                        serde_json::json!({
                                            "name": a.name,
                                            "description": a.description,
                                            "required": a.required,
                                        })
                                    })
                                    .collect();
                                serde_json::json!({
                                    "name": p.name,
                                    "description": p.description,
                                    "arguments": args,
                                })
                            })
                            .collect();
                        Some(JsonRpcResponse::success(
                            id,
                            serde_json::json!({ "prompts": prompts_json }),
                        ))
                    }
                    Err(err) => Some(JsonRpcResponse::error(
                        id,
                        -32603,
                        format!("Failed to list prompts: {err}"),
                    )),
                }
            }
            "prompts/get" => {
                let id = id.unwrap_or(serde_json::Value::Null);
                if !self.prompts_enabled {
                    return Some(JsonRpcResponse::error(
                        id,
                        -32601,
                        "Prompts capability is disabled",
                    ));
                }
                let params = match &req.params {
                    Some(p) => p,
                    None => {
                        return Some(JsonRpcResponse::error(
                            id,
                            -32602,
                            "Missing params for prompts/get",
                        ));
                    }
                };
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let mut args_map = HashMap::new();
                if let Some(args_obj) = params.get("arguments").and_then(|v| v.as_object()) {
                    for (k, v) in args_obj {
                        if let Some(s) = v.as_str() {
                            args_map.insert(k.clone(), s.to_string());
                        } else {
                            args_map.insert(k.clone(), v.to_string());
                        }
                    }
                }
                if let Err(e) = self.ensure_kb() {
                    return Some(JsonRpcResponse::error(
                        id,
                        -32603,
                        format!("Error opening KB: {e}"),
                    ));
                }
                let kb = self.kb.as_mut().unwrap();
                let mut engine = McpEngine::with_git(kb, &self.settings, &crate::platform::SystemGitRunner);
                match engine.get_prompt(name, &args_map) {
                    Ok(resolved_text) => Some(JsonRpcResponse::success(
                        id,
                        serde_json::json!({
                            "description": format!("Prompt: {name}"),
                            "messages": [
                                {
                                    "role": "user",
                                    "content": {
                                        "type": "text",
                                        "text": resolved_text
                                    }
                                }
                            ]
                        }),
                    )),
                    Err(err) => Some(JsonRpcResponse::error(
                        id,
                        -32602,
                        format!("Prompt error: {err}"),
                    )),
                }
            }
            _ => {
                if let Some(id) = id {
                    Some(JsonRpcResponse::error(
                        id,
                        -32601,
                        format!("Method not found: {}", req.method),
                    ))
                } else {
                    None
                }
            }
        }
    }
}

pub fn serve_stdio<R: BufRead, W: Write>(
    reader: R,
    mut writer: W,
    server: &mut McpServer,
) -> std::io::Result<()> {
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Result<JsonRpcRequest, _> = serde_json::from_str(trimmed);
        match req {
            Ok(request) => {
                if let Some(response) = server.handle_request(request) {
                    let out = serde_json::to_string(&response)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                    writeln!(writer, "{}", out)?;
                    writer.flush()?;
                }
            }
            Err(e) => {
                let resp = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32700,
                    format!("Parse error: {e}"),
                );
                let out = serde_json::to_string(&resp)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                writeln!(writer, "{}", out)?;
                writer.flush()?;
            }
        }
    }
    Ok(())
}

pub fn run_mcp_setup(_root: &Path) -> ExitCode {
    println!("To register kb with Claude Code:");
    println!("  claude mcp add kb -- kb mcp");
    println!();
    println!("To allow writing (create and edit entries):");
    println!("  claude mcp add kb -- kb mcp --allow-write");
    println!();
    println!("Generic MCP configuration (e.g. claude_desktop_config.json):");
    println!(
        "{}",
        "{\n  \"mcpServers\": {\n    \"kb\": {\n      \"command\": \"kb\",\n      \"args\": [\"mcp\"]\n    }\n  }\n}"
    );
    ExitCode::SUCCESS
}

pub fn run_mcp(root: &Path, loaded: &Loaded, args: &McpArgs) -> ExitCode {
    if let Some(McpSubcommand::Setup) = &args.command {
        return run_mcp_setup(root);
    }

    let write_allowed = if args.allow_write {
        true
    } else if args.read_only {
        false
    } else {
        loaded.settings.mcp_write()
    };

    let prompts_enabled = loaded.settings.mcp_prompts();

    let mut server = McpServer::with_settings(root, loaded.settings.clone(), write_allowed, prompts_enabled);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    if let Err(e) = serve_stdio(stdin.lock(), stdout.lock(), &mut server) {
        eprintln!("kb mcp error: {e}");
        return ExitCode::from(2);
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_initialize_and_ping_handshake() {
        let temp = tempfile::tempdir().unwrap();
        let mut server = McpServer::new(temp.path(), false, true);

        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"clientInfo\":{\"name\":\"test-client\"}}}\n\
                     {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
                     {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";

        let mut output = Vec::new();
        serve_stdio(Cursor::new(input), &mut output, &mut server).unwrap();

        let out_str = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = out_str.lines().collect();
        assert_eq!(lines.len(), 2);

        let init_resp: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(init_resp["id"], 1);
        assert_eq!(init_resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(init_resp["result"]["serverInfo"]["name"], "kb");
        assert_eq!(server.client_name.as_deref(), Some("test-client"));

        let ping_resp: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(ping_resp["id"], 2);
        assert_eq!(ping_resp["result"], serde_json::json!({}));
    }

    #[test]
    fn test_parse_error_and_method_not_found() {
        let temp = tempfile::tempdir().unwrap();
        let mut server = McpServer::new(temp.path(), false, true);

        let input = "{invalid json}\n{\"jsonrpc\":\"2.0\",\"id\":99,\"method\":\"nonexistent\"}\n";
        let mut output = Vec::new();
        serve_stdio(Cursor::new(input), &mut output, &mut server).unwrap();

        let out_str = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = out_str.lines().collect();
        assert_eq!(lines.len(), 2);

        let parse_err: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parse_err["error"]["code"], -32700);

        let not_found: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(not_found["id"], 99);
        assert_eq!(not_found["error"]["code"], -32601);
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let dest = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy_dir(&e.path(), &dest);
            } else {
                std::fs::copy(e.path(), dest).unwrap();
            }
        }
    }

    #[test]
    fn test_tools_list_and_call_read_tools() {
        let temp = tempfile::tempdir().unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/search");
        copy_dir(&src, temp.path());

        // 1. Read-only server tools/list
        let mut server = McpServer::new(temp.path(), false, false);
        let list_req = "{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"tools/list\"}\n";
        let mut output = Vec::new();
        serve_stdio(Cursor::new(list_req), &mut output, &mut server).unwrap();
        let out_str = String::from_utf8(output).unwrap();
        let list_resp: serde_json::Value = serde_json::from_str(out_str.trim()).unwrap();
        let tools = list_resp["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"search"));
        assert!(names.contains(&"get"));
        assert!(names.contains(&"list_tags"));
        assert!(!names.contains(&"create"));
        assert!(!names.contains(&"update"));

        // 2. Server with write_allowed = true includes create & update
        let mut write_server = McpServer::new(temp.path(), true, false);
        let mut output2 = Vec::new();
        serve_stdio(Cursor::new(list_req), &mut output2, &mut write_server).unwrap();
        let out_str2 = String::from_utf8(output2).unwrap();
        let list_resp2: serde_json::Value = serde_json::from_str(out_str2.trim()).unwrap();
        let tools2 = list_resp2["result"]["tools"].as_array().unwrap();
        let names2: Vec<&str> = tools2.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names2.contains(&"create"));
        assert!(names2.contains(&"update"));

        // 3. Call search tool
        let search_call = "{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"tools/call\",\"params\":{\"name\":\"search\",\"arguments\":{\"query\":\"authorizer\"}}}\n";
        let mut output3 = Vec::new();
        serve_stdio(Cursor::new(search_call), &mut output3, &mut server).unwrap();
        let out_str3 = String::from_utf8(output3).unwrap();
        let search_resp: serde_json::Value = serde_json::from_str(out_str3.trim()).unwrap();
        assert_eq!(search_resp["id"], 11);
        let content_text = search_resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed_content: serde_json::Value = serde_json::from_str(content_text).unwrap();
        let results = parsed_content["results"].as_array().unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r["id"] == "sumo/5xx-by-authorizer"));

        // 4. Call get tool
        let get_call = "{\"jsonrpc\":\"2.0\",\"id\":12,\"method\":\"tools/call\",\"params\":{\"name\":\"get\",\"arguments\":{\"id\":\"sumo/5xx-by-authorizer\",\"resolve\":true}}}\n";
        let mut output4 = Vec::new();
        serve_stdio(Cursor::new(get_call), &mut output4, &mut server).unwrap();
        let out_str4 = String::from_utf8(output4).unwrap();
        let get_resp: serde_json::Value = serde_json::from_str(out_str4.trim()).unwrap();
        assert_eq!(get_resp["id"], 12);
        let get_content = get_resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed_get: serde_json::Value = serde_json::from_str(get_content).unwrap();
        assert_eq!(parsed_get["id"], "sumo/5xx-by-authorizer");
        assert_eq!(parsed_get["type"], "sumo");

        // 5. Call list_tags tool
        let tags_call = "{\"jsonrpc\":\"2.0\",\"id\":13,\"method\":\"tools/call\",\"params\":{\"name\":\"list_tags\",\"arguments\":{}}}\n";
        let mut output5 = Vec::new();
        serve_stdio(Cursor::new(tags_call), &mut output5, &mut server).unwrap();
        let out_str5 = String::from_utf8(output5).unwrap();
        let tags_resp: serde_json::Value = serde_json::from_str(out_str5.trim()).unwrap();
        assert_eq!(tags_resp["id"], 13);
        let tags_content = tags_resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed_tags: serde_json::Value = serde_json::from_str(tags_content).unwrap();
        let tags_arr = parsed_tags["tags"].as_array().unwrap();
        assert!(!tags_arr.is_empty());
    }

    #[test]
    fn test_tools_call_create_and_update() {
        let temp = tempfile::tempdir().unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/search");
        copy_dir(&src, temp.path());

        // 1. Call create on read-only server fails
        let mut ro_server = McpServer::new(temp.path(), false, false);
        let create_call = "{\"jsonrpc\":\"2.0\",\"id\":20,\"method\":\"tools/call\",\"params\":{\"name\":\"create\",\"arguments\":{\"title\":\"Test Entry\",\"payload\":\"echo 123\"}}}\n";
        let mut out = Vec::new();
        serve_stdio(Cursor::new(create_call), &mut out, &mut ro_server).unwrap();
        let resp: serde_json::Value = serde_json::from_str(String::from_utf8(out).unwrap().trim()).unwrap();
        assert_eq!(resp["result"]["isError"], true);
        assert!(resp["result"]["content"][0]["text"].as_str().unwrap().contains("Write tools are disabled"));

        // 2. Call create on write_allowed server with dry_run
        let mut write_server = McpServer::new(temp.path(), true, false);
        let create_dry = "{\"jsonrpc\":\"2.0\",\"id\":21,\"method\":\"tools/call\",\"params\":{\"name\":\"create\",\"arguments\":{\"title\":\"Dry Run Cmd\",\"type\":\"cmd\",\"payload\":\"git status\",\"dry_run\":true}}}\n";
        let mut out2 = Vec::new();
        serve_stdio(Cursor::new(create_dry), &mut out2, &mut write_server).unwrap();
        let resp2: serde_json::Value = serde_json::from_str(String::from_utf8(out2).unwrap().trim()).unwrap();
        assert_eq!(resp2["result"]["isError"], false);
        let dry_data: serde_json::Value = serde_json::from_str(resp2["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(dry_data["dry_run_content"].is_string());

        // 3. Call create with actual save
        let create_real = "{\"jsonrpc\":\"2.0\",\"id\":22,\"method\":\"tools/call\",\"params\":{\"name\":\"create\",\"arguments\":{\"title\":\"New Created Cmd\",\"type\":\"cmd\",\"payload\":\"echo hello\",\"path\":\"cmd/new-test-cmd\"}}}\n";
        let mut out3 = Vec::new();
        serve_stdio(Cursor::new(create_real), &mut out3, &mut write_server).unwrap();
        let resp3: serde_json::Value = serde_json::from_str(String::from_utf8(out3).unwrap().trim()).unwrap();
        assert_eq!(resp3["result"]["isError"], false);
        let created_data: serde_json::Value = serde_json::from_str(resp3["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(created_data["id"], "cmd/new-test-cmd");
        let initial_version = created_data["version"].as_str().unwrap().to_string();

        // 4. Call update with wrong version fails
        let update_bad_ver = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 23,
            "method": "tools/call",
            "params": {
                "name": "update",
                "arguments": {
                    "id": "cmd/new-test-cmd",
                    "version": "badhash",
                    "changes": {
                        "title": "Changed"
                    }
                }
            }
        })).unwrap() + "\n";
        let mut out4 = Vec::new();
        serve_stdio(Cursor::new(update_bad_ver), &mut out4, &mut write_server).unwrap();
        let resp4: serde_json::Value = serde_json::from_str(String::from_utf8(out4).unwrap().trim()).unwrap();
        assert_eq!(resp4["result"]["isError"], true);
        assert!(resp4["result"]["content"][0]["text"].as_str().unwrap().contains("version mismatch"));

        // 5. Call update with dry_run
        let update_dry = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 24,
            "method": "tools/call",
            "params": {
                "name": "update",
                "arguments": {
                    "id": "cmd/new-test-cmd",
                    "version": initial_version,
                    "dry_run": true,
                    "changes": {
                        "title": "Updated Title"
                    }
                }
            }
        })).unwrap() + "\n";
        let mut out5 = Vec::new();
        serve_stdio(Cursor::new(update_dry), &mut out5, &mut write_server).unwrap();
        let resp5: serde_json::Value = serde_json::from_str(String::from_utf8(out5).unwrap().trim()).unwrap();
        assert_eq!(resp5["result"]["isError"], false);
        let dry_up_data: serde_json::Value = serde_json::from_str(resp5["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(dry_up_data["dry_run_diff"].is_string());
    }
}
