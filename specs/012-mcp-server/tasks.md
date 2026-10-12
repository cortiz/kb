# Tasks: MCP Server

**Input**: Design documents from `/specs/012-mcp-server/` (`spec.md`, `plan.md`, `data-model.md`, `contracts/`, `research.md`, `quickstart.md`)  
**Feature Branch**: `012-mcp-server`  
**Milestone**: MCP (Spec 012)  

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project configuration, stdio JSON-RPC protocol framing, and server CLI routing.

- [x] T001 Add `[mcp]` configuration settings (`write`, `prompts`, `record_usage`, `use_remembered`, `exclude_tags`, `exclude_types`, `write_types`, `log_reads`) to `crates/kb-core/src/config.rs`.
- [x] T002 Implement JSON-RPC 2.0 message parsing, serialization, and stdio framing in `crates/kb/src/commands/mcp.rs`.
- [x] T003 [P] Add `kb mcp` subcommand CLI arguments (`--allow-write`, `--read-only`, `--root`) and `kb mcp setup` in `crates/kb/src/commands/mcp.rs` and route in `crates/kb/src/main.rs`.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core MCP domain types, audit logging, review state tracking, and protocol dispatch.

**⚠️ CRITICAL**: Must be completed before any user story can be implemented.

- [x] T004 Create core MCP view and transfer types (`EntryView`, `VarDeclarationView`, `ChangeSet`, `ReviewState`, `AuditEntry`) in `crates/kb-core/src/mcp/types.rs`.
- [x] T005 [P] Implement append-only audit logger writing to `.kb/mcp.log` in `crates/kb-core/src/mcp/audit.rs`.
- [x] T006 Implement review state tracking (`ReviewStateTracker`: checking git trailers `Kb-Source: mcp` and `.kb/state.json`) in `crates/kb-core/src/mcp/review.rs`.
- [x] T007 Implement MCP protocol handshake (`initialize`, `notifications/initialized`, `ping`) responding within 150 ms in `crates/kb/src/commands/mcp.rs`.

**Checkpoint**: Foundation ready — JSON-RPC protocol lifecycle responds over stdio.

---

## Phase 3: User Story 1 — The agent finds and uses my queries (Priority: P1) 🎯 MVP

**Goal**: Expose read tools (`search`, `get`, `list_tags`) over MCP with M2 ranking, M5 resolution, M7 action links, and zero usage distortion.

**Independent Test**: Scripted MCP test client connects over stdio, calls `search` ("5xx auth") and `get` with variable resolution, verifying ranked results, resolved fields, console links, and exit cleanly without executing commands.

### Tests for User Story 1

- [x] T008 [P] [US1] Write unit tests for MCP read queries (`search`, `get`, `list_tags`) in `crates/kb-core/src/mcp/engine.rs`.
- [x] T009 [P] [US1] Write integration tests for `search`, `get`, and `list_tags` over stdio in `crates/kb/tests/cli_mcp.rs`.

### Implementation for User Story 1

- [x] T010 [US1] Implement `McpEngine::search` with M2 ranked syntax, type/tag filters, and exclusion checks in `crates/kb-core/src/mcp/engine.rs`.
- [x] T011 [US1] Implement `McpEngine::get` returning `EntryView` with declared variables, version hash, and M7 action links in `crates/kb-core/src/mcp/engine.rs`.
- [x] T012 [US1] Implement `McpEngine::list_tags` returning vocabulary with counts in `crates/kb-core/src/mcp/engine.rs`.
- [x] T013 [US1] Implement variable resolution with declared defaults and missing variable error reporting in `crates/kb-core/src/mcp/engine.rs`.
- [x] T014 [US1] Dispatch `tools/list` and `tools/call` for `search`, `get`, and `list_tags` in `crates/kb/src/commands/mcp.rs`.

**Checkpoint**: User Story 1 complete. Agents can discover, read, and resolve queries and commands safely.

---

## Phase 4: User Story 2 — Secrets never reach the agent (Priority: P1)

**Goal**: Enforce strict credential isolation: secrets with `env` source resolve to `$VAR` references, secrets without `env` retain placeholders, and agent-supplied secret arguments are rejected.

**Independent Test**: Query fixture entries containing secrets through `get` and verify responses contain `"$VAR"` or `"{{token}}"`, zero plaintext credentials, and rejection when passing secret values in `vars`.

### Tests for User Story 2

- [x] T015 [P] [US2] Write unit tests verifying secret transformation to `$VAR` references and rejection of agent-supplied secrets in `crates/kb-core/src/mcp/engine.rs`.
- [x] T016 [P] [US2] Write integration tests in `crates/kb/tests/cli_mcp.rs` verifying zero secret exposure across all tool outputs, errors, and audit logs.

### Implementation for User Story 2

- [x] T017 [US2] Enforce secret variable resolution to shell `$VAR_NAME` syntax (or placeholder retention) in `crates/kb-core/src/mcp/engine.rs`.
- [x] T018 [US2] Reject tool calls containing values for `kind: secret` variables in `vars` without echoing the value in `crates/kb-core/src/mcp/engine.rs`.

