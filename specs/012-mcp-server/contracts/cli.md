# CLI Contract: MCP Server

**Feature**: MCP Server (`012-mcp-server`)  
**Status**: Completed  
**Date**: 2026-10-11  

---

## 1. `kb mcp`

Runs the Model Context Protocol stdio server.

### Usage
```sh
kb mcp [OPTIONS]
```

### Options
- `--allow-write`: Enable write tools (`create`, `update`) for this session, overriding config.
- `--read-only`: Force write tools off for this session, overriding config.
- `--root <PATH>`: Explicit KB root directory.

### Stdio Rules
- **stdin**: Inbound JSON-RPC 2.0 messages (UTF-8, newline-delimited).
- **stdout**: Outbound JSON-RPC 2.0 responses and notifications ONLY. No log messages or banners.
- **stderr**: Diagnostic logging, error traces, startup info.

### Exit Codes
- `0`: Normal termination when the MCP client closes stdin/session.
- `2`: Startup error (e.g. invalid KB root, configuration error).
- All tool execution failures are returned as MCP JSON-RPC protocol error objects, never by process termination.

---

## 2. `kb mcp setup`

Prints configuration snippets to register `kb mcp` with MCP clients.

### Usage
```sh
kb mcp setup [--root <PATH>]
```

### Output
Prints registration instructions for Claude Code and generic MCP clients:
```text
To register kb with Claude Code:
  claude mcp add kb -- kb mcp

To allow writing (create and edit entries):
  claude mcp add kb -- kb mcp --allow-write

Generic MCP configuration (e.g. claude_desktop_config.json):
{
  "mcpServers": {
    "kb": {
      "command": "kb",
      "args": ["mcp"]
    }
  }
}
```

---

## 3. `kb review`

Interactive command to review, approve, or revert agent-created or agent-modified entries.

### Usage
```sh
kb review [ID...] [OPTIONS]
```

### Arguments
- `[ID...]`: Optional specific entry IDs to review. If omitted, reviews all unreviewed entries.

### Options
- `--approve-all`: Approve all pending unreviewed entries non-interactively.
- `--json`: Output unreviewed entries and diffs as a JSON document without prompting.

### Interactive Prompt Flow
For each unreviewed entry:
1. Shows entry ID, title, authoring agent client name, and timestamp.
2. Displays the colored unified diff against the last approved version (or full entry if newly created).
3. Prompts:
   ```text
   Review choice: [a]pprove, [r]evert, [e]dit, [s]kip (a/r/e/s)?
   ```
   - `a` (approve): Records approval commit `kb: review <id>` (or updates state.json) and removes unreviewed badge.
   - `r` (revert): Restores last approved version, commits `kb: revert agent change to <id>`, and marks reviewed.
   - `e` (edit): Opens file in `$EDITOR`; if saved, marks reviewed.
   - `s` (skip): Leaves unreviewed and moves to the next entry.
