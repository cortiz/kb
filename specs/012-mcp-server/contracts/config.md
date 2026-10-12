# Configuration Contract: MCP Server

**Feature**: MCP Server (`012-mcp-server`)  
**Status**: Completed  
**Date**: 2026-10-11  

---

## `[mcp]` Section in `config.toml`

The `[mcp]` table governs server behavior when `kb mcp` is executed.

```toml
[mcp]
# Enable agent write tools (create and update). Defaults to false for security.
write = false

# Expose prompt entries as MCP slash commands / prompts. Defaults to true.
prompts = true

# Record tool lookups as usage hits in frecency rankings. Defaults to false.
record_usage = false

# Use remembered variable values as pre-fills for agent queries. Defaults to false.
use_remembered = false

# Tags excluded from all agent read operations, searches, and prompts.
exclude_tags = ["private"]

# Entry types hidden from MCP. Empty by default (all types visible).
exclude_types = []

# Entry types the agent is permitted to create or update.
write_types = ["sql", "sumo", "cw", "cmd", "prompt", "link", "code"]

# Append read queries to .kb/mcp.log. Defaults to false (writes are always logged).
log_reads = false
```

### Precedence Rules
1. CLI flags (`--allow-write`, `--read-only`, `--root`) beat environment variables.
2. Environment variables (`KB_MCP_WRITE`, `KB_MCP_PROMPTS`, `KB_MCP_EXCLUDE_TAGS`, etc.) beat configuration values.
3. Configuration values beat built-in defaults.
