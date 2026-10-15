# Tasks: Markdown Notes

<!--
  ============================================================================
  Tasks for 014-markdown-notes
  Derived from:
  - specs/014-markdown-notes/spec.md
  - specs/014-markdown-notes/plan.md
  - specs/014-markdown-notes/data-model.md
  - specs/014-markdown-notes/contracts/
  - specs/014-markdown-notes/research.md
  - .specify/memory/constitution.md
  ============================================================================
-->

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Test fixtures and infrastructure initialization for note entries

- [X] T001 [P] Create test fixture notes directory and baseline fixture files (standard note with title, note with `# Title` body fallback, oversized note >64 KB, malicious escape sequences note, and note with entry links) in `crates/kb/tests/fixtures/notes/`
- [X] T002 [P] Register note fixture loading and helper utilities in `crates/kb/tests/support/mod.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure and data types that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T003 [P] Add `EntryType::Note` enum variant and register `"note"` in `TYPE_LIST`, `ALL_KEYS`, `from_name`, and `name()` in `crates/kb-core/src/entry.rs`
- [X] T004 [P] Implement Note validation rules and lint diagnostics in `crates/kb-core/src/lint/rules.rs` for `E-NOTE-TITLE`: "Note lacks frontmatter `title` AND body has no level-1 heading (`# ...`)" ("Add `title: \"...\"` in frontmatter or `# Title` at start of body"), `E-NOTE-VARS`: "Note frontmatter declares a `vars:` key" ("Remove `vars:` (notes do not support variable interpolation)"), `W-NOTE-OVERSIZED`: "Note body byte length exceeds 64 KB ($65,536$ bytes)" ("Split large note into separate linked entries"), `I-NOTE-EMPTY`: "Note has valid frontmatter but completely empty body" ("Add content or documentation to note body"), and evaluate `E-SECRET-DETECTED` across the entire body: "Note body matches known secret pattern (AWS key, private key, token assignment)" ("Remove secret credentials from note body")
- [X] T005 Extract canonical plain-text title fallback from first non-empty line level-1 heading (`# Title`) when frontmatter `title` is omitted, stripping inline Markdown syntax (code ticks, asterisks, strikethrough, links) and collapsing whitespace in `crates/kb-core/src/entry.rs`
- [X] T006 [P] Bump `INDEX_VERSION` from 2 to 3 and add `entry_links` table schema: "CREATE TABLE IF NOT EXISTS entry_links (source_id TEXT NOT NULL, target_id TEXT NOT NULL, PRIMARY KEY(source_id, target_id)) WITHOUT ROWID; CREATE INDEX IF NOT EXISTS entry_links_target ON entry_links(target_id);" in `crates/kb-core/src/index/schema.rs`
- [X] T007 Unit tests for `NoteEntry` parsing, level-1 heading title fallback, and lint error/warning invariants in `crates/kb-core/src/entry.rs`

**Checkpoint**: Foundation ready - user story implementation can now begin in parallel

---

## Phase 3: User Story 1 - Write a note and find it later (Priority: P1) 🎯 MVP

**Goal**: Carlos can author notes via `kb note` or `kb add --type note`, save them to `note/<slug>.md`, and search them with fast ranking, column filters (`type:note`, `in:body`), snippet generation, and typo vocabulary isolation.

**Independent Test**: Create fixture notes of varying lengths and heading structures in a test KB populated with queries and commands sharing keywords. Run automated search queries across the dataset and assert ranking order (headings > body), `in:body` filtering, typo tolerance isolation, and snippet extraction.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T008 [P] [US1] Unit test for typo vocabulary isolation: verify that for note entries ONLY `cols[0..=3]` (`title`, `tags`, `id`, `meta`) contribute words to `vocab` and `cols[4]` (`payload` = note body) is strictly excluded in `crates/kb-core/src/index/sync.rs`
- [X] T009 [P] [US1] Integration tests for note authoring via `kb note` and `kb add --type note` with slug generation (`note/<slug>.md`) and title fallback in `crates/kb/tests/cli_note.rs`
- [X] T010 [P] [US1] Integration tests for note search ranking (headings > body), `in:body` column filter alias, `type:note` filter, and syntax-stripped snippet generation in `crates/kb/tests/cli_search.rs`