**Checkpoint**: User Story 2 complete. Plaintext secrets are strictly blocked from agent visibility.

---

## Phase 5: User Story 3 — The agent saves something useful it just built (Priority: P1)

**Goal**: Opt-in `create` tool allowing agents to save new entries through the M6 save path with secret validation, auto-commit with agent trailers, and unreviewed flagging.

**Independent Test**: Start `kb mcp --allow-write`, call `create` with title, payload, and notes; verify file creation, auto-commit with `Kb-Source: mcp` trailer, unreviewed state, and rejection when write mode is off or content contains secrets.

### Tests for User Story 3

- [x] T019 [P] [US3] Write unit tests for `McpEngine::create_entry` (type detection, path validation, secret rejection, auto-declaration of undeclared placeholders) in `crates/kb-core/src/mcp/write.rs`.
- [x] T020 [P] [US3] Write integration tests in `crates/kb/tests/cli_mcp.rs` verifying `create` execution, dry-run mode, and opt-in gating (`--allow-write` vs `--read-only`).

### Implementation for User Story 3

- [x] T021 [US3] Implement `McpEngine::create_entry` with M1 lint, secret checks, and auto-declaration of undeclared placeholders as `kind: text` in `crates/kb-core/src/mcp/write.rs`.
- [x] T022 [US3] Implement atomic file saving, index updating, and auto-commit with `Kb-Source: mcp` and `Kb-Client: <name>` trailers in `crates/kb-core/src/mcp/write.rs`.
- [x] T023 [US3] Wire `create` tool in `tools/list` and `tools/call` conditional on `--allow-write` or `mcp.write = true` in `crates/kb/src/commands/mcp.rs`.

**Checkpoint**: User Story 3 complete. Agents can safely capture new snippets with full attribution and audit logging.

---

## Phase 6: User Story 4 — The agent improves an existing entry (Priority: P2)

**Goal**: Opt-in `update` tool enabling incremental, format-preserving edits with content-hash concurrency validation.

**Independent Test**: Call `update` with entry ID, version hash, and `changes` (deltas and field updates); verify format-preserving modification, version mismatch rejection, dry-run unified diffs, and updated git commits.

### Tests for User Story 4

- [x] T024 [P] [US4] Write unit tests for `McpEngine::update_entry` (SHA-256 version concurrency check, tag/var delta mutation, format preservation) in `crates/kb-core/src/mcp/write.rs`.
- [x] T025 [P] [US4] Write integration tests for `update` tool in `crates/kb/tests/cli_mcp.rs` including conflict rejection and dry-run diff generation.

### Implementation for User Story 4

- [x] T026 [US4] Implement optimistic concurrency verification comparing SHA-256 version hashes in `crates/kb-core/src/mcp/write.rs`.
- [x] T027 [US4] Implement format-preserving `update_entry` supporting delta mutations (`add_tags`, `remove_tags`, `set_vars`, `remove_vars`) and full replacements (`tags`, `vars`) in `crates/kb-core/src/mcp/write.rs`.
- [x] T028 [US4] Wire `update` tool in `tools/list` and `tools/call` with dry-run unified diff generation in `crates/kb/src/commands/mcp.rs`.

**Checkpoint**: User Story 4 complete. Agents can safely modify existing entries without overwriting unread human edits.

---

## Phase 7: User Story 5 — I see and approve what the agent changed (Priority: P1)

**Goal**: Unreviewed entry safety protections across TUI/CLI/widget, and interactive `kb review` command to approve or revert changes.

**Independent Test**: Verify `agent` badge on unreviewed entries, elevated `run` confirmation requirement for unreviewed commands in `kb run` and widget, and interactive `kb review` approval and reversion flows.

### Tests for User Story 5

- [x] T029 [P] [US5] Write unit tests for `ReviewStateTracker` approval commits and reversion in `crates/kb-core/src/mcp/review.rs`.
- [x] T030 [P] [US5] Write integration tests in `crates/kb/tests/cli_review.rs` for `kb review` prompt choices (`[a]pprove`, `[r]evert`, `[e]dit`, `[s]kip`), `--approve-all`, and `--json`.
- [x] T031 [P] [US5] Write integration tests in `crates/kb/tests/cli_run.rs` and `crates/kb/tests/shell_integration_pty.rs` verifying elevated confirmation on unreviewed commands.

### Implementation for User Story 5

- [x] T032 [US5] Implement unreviewed badge in TUI list, preview, and `kb show`, plus header unreviewed count and `unreviewed:true` search filter in `crates/kb/src/tui/` and `crates/kb-core/src/search/`.
- [x] T033 [US5] Implement safety gate requiring typed `run` confirmation for unreviewed `cmd` entries in `crates/kb/src/commands/run.rs` and `y` confirmation in widget/open.
- [x] T034 [US5] Implement interactive `kb review` command presenting colored unified diffs with `[a]pprove`, `[r]evert`, `[e]dit`, and `[s]kip` actions in `crates/kb/src/commands/review.rs`.

