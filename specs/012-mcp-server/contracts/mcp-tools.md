# MCP Tools & Protocol Contract: MCP Server

**Feature**: MCP Server (`012-mcp-server`)  
**Status**: Completed  
**Date**: 2026-10-11  

---

## 1. JSON-RPC Protocol Framing

- JSON-RPC 2.0 messages are transmitted newline-delimited (`\n`) over stdio.
- Server returns protocol version `2024-11-05`.
- Supported methods:
  - `initialize`: Returns server capabilities: `{ "tools": {}, "prompts": {} }`.
  - `notifications/initialized`: Client acknowledgment.
  - `ping`: Health check returning `{}`.
  - `tools/list`: Returns schemas for available tools.
  - `tools/call`: Executes specified tool with parameters.
  - `prompts/list`: Returns available MCP prompts.
  - `prompts/get`: Resolves and returns prompt message text.

---

## 2. Tool Schemas

### 2.1 `search`
Search the KB index using M2 ranked search syntax.

#### Parameters
```json
{
  "type": "object",
  "properties": {
    "query": { "type": "string", "description": "Search query keywords or syntax" },
    "types": { "type": "array", "items": { "type": "string" }, "description": "Filter by entry types" },
    "tags": { "type": "array", "items": { "type": "string" }, "description": "Filter by tags" },
    "limit": { "type": "integer", "default": 20, "maximum": 100, "description": "Maximum results to return" }
  }
}
```

#### Output
Array of ranked matches:
```json
{
  "results": [
    {
      "id": "sumo/5xx-by-authorizer",
      "type": "sumo",
      "title": "5xx by authorizer",
      "tags": ["auth", "errors"],
      "snippet": "_sourceCategory={{env}}/lambda/authorizer | where status >= 500...",
      "score": 0.94,
      "unreviewed": false
    }
  ]
}
```

---

### 2.2 `get`
Retrieve detailed entry data, optionally resolving variables and generating action links.

#### Parameters
```json
{
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
```

#### Secret Safety Rules
- Variables of kind `secret` with an `env` source are resolved to `$ENV_VAR` references (e.g. `"$SUMO_TOKEN"`).
- Secrets lacking `env` retain their `{{placeholder}}` token with a warning.
- Supplying a value for a `secret` variable in `vars` returns an error without echoing the value.

---

### 2.3 `list_tags`
List all tags in the KB with frequency counts.

#### Parameters
```json
{ "type": "object", "properties": {} }
```

#### Output
```json
{
  "tags": [
    { "name": "aws", "count": 14 },
    { "name": "ops", "count": 9 }
  ]
}
```

---

### 2.4 `create` (Write Tool, Opt-in)
Create a new KB entry through the M6 save path.

#### Parameters
```json
{
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
```

---

### 2.5 `update` (Write Tool, Opt-in)
Update an existing entry with optimistic concurrency version check.

#### Parameters
```json
{
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
```
