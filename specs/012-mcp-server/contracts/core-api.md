# Core API Contract: MCP Server

**Feature**: MCP Server (`012-mcp-server`)  
**Status**: Completed  
**Date**: 2026-10-11  

---

## 1. Core Module: `kb_core::mcp`

The `kb_core::mcp` module provides protocol-agnostic querying, entry viewing, writing, and review state management.

### 1.1 Review State Engine
```rust
pub struct ReviewStateTracker;

impl ReviewStateTracker {
    /// Determines whether an entry is unreviewed (via git trailers or local state cache).
    pub fn is_unreviewed(root: &Path, id: &str) -> bool;

    /// Returns all currently unreviewed entry IDs.
    pub fn unreviewed_entries(root: &Path) -> Vec<String>;

    /// Records an approval for the given entry ID (creates git review commit or updates state).
    pub fn approve(root: &Path, id: &str) -> std::io::Result<()>;

    /// Reverts an entry to its last approved git revision.
    pub fn revert(root: &Path, id: &str) -> std::io::Result<()>;

    /// Computes the unified diff between the current entry and last approved version.
    pub fn review_diff(root: &Path, id: &str) -> std::io::Result<String>;
}
```

### 1.2 MCP Query & View Engine
```rust
pub struct McpEngine<'a> {
    kb: &'a mut Kb,
    config: &'a Config,
}

impl<'a> McpEngine<'a> {
    pub fn search(&mut self, query: &str, types: &[String], tags: &[String], limit: usize) -> Result<Vec<SearchResultView>, KbError>;

    pub fn get(&mut self, id: &str, resolve: bool, vars: &HashMap<String, String>, include_code: bool) -> Result<EntryView, KbError>;

    pub fn list_tags(&self) -> Result<Vec<(String, usize)>, KbError>;

    pub fn list_prompts(&self) -> Result<Vec<PromptView>, KbError>;

    pub fn get_prompt(&self, name: &str, args: &HashMap<String, String>) -> Result<String, KbError>;

    pub fn create_entry(&mut self, req: &CreateEntryRequest, client_name: Option<&str>) -> Result<CreateEntryResponse, KbError>;

    pub fn update_entry(&mut self, req: &UpdateEntryRequest, client_name: Option<&str>) -> Result<UpdateEntryResponse, KbError>;
}
```

### 1.3 Audit Log
```rust
pub fn log_mcp_operation(
    kb_root: &Path,
    client: Option<&str>,
    tool: &str,
    entry_id: &str,
    outcome: &str,
    error_code: Option<&str>,
) -> std::io::Result<()>;
```