**Checkpoint**: User Story 5 complete. Users have complete visibility and veto power over all agent modifications.

---

## Phase 8: User Story 6 — Prompts become slash commands (Priority: P2)

**Goal**: Offer `prompt` entries as MCP prompts with arguments derived from variable declarations.

**Independent Test**: Create prompt entries with variables, call `prompts/list` and `prompts/get`, verifying correct argument declarations, resolution with provided args, and exclusion of prompts with secrets or excluded tags.

### Tests for User Story 6

- [x] T035 [P] [US6] Write unit tests for MCP prompt discovery and argument mapping in `crates/kb-core/src/mcp/engine.rs`.
- [x] T036 [P] [US6] Write integration tests for `prompts/list` and `prompts/get` in `crates/kb/tests/cli_mcp.rs`.

### Implementation for User Story 6

- [x] T037 [US6] Implement prompt entry filtering (excluding secrets and `mcp.exclude_tags`) and argument derivation in `crates/kb-core/src/mcp/engine.rs`.
- [x] T038 [US6] Dispatch `prompts/list` and `prompts/get` returning resolved prompt message text in `crates/kb/src/commands/mcp.rs`.

**Checkpoint**: User Story 6 complete. Reusable prompts are directly accessible as agent prompts.

---

## Phase 9: User Story 7 — Easy setup in my agent (Priority: P2)

**Goal**: Seamless agent onboarding via `kb mcp setup` and doctor validation.

**Independent Test**: Run `kb mcp setup` and `kb doctor` to verify correct client registration instructions and green diagnostic checks.

### Tests for User Story 7

- [x] T039 [P] [US7] Write integration test for `kb mcp setup` command output and `kb doctor` MCP diagnostics in `crates/kb/tests/cli_mcp.rs`.

### Implementation for User Story 7

- [x] T040 [US7] Implement `kb mcp setup` printing Claude Code (`claude mcp add kb -- kb mcp`) and generic JSON configurations in `crates/kb/src/commands/mcp.rs`.
- [x] T041 [US7] Add `kb mcp` connectivity check to `kb doctor` in `crates/kb/src/doctor.rs`.

**Checkpoint**: User Story 7 complete. Developer onboarding is friction-free.

---

## Phase 10: Polish & Cross-Cutting Concerns

**Purpose**: Performance benchmarks, guardrails validation, and user documentation.

- [x] T042 [P] Add benchmark tests for MCP startup ($\le 150$ ms) and tool search latency ($\le 50$ ms) in `tests/perf.rs`.
- [x] T043 [P] Verify architectural boundaries pass `platform_boundary` and `varres_boundary` tests in `crates/kb/tests/`.
- [x] T044 Update `README.md` with MCP Server instructions, Claude Code setup, review flow, and configuration options.
- [x] T045 Execute end-to-end quickstart validation scenarios from `specs/012-mcp-server/quickstart.md`.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: Independent — can begin immediately.
- **Foundational (Phase 2)**: Depends on Phase 1 — BLOCKS all user stories.
- **User Story 1 (Phase 3)**: Depends on Phase 2 — Core MVP.
- **User Story 2 (Phase 4)**: Depends on Phase 3 (US1).
- **User Story 3 (Phase 5)**: Depends on Phase 2 & Phase 4 (US2 secret rules).
- **User Story 4 (Phase 6)**: Depends on Phase 5 (US3 write path).
- **User Story 5 (Phase 7)**: Depends on Phase 5 & Phase 6 (US3/US4 agent changes).
- **User Story 6 (Phase 8)**: Depends on Phase 3 (US1 engine).
- **User Story 7 (Phase 9)**: Depends on Phase 3 (US1 server).
- **Polish (Phase 10)**: Depends on completion of all user story phases.

### Parallel Opportunities

- Within Phase 1: T003 can run in parallel with T001/T002.
- Within Phase 2: T005, T006, and T007 can run concurrently once T004 is defined.
- Within User Story 1: T008 and T009 test writing can run in parallel.
- User Story 6 (Prompts) and User Story 7 (Setup) can run in parallel with User Story 3/4/5 once US1 is complete.
- Polish tasks T042, T043, and T044 can proceed in parallel.

---

## Implementation Strategy

### MVP First (Phases 1–3)
1. Complete Phase 1 (Setup) and Phase 2 (Foundational).
2. Complete Phase 3 (User Story 1: Read tools `search`, `get`, `list_tags`).
3. Validate MVP: Claude Code can search and inspect curated KB entries over stdio.

### Incremental Feature Delivery
1. Add Secret Protection (Phase 4, US2): Enforce credential shielding.
2. Add Agent Capture (Phase 5, US3): Enable safe `create`.
3. Add Incremental Editing (Phase 6, US4): Enable optimistic `update`.
4. Add Human Review & Safety Gates (Phase 7, US5): `kb review` and unreviewed badges.
5. Add Prompts & Setup (Phases 8–9, US6–US7): Polish agent experience.
6. Run benchmarks and finalize documentation (Phase 10).
