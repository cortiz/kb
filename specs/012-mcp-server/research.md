# Research & Design Decisions: MCP Server

**Feature**: MCP Server (`012-mcp-server`)  
**Status**: Completed  
**Date**: 2026-10-11  

---

## 1. MCP Protocol Implementation Strategy

### Decision
Implement a lightweight, native Model Context Protocol (MCP) transport engine directly using standard Rust standard library I/O (`std::io::BufReader`, `std::io::stdin`, `std::io::stdout`) and `serde_json` for JSON-RPC 2.0 message dispatch, rather than depending on an external heavy asynchronous MCP SDK.

### Rationale
1. **Constitution Principle IX (Simplicity First) & Dependency Footprint**: Heavy external MCP crates often require asynchronous runtimes (`tokio`), HTTP transports, SSE abstractions, and large dependency graphs. `kb mcp` is strictly stdio-only, synchronous, and local.
2. **Cold Start Performance ($\le 150$ ms)**: Stdio-based JSON-RPC line parsing (`BufRead::lines()`) starts instantly with near-zero overhead.
3. **Control over stdio separation (FR-003)**: Stdout must carry strictly framed JSON-RPC messages; all logging, tracing, and diagnostics must route to `stderr`. Managing stdio directly guarantees protocol channel hygiene.
4. **Maintenance & Stability**: The core MCP specification for stdio is standard JSON-RPC 2.0 with four standard lifecycle methods (`initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`, `prompts/list`, `prompts/get`).

### Alternatives Considered
- **Third-party Rust MCP SDK (e.g. `mcp-sdk-rust` or `rmcp`)**: Brings async runtime dependencies (`tokio`, `hyper`, `axum`) unneeded for a local stdio child process, increasing binary size and build time.
- **Python or Node bridge**: Violates Constitution Principle IV (single static binary).

---

## 2. Review State Tracking & Git Integration

### Decision
Track review status dual-layer:
1. **Git Repository Mode**:
   - Agent writes generate a commit using the M9 commit engine, appending trailers:
     ```text
     Kb-Source: mcp
     Kb-Client: claude-code
     ```
   - An entry is **unreviewed** if the most recent commit touching that entry's file contains the `Kb-Source: mcp` trailer and no subsequent review commit has approved it.
   - Approval generates an empty commit touching the entry with:
     ```text
     kb: review <id>

     Kb-Reviewed: <head-commit-sha>
     ```
   - Reverting restores the last approved commit's file contents via M6 save path, committed as:
     ```text
     kb: revert agent change to <id>
     ```
2. **Non-Git Fallback & Fast Cache**:
   - Store review state in `.kb/state.json` under an `unreviewed` set: `["cmd/tail-logs", "sumo/5xx-authorizer"]`.
   - When git is active, `.kb/state.json` acts as a fast cache updated on writes and review commands.

### Rationale
- Syncs across machines seamlessly via `git push`/`git pull` without requiring an external database or server (Constitution Principle I & III).
- Preserves full audit history and allows precise unified diff generation against the last human-approved commit.

### Alternatives Considered
- **Storing review state only in local `.kb/state.json`**: Fails FR-030 (review state would not sync between developer machines).
- **Frontmatter field `reviewed: true/false` in Markdown files**: Modifying frontmatter on approval pollutes file history and creates unnecessary file churn.

---

## 3. Secret Protection in Agent Communication

### Decision
Implement strict variable redaction and transformation during MCP tool execution:
1. **Variables with `env` attribute**: When resolving entries (`get` with `resolve: true`), substitute the current shell's environment variable reference (`"$SUMO_TOKEN"`) instead of reading or printing the secret value.
2. **Variables without `env` attribute**: Retain the unresolved template placeholder (`"{{token}}"`) and append an explanatory note in the tool response stating that the variable must be supplied outside the agent.
3. **Agent-supplied secret arguments**: If an agent attempts to pass a value for a `secret` kind variable in `vars: { token: "secret-val" }`, the tool rejects the call with an error naming the variable without echoing the passed value.
4. **Agent writes (`create`, `update`)**: All agent writes pass through `kb lint` and the M6 secret detector. If secret-like patterns are detected, the write is rejected with line numbers and rule names, never the secret text.

### Rationale
Satisfies Constitution Principle VI. Content delivered to or received from AI agents may be sent to third-party model providers, cached, or logged. Secret values must never traverse the MCP pipe.

---

## 4. Write Path & Content Versioning

### Decision
Enforce atomic format-preserving edits with content-hash version verification:
1. `version` returned in `get` and `create` is the hex-encoded SHA-256 hash of the entry's raw file content on disk.
2. `update` requires `version`. If the file on disk currently has a different SHA-256 hash, `update` is rejected with an error including the current version, preventing mid-air collisions with human edits or concurrent agent sessions.
3. Edits use `kb_core::authoring::update_entry` to preserve Markdown comments, line endings, and frontmatter formatting byte-for-byte outside changed fields.
4. Undeclared variable placeholders `{{placeholder}}` in agent payloads are automatically declared as `kind: text` in frontmatter before saving, preventing unexpected lint failures.

### Rationale
Guarantees integrity under concurrent editing and format-preservation required by Constitution Principle I and M6.

---

## 5. Audit Logging Architecture

### Decision
Log all MCP write requests to `.kb/mcp.log` in an append-only JSON Lines (JSONL) format:
```json
{"timestamp":"2026-10-11T14:30:00Z","client":"claude-code","tool":"create","entry_id":"cw/errors-api","outcome":"ok"}
```
Logging never includes entry body, payload, notes, or variable values. Read operations are logged only when `mcp.log_reads = true`.

### Rationale
Provides a reliable local audit trail for security without risk of credential leakage in logs.
