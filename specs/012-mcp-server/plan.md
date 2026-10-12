# Implementation Plan: MCP Server

**Branch**: `012-mcp-server` | **Date**: 2026-10-11 | **Spec**: [specs/012-mcp-server/spec.md](spec.md)

**Input**: Feature specification from `/specs/012-mcp-server/spec.md`

## Summary

Milestone MCP (`012-mcp-server`) exposes the knowledge base to AI coding agents (Claude Code and standard MCP clients) via `kb mcp`, a local stdio Model Context Protocol server. It provides:
1. Native stdio JSON-RPC 2.0 protocol engine adhering to MCP `2024-11-05` specification.
2. Read tools (`search`, `get`, `list_tags`) with M2 ranking, M5 variable resolution, M7 action link generation, and zero usage distortion by default.
3. Strict secret isolation replacing secrets with `$VAR` references and rejecting agent-supplied credentials.
4. Dynamic MCP prompts generated from `prompt` entries with variable arguments.
5. Opt-in write tools (`create`, `update`) gated by `--allow-write` and `mcp.write`, routed through the M6 save path with atomic writes, format-preserving edits, content hash concurrency checks, and auto-declaration of undeclared placeholders.
6. History attribution and review workflows: git trailers (`Kb-Source: mcp`, `Kb-Client: <name>`), unreviewed badges, execution safety elevation (typing `run` for unreviewed commands), and an interactive `kb review` command for approving or reverting changes.

---

## Technical Context

**Language/Version**: Rust 1.88 (2024 edition)  
**Primary Dependencies**: `serde` (1.0), `serde_json` (1.0), `rusqlite` (bundled), `clap` (4.x with derive), `ratatui` (0.30), `crossterm` (0.29), `syntect` (5.x).  
**Storage**: Plain UTF-8 Markdown entries; SQLite + FTS5 index in `.kb/index.db`; `.kb/state.json` for local review caching; `.kb/mcp.log` for append-only audit trail.  
**Testing**: `cargo test --workspace` with automated unit, integration, and MCP stdio session tests.  
**Target Platform**: macOS (arm64, x86_64) and Linux (x86_64, aarch64); Claude Code, Cursor, and standard MCP clients.  
**Project Type**: Terminal knowledge base CLI and TUI binary (`crates/kb`) over shared library core (`crates/kb-core`).  
**Performance Goals**:
- Server cold start and initial handshake response $\le 150$ ms on 10,000 entries.
- `search` tool latency $\le 50$ ms.
- Dynamic prompt list updates within time of next request.  
**Constraints**:
- Stdio only; zero listening sockets, no remote network transports (Constitution Principle III).
- Stdout reserved strictly for JSON-RPC 2.0 protocol messages; all diagnostics and logs to stderr.
- Zero plaintext secrets delivered to or stored by agents (Constitution Principle VI).
- Insert/suggest only; commands are returned as text, never executed by `kb mcp`.  
**Scale/Scope**: 10,000+ entries, 5 MCP tools (`search`, `get`, `list_tags`, `create`, `update`), prompts derived from `prompt` entries.

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evaluation |
|---|---|---|
| **I. Plain Files** | PASS | All entries remain plain UTF-8 Markdown files in the KB root. Agent writes use the M6 format-preserving save path. |
| **II. Disposable Index** | PASS | Searches query SQLite FTS5 index; agent writes update the index incrementally; full reindex reconstructs state. |
| **III. Local-Only** | PASS | Server runs over stdio as a child process of the local MCP client. Zero network listeners, sockets, or telemetry. |
| **IV. Terminal Only, One Core** | PASS | Formally amended in Constitution v1.1.0 to authorize stdio agent adapters (MCP) as thin front ends over `kb-core`. No GUI. Core domain logic lives in `kb-core`. |
| **V. Portable by Construction** | PASS | Supported on macOS and Linux over standard stdin/stdout pipes. |
| **VI. Secrets Never Stored** | PASS | Secrets resolved to environment variable references (`"$VAR"`), never plaintext; agent-passed secret values rejected; secret detection blocks agent writes. |
| **VII. Keyboard-First and Fast** | PASS | Latency budgets $\le 150$ ms startup, $\le 50$ ms search. Unreviewed changes require explicit typing `run` before execution. |
| **VIII. Test-First Core** | PASS | Strict red-green TDD. Comprehensive unit and integration test suite with scripted MCP client harness against fixture KB. |
| **IX. Simplicity First** | PASS | Implemented natively with `serde_json` without heavy external async runtimes or bloated MCP SDKs. 5 tools, prompts from entries, no rename or delete. |

---

## Project Structure

### Documentation (this feature)

```text
specs/012-mcp-server/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── cli.md
│   ├── config.md
│   ├── core-api.md
│   └── mcp-tools.md
├── checklists/
│   └── requirements.md
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code Layout

```text
crates/kb-core/src/
├── mcp/                 # Core MCP domain adapters and query engine
│   ├── mod.rs
│   ├── types.rs         # Protocol views (EntryView, PromptView, ChangeSet)
│   ├── engine.rs        # McpEngine: search, get, list_tags, prompts
│   ├── review.rs        # ReviewStateTracker: git trailers, state.json, diffing
│   ├── audit.rs         # Audit log append-only writer (.kb/mcp.log)
│   └── write.rs         # Agent create and update save path logic
├── config.rs            # Added [mcp] settings
└── lib.rs

crates/kb/src/
├── commands/
│   ├── mcp.rs           # kb mcp stdio JSON-RPC dispatch loop & setup
│   └── review.rs        # kb review interactive CLI diff prompt
├── commands.rs
├── doctor.rs            # Doctor check for kb mcp readiness
└── main.rs

crates/kb/tests/
├── cli_mcp.rs           # Automated stdio MCP protocol integration tests
└── cli_review.rs        # Tests for review, approval, reversion, and execution gates
```

---

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| None | All requirements met within core principles and Constitution v1.1.0 amendment | N/A |
