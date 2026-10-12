# Data Model: MCP Server

**Feature**: MCP Server (`012-mcp-server`)  
**Status**: Completed  
**Date**: 2026-10-11  

---

## 1. Entities

### 1.1 EntryView (MCP Representation of a KB Entry)

Returned by `get` and in truncated form by `search`:

```rust
pub struct EntryView {
    pub id: String,
    pub entry_type: String,
    pub title: String,
    pub tags: Vec<String>,
    pub fields: BTreeMap<String, serde_json::Value>,
    pub payload: String,
    pub notes: String,
    pub vars: BTreeMap<String, VarDeclarationView>,
    pub version: String,
    pub unreviewed: bool,
    pub used_in: Vec<String>,
    pub findings: Vec<FindingView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_payload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_fields: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<EntryLinksView>,
}

pub struct VarDeclarationView {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub struct EntryLinksView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
}
```

### 1.2 ChangeSet (Update Mutations)

Input to the `update` MCP tool:

```rust
pub struct ChangeSet {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub payload: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub fields: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,             // Full tag replacement
    #[serde(default)]
    pub add_tags: Option<Vec<String>>,         // Incremental tag additions
    #[serde(default)]
    pub remove_tags: Option<Vec<String>>,      // Incremental tag removals
    #[serde(default)]
    pub vars: Option<BTreeMap<String, VarDeclarationView>>, // Full vars replacement
    #[serde(default)]
    pub set_vars: Option<BTreeMap<String, VarDeclarationView>>, // Incremental var additions/updates
    #[serde(default)]
    pub remove_vars: Option<Vec<String>>,      // Incremental var removals
}
```

### 1.3 ReviewState & Git Trailers

Represents review status of an entry:

```rust
pub struct ReviewState {
    pub id: String,
    pub unreviewed: bool,
    pub last_mcp_commit: Option<String>,
    pub last_reviewed_commit: Option<String>,
}
```

Git trailer specifications:
- Write by agent:
  ```text
  Kb-Source: mcp
  Kb-Client: <client-name>
  ```
- Review approval commit:
  ```text
  kb: review <id>

  Kb-Reviewed: <commit-sha>
  ```

### 1.4 AuditEntry (Audit Log)

Serialized into `.kb/mcp.log`:

```rust
pub struct AuditEntry {
    pub timestamp: String, // ISO-8601 UTC
    pub client: String,
    pub tool: String,
    pub entry_id: String,
    pub outcome: String,   // "ok" or "error"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}
```

---

## 2. Configuration Schema (`[mcp]`)

Added to `crates/kb-core/src/config.rs`:

| Key | Type | Default | Environment | Description |
|---|---|---|---|---|
| `mcp.write` | `bool` | `false` | `KB_MCP_WRITE` | Enable agent write tools (`create`, `update`). |
| `mcp.prompts` | `bool` | `true` | `KB_MCP_PROMPTS` | Offer `prompt` entries as MCP prompts. |
| `mcp.record_usage` | `bool` | `false` | `KB_MCP_RECORD_USAGE` | Whether agent tool calls update entry frecency usage. |
| `mcp.use_remembered`| `bool` | `false` | `KB_MCP_USE_REMEMBERED` | Whether to use remembered variable values as pre-fills for agents. |
| `mcp.exclude_tags` | `list` | `["private"]` | `KB_MCP_EXCLUDE_TAGS` | Tags excluded from all agent queries and prompts. |
| `mcp.exclude_types`| `list` | `[]` | `KB_MCP_EXCLUDE_TYPES`| Entry types completely hidden from MCP. |
| `mcp.write_types`  | `list` | `["sql", "sumo", "cw", "cmd", "prompt", "link", "code"]` | `KB_MCP_WRITE_TYPES` | Entry types the agent is permitted to write. |
| `mcp.log_reads`    | `bool` | `false` | `KB_MCP_LOG_READS` | Append search and get queries to `.kb/mcp.log`. |