### Implementation for User Story 1

- [X] T011 [P] [US1] Implement note draft creation, template generation (with optional `private` tag prompt), and default storage path `note/<slug-of-title>.md` in `crates/kb-core/src/author/draft.rs`
- [X] T012 [P] [US1] Implement entry type auto-detection distinguishing `note` (dominant Markdown headings, lists, tables) from `prompt` (conversational AI instructions) in `crates/kb-core/src/author/detect.rs`
- [X] T013 [US1] Implement note search column mapping in `crates/kb-core/src/index/doc.rs`: headings 1–3 and link URLs to `meta`, body text/tables/lists/code to `payload` capped at 256 KB ($262,144$ bytes), and `notes` column empty
- [X] T014 [US1] Implement typo vocabulary isolation in `crates/kb-core/src/index/sync.rs`: ensure `insert_doc` and `apply_reset` exclude note `payload` (`cols[4]`) from `vocab` insertion
- [X] T015 [US1] Add `in:body` search filter as a canonical alias for `payload` in `crates/kb-core/src/query.rs` and `crates/kb-core/src/search.rs`
- [X] T016 [US1] Implement search snippet extraction for note entries from highest-scoring matching paragraph with Markdown syntax markers stripped in `crates/kb-core/src/search.rs`
- [X] T017 [US1] Implement `kb note [TITLE]` CLI command and `kb add --type note` support in `crates/kb/src/commands/note.rs` and register in `crates/kb/src/main.rs`
- [X] T018 [US1] Implement TUI add form support for `type: note` with multi-line body input, `private` tag toggle, and `Ctrl-E` external editor launch in `crates/kb/src/tui/add.rs` and `crates/kb/src/tui/add_view.rs`
- [X] T019 [US1] Implement non-interactive `--pick` / payload output mode for note entries returning Markdown body without frontmatter in `crates/kb/src/commands/pick.rs`

**Checkpoint**: At this point, User Story 1 is fully functional and testable independently (MVP ready!)

---

## Phase 4: User Story 2 - Read a note comfortably in the terminal (Priority: P1)

**Goal**: Carlos can read rich Markdown notes in the TUI preview pane and `kb show` with styled headings, wrapped tables with Unicode box borders, task lists, callouts, fenced code frames with syntax highlighting, strikethrough, footnotes, smooth scrolling, and bounded memory caching.

**Independent Test**: Render test notes containing all supported CommonMark extensions in ratatui preview fixtures; verify correct character layout, box borders, syntax highlighting triggers, and smooth vertical/horizontal scrolling in full preview.

### Tests for User Story 2 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T020 [P] [US2] Integration tests for `kb show <id>` terminal rendering and `kb show --raw <id>` unmodified source output in `crates/kb/tests/cli_show.rs`
- [X] T021 [P] [US2] PTY snapshot and interactive scrolling tests for note preview rendering, table wrapping, and code block display in `crates/kb/tests/tui_markdown_notes.rs`

### Implementation for User Story 2

