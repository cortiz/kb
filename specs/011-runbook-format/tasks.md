# Tasks: Runbook Format

<!--
  ============================================================================
  Tasks for 011-runbook-format
  Derived from:
  - specs/011-runbook-format/spec.md
  - specs/011-runbook-format/plan.md
  - specs/011-runbook-format/data-model.md
  - specs/011-runbook-format/contracts/
  - specs/011-runbook-format/research.md
  - .specify/memory/constitution.md
  ============================================================================
-->

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Test fixtures and infrastructure initialization for runbook entries

- [X] T001 [P] Create test fixture runbooks directory and baseline fixtures (valid runbook with mixed entry/do steps, missing entry, duplicate step id, type mismatch binding, >100 steps, unreviewed entry step) in `crates/kb/tests/fixtures/runbooks/`
- [X] T002 [P] Register runbook fixture loading and helper utilities in `crates/kb/tests/support/mod.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure and data types that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T003 [P] Add `EntryType::Runbook` enum variant and register `"runbook"` in `TYPE_LIST`, `ALL_KEYS`, `from_name`, `name()`, and payload requirements (notes body, no payload) in `crates/kb-core/src/entry.rs`
- [X] T004 [P] Implement `RunbookEntry`, `RunbookStep`, `StepTarget`, `StepVarBinding` data structures, frontmatter deserialization, and slug derivation for implicit step IDs in `crates/kb-core/src/runbook/mod.rs`
- [X] T005 [P] Implement variable binding table evaluator and kind compatibility checks (`enum` subset, `number` -> `text`, `secret` -> `secret`, literal validation) in `crates/kb-core/src/runbook/binding.rs`
- [X] T006 [P] Bump `INDEX_VERSION` from 3 to 4, add `runbook_steps` table and index schema in `crates/kb-core/src/index/schema.rs` and update drop/create statements
- [X] T007 Unit tests for runbook parsing, step id slugification, and variable binding evaluation in `crates/kb-core/src/runbook/mod.rs` and `crates/kb-core/src/runbook/binding.rs`

**Checkpoint**: Foundation ready - user story implementation can now begin in parallel

---

## Phase 3: User Story 1 - Write a runbook that reuses my entries (Priority: P1) 🎯 MVP

**Goal**: Carlos writes a runbook referencing existing entries or manual instructions. `kb lint` verifies all step references, binding compatibility, step limits, and suggests fixes for broken references.

**Independent Test**: Run `kb lint` and `kb lint --json` across fixture runbooks covering every rule in FR-012/FR-014; verify diagnostic codes, severities, line numbers, and exit codes.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T008 [P] [US1] Unit and integration tests for all lint rules KB070–KB084 in `crates/kb-core/src/lint/rules.rs` and `crates/kb/tests/cli_lint.rs`
- [X] T009 [P] [US1] Integration tests for `kb lint --json` output formatting, exit codes, and fix suggestions in `crates/kb/tests/cli_lint.rs`

### Implementation for User Story 1

- [X] T010 [US1] Implement lint rules KB070–KB075 (no steps, entry and do conflict, missing entry with rename detection / edit distance suggestion, step is runbook, duplicate step id, invalid step id) in `crates/kb-core/src/lint/rules.rs`
- [X] T011 [US1] Implement lint rules KB076–KB078 (invalid binding kind compatibility, binding unknown runbook var, binding var not in entry) in `crates/kb-core/src/lint/rules.rs`
- [X] T012 [US1] Implement lint rules KB079–KB084 (too many steps >100, long runbook >50, step entry has errors, unused runbook var, unbound step var info, implicit step id info) in `crates/kb-core/src/lint/rules.rs`
- [X] T013 [US1] Implement `kb lint --fix` for broken runbook references (FR-015) using format-preserving edits in `crates/kb/src/commands/lint.rs`

**Checkpoint**: User Story 1 functional and testable independently (MVP ready!)

---

## Phase 4: User Story 2 - Find and read runbooks (Priority: P1)

**Goal**: Carlos searches `deploy auth` and sees the runbook. The preview displays variables, numbered steps with entry badges/titles, confirm/optional flags, bindings, and error indicators. Ordinary entries show "Used in".

**Independent Test**: Search runbook terms, execute `runbook:<id>` and `used-by:runbook` filters, and verify preview output and "Used in" sections in TUI and CLI.

### Tests for User Story 2 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T014 [P] [US2] Integration tests for runbook search indexing, `runbook:<id>` filter, `used-by:runbook` filter, and type chip `type:runbook` in `crates/kb/tests/cli_search.rs`
- [X] T015 [P] [US2] TUI preview tests for runbook step rendering (numbered steps, badges, optional/confirm, bindings, missing/errors/agent indicators) and "Used in" section on regular entries in `crates/kb/tests/tui_preview.rs`

### Implementation for User Story 2

- [X] T016 [US2] Implement runbook document mapping in `crates/kb-core/src/index/doc.rs` (title, tags, id, meta=step ids/titles, payload=do text/step notes, notes=body) and index synchronization for `runbook_steps` in `crates/kb-core/src/index/sync.rs`
- [X] T017 [US2] Implement query filters `runbook:<id>` and `used-by:runbook` in `crates/kb-core/src/query.rs` and `crates/kb-core/src/search.rs`
- [X] T018 [US2] Register runbook type color, badge (`runbook`), and chip in `crates/kb-core/src/entry.rs` and `crates/kb/src/tui/theme.rs`
- [X] T019 [US2] Implement runbook preview rendering in `crates/kb/src/tui/preview.rs` (variables, numbered steps, referenced type badge and title, manual badges, bindings, indicators for missing/errors/agent, and sanitized text)
- [X] T020 [US2] Implement "Used in" runbooks section in entry previews for entries referenced by runbooks in `crates/kb/src/tui/preview.rs`
- [X] T021 [US2] Implement index query methods for fast "Used in" and runbook step lookups in `crates/kb-core/src/index/mod.rs`

**Checkpoint**: User Stories 1 AND 2 functional and testable independently

---

## Phase 5: User Story 3 - Renames, deletes and sync never break runbooks (Priority: P1)

**Goal**: Renaming an entry updates all referencing runbooks in the same commit. Deleting a referenced entry refuses unless `--force`. Sync across clones arrives consistently.

**Independent Test**: Rename and delete entries across multi-clone git fixtures with runbooks; assert atomic git commits, zero broken references, and refusal on unforced delete.

### Tests for User Story 3 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T022 [P] [US3] Integration tests for `kb mv` updating referencing runbooks in format-preserving fashion with single git commit in `crates/kb/tests/cli_mv.rs`
- [X] T023 [P] [US3] Integration tests for `kb rm` refusing deletion of referenced entries without `--force` in `crates/kb/tests/cli_rm.rs`
- [X] T024 [P] [US3] Multi-clone git sync test verifying runbook rename consistency across clones in `crates/kb/tests/sync_runbooks.rs`

### Implementation for User Story 3

- [X] T025 [US3] Implement format-preserving runbook reference replacement in `crates/kb-core/src/author/fsops.rs`
- [X] T026 [US3] Wire `kb mv` and TUI rename to find referencing runbooks via index, update `entry:` references, and stage/commit rename + runbook edits in one commit with runbook list in message body in `crates/kb/src/commands/mv.rs` and `crates/kb/src/tui/app.rs`
- [X] T027 [US3] Wire `kb rm` and TUI delete to check `runbook_steps` and refuse deletion unless `--force` (or confirmed in TUI modal listing runbooks) in `crates/kb/src/commands/rm.rs` and `crates/kb/src/tui/app.rs`

**Checkpoint**: User Stories 1, 2, and 3 functional and testable independently

---

## Phase 6: User Story 4 - Create a runbook quickly (Priority: P2)

**Goal**: Carlos creates a runbook via `kb runbook new` with automatic variable lifting, editor template, shell dynamic completion, and TUI `mark-entry` / `new-runbook-from-marks`.

**Independent Test**: Run `kb runbook new` with overlapping variables; assert lifted variable kinds and intersection of options; test mark toggling and runbook creation in TUI test runner.

### Tests for User Story 4 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T028 [P] [US4] Integration tests for `kb runbook new` variable lifting (compatible kinds, enum intersection, shared default) and step generation in `crates/kb/tests/cli_runbook_new.rs`
- [X] T029 [P] [US4] TUI tests for `mark-entry` and `new-runbook-from-marks` keybindings in `crates/kb/tests/tui_runbook_marks.rs`

### Implementation for User Story 4

- [X] T030 [US4] Implement variable lifting algorithm and runbook template generation in `crates/kb-core/src/author/runbook_new.rs`
- [X] T031 [US4] Implement `kb runbook new <title> <entry-ids...>` CLI command with `--path`, `--tag`, `--yes`, `--json` in `crates/kb/src/commands/runbook.rs`
- [X] T032 [US4] Implement `kb add --type runbook --editor` template with commented step fields in `crates/kb-core/src/author/draft.rs`
- [X] T033 [US4] Implement TUI `mark-entry` and `new-runbook-from-marks` keymap commands and state in `crates/kb/src/tui/mark.rs`, `crates/kb/src/tui/keymap.rs`, and default bindings (`Alt-M`/`v`, `Alt-B`/`B`) in `crates/kb/src/tui/config.rs`
- [X] T034 [US4] Add shell dynamic completion for `kb runbook new` entry IDs and `runbook:` filter in `crates/kb/src/commands/complete.rs`

**Checkpoint**: User Stories 1 through 4 functional and testable independently

---

## Phase 7: User Story 5 - Agents read and write runbooks (Priority: P2)

**Goal**: AI agents interact with runbooks over MCP (`get` with binding table and resolved steps, `create`, `update` with `changes.steps`, `hidden step` masking, and "used in" lists).

**Independent Test**: Execute MCP protocol tests against runbooks; assert step resolution, private entry masking, git commit trailers (`Kb-Source: mcp`), and `unreviewed` flags.

### Tests for User Story 5 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T035 [P] [US5] MCP integration tests for runbook `get` (steps + binding table), `resolve: true` payload resolution, and hidden step masking in `crates/kb/tests/mcp_runbooks.rs`
- [X] T036 [P] [US5] MCP integration tests for runbook `create` and `update`, unreviewed badge, `Kb-Source: mcp` trailer, and "used in" lists on entries in `crates/kb/tests/mcp_runbooks.rs`

### Implementation for User Story 5

- [X] T037 [US5] Update MCP `get` handler to return runbook steps, binding table, and resolve entry payloads with runbook vars/step bindings in `crates/kb/src/mcp/server.rs`
- [X] T038 [US5] Implement hidden step masking in MCP `get` for entries matching `mcp.exclude_tags` or `mcp.exclude_types` returning `{"hidden": true, "position": N}` in `crates/kb/src/mcp/server.rs`
- [X] T039 [US5] Update MCP `create` and `update` handlers for runbooks with strict lint validation, `Kb-Source: mcp` trailer, and unreviewed flagging in `crates/kb/src/mcp/server.rs`
- [X] T040 [US5] Add `used_in` runbook lists to MCP `get` and `update` responses for referenced entries in `crates/kb/src/mcp/server.rs`

**Checkpoint**: User Stories 1 through 5 functional and testable independently

---

## Phase 8: User Story 6 - Share a runbook as a checklist (Priority: P2)

**Goal**: Carlos shares a runbook as a Markdown checklist via `kb show --checklist [--resolve]` or copies it with `kb use <runbook>`. Secrets are never exposed.

**Independent Test**: Verify checklist output formatting, prompt-once variable interpolation, and environment variable / placeholder secret substitution.

### Tests for User Story 6 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T041 [P] [US6] Integration tests for `kb show <runbook> --checklist` and `--resolve` with secret masking in `crates/kb/tests/cli_checklist.rs`
- [X] T042 [P] [US6] Integration tests for `kb use <runbook>` clipboard copy and usage recording in `crates/kb/tests/cli_use.rs`

### Implementation for User Story 6

- [X] T043 [US6] Implement Markdown checklist generator in `crates/kb-core/src/runbook/checklist.rs` (headings, `- [ ]`, optional markers, manual instructions)
- [X] T044 [US6] Implement resolved checklist generation with single variable prompt, inlined fenced blocks, and secret masking (`$ENV_VAR` or `{{name}}`) in `crates/kb-core/src/runbook/checklist.rs`
- [X] T045 [US6] Wire `kb show <runbook> --checklist` and `--resolve` in `crates/kb/src/commands/show.rs`
- [X] T046 [US6] Wire `kb use <runbook>` to generate resolved checklist, copy to clipboard, and record usage in `crates/kb/src/commands/use_cmd.rs`
- [X] T047 [US6] Update pick output for runbooks in `crates/kb/src/commands/pick.rs` to output Markdown checklist in payload mode and structured JSON in `--print json`

**Checkpoint**: All user stories functional and testable independently

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: Terminal safety, performance validation, and documentation

- [X] T048 [P] Terminal safety: ensure all runbook text (title, step titles, do text, notes) passes through `sanitize_terminal` before terminal rendering in `crates/kb/src/tui/preview.rs` and `crates/kb/src/commands/show.rs`
- [X] T049 [P] Performance benchmark: verify "Used in" and `runbook:`/`used-by:` filters $\le 50$ ms on 10,000 entries in `crates/kb/tests/perf.rs`
- [X] T050 Run end-to-end quickstart validation scenarios from `specs/011-runbook-format/quickstart.md`
- [X] T051 Documentation updates in README.md and man pages
