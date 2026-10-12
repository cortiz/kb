# Quickstart & Validation Guide: MCP Server

**Feature**: MCP Server (`012-mcp-server`)  
**Status**: Completed  
**Date**: 2026-10-11  

---

## 1. Prerequisites

- Built `kb` binary: `cargo build --workspace`
- A sample knowledge base in `~/kb` (or temporary test directory initialized with `kb init`).

---

## 2. Interactive Validation Scenarios

### Scenario 1: Register Server in Claude Code
1. Run setup helper:
   ```bash
   kb mcp setup
   ```
2. Verify output includes the registration command:
   ```bash
   claude mcp add kb -- kb mcp
   ```
3. Run `kb doctor`:
   - Verify `kb mcp` readiness check passes without errors.

---

### Scenario 2: Search and Read Entries
1. Start an MCP session (or test client script):
   ```bash
   echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search","arguments":{"query":"auth"}}}' | kb mcp
   ```
2. Verify ranked results are returned with ID, title, snippet, and `unreviewed: false`.
3. Call `get` tool with variable resolution:
   ```bash
   echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get","arguments":{"id":"cmd/restart-worker","resolve":true}}}' | kb mcp
   ```
4. Verify response includes the resolved command payload and links.

---

### Scenario 3: Secret Protection
1. Query an entry containing a secret with an `env` source:
   ```bash
   echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get","arguments":{"id":"cmd/secret-test","resolve":true}}}' | kb mcp
   ```
2. Verify resolved payload replaces secret with `$VAR_NAME` reference (never plaintext).
3. Attempt to supply a secret value in `vars`:
   - Verify tool call is rejected with an error without echoing the secret.

---

### Scenario 4: Agent Capture & Unreviewed Badge
1. Start server with write permission enabled:
   ```bash
   kb mcp --allow-write
   ```
2. Call `create` tool with a new snippet:
   ```bash
   echo '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"create","arguments":{"type":"cmd","title":"Agent Cleanup","payload":"docker system prune -f"}}}' | kb mcp --allow-write
   ```
3. Verify:
   - File is written and auto-committed with `Kb-Source: mcp` trailer.
   - Entry is flagged as `unreviewed: true`.
   - `kb show cmd/agent-cleanup` and TUI display the `agent` unreviewed badge.

---

### Scenario 5: Unreviewed Execution Protection
1. Attempt to run the newly created unreviewed command:
   ```bash
   kb run cmd/agent-cleanup
   ```
2. Verify:
   - kb displays a warning that the entry was modified by an agent and not yet reviewed.
   - kb demands typing `run` to execute (strong confirmation), even though the command is non-destructive.

---

### Scenario 6: Review Flow (`kb review`)
1. Run the interactive review command:
   ```bash
   kb review
   ```
2. Verify unified diff is displayed against last approved version.
3. Select `a` (approve):
   - Review commit is recorded (`kb: review <id>`).
   - Entry loses its unreviewed badge.
   - Subsequent `kb run` no longer demands elevated confirmation.