- [X] T022 [US2] Extend streaming Markdown parser in `crates/kb/src/tui/markdown.rs` with `pulldown-cmark` features for tables, task lists, strikethrough, footnotes, and heading underlines
- [X] T023 [US2] Implement table layout with Unicode box borders (`┌─┬┐`, `│`, `├─┼┤`, `└─┴┘`), cell wrapping via `unicode-width`, and column alignment (`:---`, `:---:`, `---:`) in `crates/kb/src/tui/markdown.rs`
- [X] T024 [US2] Implement GitHub-style callouts (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`) with theme-aware icons and border styling in `crates/kb/src/tui/markdown.rs`
- [X] T025 [US2] Implement box-drawn framed code blocks (`┌─ [lang] ─┐`) with `syntect` syntax highlighting, and unhighlighted fallback for unknown languages (e.g., `mermaid`) in `crates/kb/src/tui/markdown.rs`
- [X] T026 [US2] Implement footnotes rendering (inline bracketed reference `[1]` and document footer definitions) and image text fallback `[image: alt] (path)` in `crates/kb/src/tui/markdown.rs`
- [X] T027 [P] [US2] Implement `RenderedNote` data structure (caching `Vec<Block>` parsed AST tokens for zero-reparse width reflowing) and bounded LRU cache enforcing "maximum of 20 notes or 16 MB of styled lines (whichever threshold is reached first), evicting least-recently-viewed entries" in `crates/kb/src/tui/render_cache.rs`
- [X] T028 [US2] Implement viewport-clipped drawing and lazy `syntect` highlighting (computing code spans only when entering visible scroll viewport) in `crates/kb/src/tui/preview.rs`
- [X] T029 [US2] Implement asynchronous worker parsing for notes with body size exceeding 16 KB ($16,384$ bytes) with "rendering…" placeholder in `crates/kb/src/tui/async_render.rs` and `crates/kb/src/tui/worker.rs`
- [X] T030 [US2] Implement terminal preview rendering for `kb show <id>` and raw unformatted output for `kb show --raw <id>` in `crates/kb/src/show.rs`
- [X] T031 [US2] Implement note actions in action menu: Copy Markdown source (primary Enter/copy), Copy rendered plain text (formatting and ANSI stripped), and Edit in external editor in `crates/kb/src/tui/action_view.rs` and `crates/kb/src/commands.rs`

**Checkpoint**: At this point, User Stories 1 AND 2 are both functional and testable independently

---

## Phase 5: User Story 3 - Note content can never take over my terminal (Priority: P1)

**Goal**: An automated agent or untrusted git commit creating a note with embedded C0/C1 control characters, ANSI escape sequences (CSI, OSC, DCS, APC, PM), fake hyperlinks, or raw HTML cannot compromise the terminal emulator or spoof output.

**Independent Test**: Feed malicious fixture notes containing C0/C1 control characters, ANSI CSI/OSC/DCS/APC sequences, and raw HTML into preview rendering and `kb show`. Capture PTY output and assert zero unauthorized escape codes reach the terminal.

### Tests for User Story 3 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T032 [P] [US3] Unit tests for streaming FSM sanitizer covering C0/C1 control codes, ANSI CSI, OSC, DCS, APC, PM, and literal HTML pass-through in `crates/kb-core/src/terminal/sanitize.rs`
- [X] T033 [P] [US3] Security PTY capture tests verifying zero unauthorized escape codes or unverified links reach the terminal emulator across `kb show` and TUI preview in `crates/kb/tests/security_sanitization.rs`

### Implementation for User Story 3

- [X] T034 [P] [US3] Implement zero-allocation streaming FSM sanitizer in `crates/kb-core/src/terminal/sanitize.rs` stripping C0/C1 (preserving tab and newline) and ANSI sequences (replacing with `␛` for display, stripping for `--raw`)
- [X] T035 [US3] Apply escape sequence sanitization to note titles, tags, and all free-text fields across TUI lists, headers, CLI output, and MCP text in `crates/kb-core/src/terminal/mod.rs` and `crates/kb/src/tui/view.rs`
- [X] T036 [US3] Implement safe terminal hyperlink emitter (OSC 8) restricted to verified `http`, `https`, and `mailto` schemes and gated by terminal capability detection in `crates/kb/src/platform/mod.rs` and `crates/kb/src/tui/markdown.rs`
- [X] T037 [US3] Ensure raw HTML tags (e.g. `<script>`, `<div>`) are rendered as literal plain text without interpretation in `crates/kb/src/tui/markdown.rs`

**Checkpoint**: At this point, User Stories 1, 2, and 3 are all functional, secure, and independently testable

---

## Phase 6: User Story 4 - Notes work with agents (Priority: P2)

**Goal**: An AI agent interacting via MCP can search, get, create, and update notes, respecting the secret detection gate and automatic exclusion of notes tagged `private`.

**Independent Test**: Execute MCP protocol tests calling `search`, `get`, `create`, and `update` against note entries, validating Markdown payload delivery, secret gate validation, and private tag exclusion.

### Tests for User Story 4 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T038 [P] [US4] Integration tests for MCP server note operations (`search`, `get`, `create`, `update`, secret detection error, and `private` tag exclusion) in `crates/kb/tests/cli_mcp.rs`

### Implementation for User Story 4

- [X] T039 [P] [US4] Update MCP `search` tool in `crates/kb-core/src/mcp/engine.rs` to return note entries and automatically exclude notes tagged `"private"` by default
- [X] T040 [US4] Update MCP `get` tool in `crates/kb-core/src/mcp/engine.rs` to return complete note Markdown body as `content` and structured frontmatter fields, applying text sanitization
- [X] T041 [US4] Update MCP `create` tool in `crates/kb-core/src/mcp/write.rs` to validate `type: "note"`, enforce secret detection rules, and write note to `note/<slug>.md`
- [X] T042 [US4] Update MCP `update` tool in `crates/kb-core/src/mcp/write.rs` to validate updated note content, enforce secret scanning, refresh timestamps, and update search index
- [X] T043 [US4] Integrate agent review workflow and badge display for agent-created/modified notes in `crates/kb-core/src/mcp/review.rs` and `crates/kb/src/tui/preview.rs`

**Checkpoint**: At this point, User Stories 1 through 4 are fully functional and testable

---

## Phase 7: User Story 5 - Link notes to other entries (Priority: P3)

**Goal**: Support `[[entry-id]]` references in notes, rendering target entry badge and title (`[sumo] 5xx by authorizer`), rendering broken links as `[?] <entry-id>`, providing "Follow entry link" action menu navigation with a link picker modal for multiple links, recording relationships in `entry_links` table, refactoring atomically on `kb mv`, and warning on `kb rm`.

**Independent Test**: Construct notes referencing existing and non-existent entry IDs via `[[id]]` syntax. Assert badge rendering, action menu navigation, reference updates on `kb mv`, and lint diagnostics on broken links.

### Tests for User Story 5 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T044 [P] [US5] Unit tests for `[[entry-id]]` link parsing, escaped `\[[` syntax, and `EntryLink` extraction in `crates/kb-core/src/entry/link.rs`
- [X] T045 [P] [US5] Integration tests for atomic link refactoring on `kb mv` and reference warning prompt on `kb rm` in `crates/kb/tests/cli_mv_rm.rs`
- [X] T046 [P] [US5] PTY tests for "Follow entry link" direct jump and link picker modal navigation in `crates/kb/tests/tui_action_pty.rs`

### Implementation for User Story 5

- [X] T047 [P] [US5] Implement `EntryLink` extraction and `[[entry-id]]` syntax parsing (treating `\[[` as literal `[[`) in `crates/kb-core/src/entry/link.rs`
- [X] T048 [US5] Implement `entry_links` table synchronization and target entry title indexing into `meta` column in `crates/kb-core/src/index/sync.rs`
- [X] T049 [US5] Implement `W-ENTRY-LINK-BROKEN` lint rule: "Entry reference `[[<target-id>]]` does not exist in the knowledge base" ("Fix target ID or create missing entry (fuzzy match suggestion)") in `crates/kb-core/src/lint/rules.rs`
- [X] T050 [US5] Implement entry link rendering with type badge and title, and broken link rendering as `[?] <entry-id>` in warning/red text in `crates/kb/src/tui/markdown.rs`
- [X] T051 [US5] Implement "Follow entry link" action in action menu (direct jump for single link, link picker modal for multiple links) in `crates/kb/src/tui/action_view.rs`
- [X] T052 [US5] Implement atomic entry link refactoring on `kb mv` across referencing notes in a single git commit in `crates/kb/src/author.rs` and `crates/kb-core/src/git/commit.rs`
- [X] T053 [US5] Implement referencing notes detection and deletion confirmation prompt on `kb rm` in `crates/kb/src/author.rs`

**Checkpoint**: All user stories (1 through 5) are independently functional, integrated, and verified

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Performance benchmarks, binary size enforcement, documentation, and system quality assurance

- [X] T054 [P] Performance benchmarks validating search latency $\le 50$ ms on 10,000 entries (SC-002) and resident memory growth $\le 100$ MB browsing 200 notes (SC-003) in `crates/kb/tests/perf.rs`
- [X] T055 [P] Binary size verification script ensuring release build growth $<100$ KB over v0.2.0 (SC-005) in `scripts/check-binary-size.sh` (there is no `scripts/release.sh`). **SC-005 is not met**: the script measures +217,664 bytes over the binary built from `main` before this feature (+386,048 over the v0.2.0 tag, which also holds the milestones merged since). It is not wired into CI.
- [X] T056 End-to-end validation of all scenarios in `specs/014-markdown-notes/quickstart.md`
- [X] T057 [P] Workspace code formatting and clippy validation via `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - can start immediately.
- **Foundational (Phase 2)**: Depends on Phase 1 setup fixtures - BLOCKS all user stories.
- **User Stories (Phase 3+)**: All depend on Foundational phase completion.
  - **User Story 1 (P1, MVP)**: Can start immediately after Phase 2.
  - **User Story 2 (P1)**: Can start after Phase 2; uses Note entries created in US1 or fixtures.
  - **User Story 3 (P1)**: Can start after Phase 2; sanitization integrates with US2 preview and `kb show`.
  - **User Story 4 (P2)**: Can start after Phase 2 (and benefits from US1 authoring validation).
  - **User Story 5 (P3)**: Can start after Phase 2 (adds entry links to index and preview).
- **Polish (Phase 8)**: Depends on all desired user stories being complete.

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Phase 2 - Delivers complete standalone MVP (authoring & search).
- **User Story 2 (P1)**: Can start after Phase 2 - Delivers terminal reading & preview. Integrates with US1 note files.
- **User Story 3 (P1)**: Can start after Phase 2 - Provides terminal safety. Integrates with US2 rendering.
- **User Story 4 (P2)**: Can start after Phase 2 - Provides MCP agent endpoints. Independent from US2/US3.
- **User Story 5 (P3)**: Can start after Phase 2 - Provides inter-entry linking and `kb mv`/`kb rm` refactoring.

### Within Each User Story

- Tests MUST be written FIRST and fail before implementation code is committed (Red-Green-Refactor per Constitution Principle VIII).
- Core models and data structures before services and CLI/TUI interfaces.
- Core parsing and indexing before TUI presentation.
- Checkpoint validation before declaring story complete.

### Parallel Opportunities

- **Phase 1 (Setup)**: Tasks T001 and T002 can run in parallel.
- **Phase 2 (Foundational)**: Tasks T003, T004, and T006 can run in parallel.
- **User Stories**: Once Phase 2 completes, US1, US2, US3, US4, and US5 can be developed in parallel by separate developers or executed sequentially in priority order.
- **Within US1**: T008, T009, T010 (tests) and T011, T012 can run in parallel.
- **Within US2**: T020, T021 (tests) and T027 can run in parallel.
- **Within US3**: T032, T033 (tests) and T034 can run in parallel.
- **Within US4**: T038 (test) and T039 can run in parallel.
- **Within US5**: T044, T045, T046 (tests) and T047 can run in parallel.
- **Phase 8 (Polish)**: T054, T055, and T057 can run in parallel.

---

## Parallel Example: User Story 1

```bash
# Launch test tasks for User Story 1 in parallel:
Task: T008 "Unit test for typo vocabulary isolation in crates/kb-core/src/index/sync.rs"
Task: T009 "Integration tests for note authoring in crates/kb/tests/cli_note.rs"
Task: T010 "Integration tests for note search ranking in crates/kb/tests/cli_search.rs"

# Launch independent implementation components in parallel:
Task: T011 "Implement note draft creation in crates/kb-core/src/author/draft.rs"
Task: T012 "Implement entry type auto-detection in crates/kb-core/src/author/detect.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete **Phase 1: Setup** (T001–T002).
2. Complete **Phase 2: Foundational** (T003–T007) — CRITICAL: Unblocks user stories.
3. Complete **Phase 3: User Story 1** (T008–T019).
4. **STOP and VALIDATE**: Verify Carlos can run `kb note "Role cache TTL"`, write content, save, and retrieve it via `kb search "ttl redis"`.
5. Release / demo MVP!

### Incremental Delivery

1. **Increment 1 (MVP)**: Setup + Foundational + User Story 1 → Note capture and search retrieval works end-to-end.
2. **Increment 2 (Terminal Experience)**: User Story 2 + User Story 3 → Beautiful, comfortable terminal rendering and strict terminal safety protection against malicious escape codes.
3. **Increment 3 (Agent Integration)**: User Story 4 → AI agents can read and write notes via MCP, with `private` tag shielding.
4. **Increment 4 (Knowledge Graph)**: User Story 5 → Notes link to commands/queries with `[[id]]` syntax, refactored automatically with `kb mv`.
5. **Increment 5 (Release Hardening)**: Phase 8 Polish → Verify $\le 50$ ms search, $<100$ KB binary growth, and 100% test coverage.

### Parallel Team Strategy

With multiple engineers:
1. Team completes Setup + Foundational together (T001–T007).
2. Once Foundational is complete:
   - **Engineer A**: User Story 1 (Authoring & Search)
   - **Engineer B**: User Story 2 (Terminal Markdown Rendering)
   - **Engineer C**: User Story 3 (Terminal Safety & Sanitization)
3. Once US1–US3 land:
   - **Engineer A**: User Story 4 (MCP Server Integration)
   - **Engineer B**: User Story 5 (Entry Links & Refactoring)
4. Team runs Polish & Performance validation together (Phase 8).

---

## Notes

- `[P]` tasks = different files, no blocking dependencies on incomplete tasks.
- `[Story]` label maps task directly to user stories from `spec.md` for traceability.
- Red-Green-Refactor: Verify tests fail before implementing features.
- All tasks specify concrete file paths.
- Stop at any checkpoint to validate individual stories independently.

---

## Implementation notes

Where the work differs from the task text:

- **Rule codes**: the lint rules are `KB070-note-title`, `KB071-note-vars`, `KB072-note-oversized`, `KB073-note-empty` and `KB074-entry-link-broken`, in the existing `KBnnn-slug` scheme. `E-SECRET-DETECTED` is the existing `KB060`–`KB065` scan, which already covers a note's whole body.
- **T029**: previews are already built on the UI's worker thread, so there is no `async_render.rs`. The worker now also lays a note out for the pane's width, loads the grammars of its code blocks and highlights the first screenful, so the first frame costs under a millisecond; the preview pane says "rendering…" until it arrives.
- **T036**: OSC 8 hyperlinks are emitted by `kb show` only (the TUI's cell buffer cannot carry them), for `http`, `https` and `mailto`, when stdout is a terminal that advertises support (`FORCE_HYPERLINK` overrides).
- **T035**: titles, tags, variable descriptions, lint messages and search snippets are defused where they are read; a file whose name has control characters is not an entry.
- **T054**: the budgets are `#[ignore]` tests in `crates/kb/tests/perf.rs` (run in release mode): search at 10,000 entries with 1,000 notes, a 240 KB note's first frame and scrolling, and resident memory while browsing 200 notes (+3 MB).
- **MCP**: see "Implementation notes" in `contracts/mcp.md`.

