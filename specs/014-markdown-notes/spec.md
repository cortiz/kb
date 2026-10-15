# Feature Specification: Markdown Notes

**Feature Branch**: `014-markdown-notes`
**Created**: 2026-10-14
**Status**: Draft
**Milestone**: Notes (spec 014; runbook run mode and run records become 015 and 016; terminal images and the type registry come later)
**Depends on**: M1 (`001-entry-format`), M2 (`002-index-search`), M3 (`003-tui-core`), M6 (`007-authoring`), M7 (`008-actions-code-links`), MCP (`012-mcp-server`). Works with R1 (`011-runbook-format`) and the vim keymap (`013-vim-keymap`) if they land first.
**Input**: Add a `note` entry type for free-form Markdown: random notes, explanations, how-tos. Render Markdown well in the terminal only by extending kb's existing renderer, make notes properly searchable without hurting search speed, keep memory use low for long notes, and never let note content control the terminal. No diagram rendering.

## Overview

Not everything worth keeping is a query or a command. "Why the role cache TTL is 300 s", how the authorizer talks to Redis, steps someone explained in a meeting: today these end up in a separate notes app, disconnected from the queries and commands they explain.

A `note` is an entry whose whole body is Markdown. It lives in the KB like any other entry: one file, searchable, synced with git, readable by agents through MCP.

Five decisions shape this spec:

1. **Terminal only.** Notes are rendered in the TUI and by `kb show`. There is no HTML output and no browser view (constitution IV).
2. **kb's own renderer.** kb already renders Markdown in `crates/kb/src/tui/markdown.rs` on top of `pulldown-cmark`, which streams events without building a document tree. This spec extends it with the missing elements instead of adding a second Markdown library. No new dependency is needed for Markdown.
3. **Notes are first-class in search.** Their body is indexed as main content, with headings above body text; note bodies stay out of the typo vocabulary so search keeps its 50 ms budget.
4. **Low memory for long notes.** Render once per file version, cache the result with a size cap, draw only the visible lines, and highlight code blocks only when they come into view.
5. **Note text can never control the terminal.** Notes can be written by agents (MCP) or synced from other machines, so control characters and escape sequences are removed before anything is drawn.

Diagram languages such as Mermaid get no special treatment: a ```` ```mermaid ```` block is an ordinary code block, shown as text. Drawing diagrams was considered and dropped to keep kb small.

## Clarifications

### Session 2026-10-14

- Q: How should the action menu present entry links when a note contains multiple `[[entry-id]]` references? (FR-026) → A: Single "Follow entry link" action that jumps directly if exactly one link exists, or opens a filtered link picker modal if multiple links exist.
- Q: How should the canonical title be extracted when a note omits frontmatter title and uses a level-1 heading containing inline Markdown styling? (FR-003) → A: Strip all inline Markdown syntax (code ticks, asterisks, strikethrough, links) and collapse whitespace to produce a normalized plain-text title.
- Q: How should the TUI preview render a broken entry link when the referenced entry ID does not exist in the knowledge base? (FR-029) → A: Render a `[?]` placeholder badge followed by the target entry ID in warning/red text (e.g., `[?] missing/entry-id`).

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Write a note and find it later (Priority: P1)

Carlos runs `kb note "Role cache TTL"`, writes a few paragraphs with a heading and a code block in his editor, saves and quits. A week later he searches `ttl redis` and the note is the first result.

**Why this priority**: Capture and retrieval are the primary purpose of notes in the knowledge base.

**Independent Test**: Create fixture notes of varying lengths and heading structures in a test KB populated with queries and commands sharing keywords. Run automated search queries across the dataset and assert ranking order and highlighted snippet output.

**Acceptance Scenarios**:

1. **Given** `kb note "<title>"`, **Then** kb opens the user's editor on a note template (frontmatter with title, followed by an empty body), validates on save, and writes it at `note/<slug>` by default as defined in M6.
2. **Given** `kb note` invoked with no title, **When** the body begins with a level-1 heading, **Then** that heading becomes the title across list views, search results, and MCP responses, with any inline Markdown styling stripped to plain text.
3. **Given** a search query whose terms appear in a note's headings or body, **Then** the note is returned, ranked per FR-010 (headings higher than body), with a snippet extracted from the best-matching paragraph.
4. **Given** a search query matching a note solely via a misspelling of a body term, **Then** the note is not guaranteed to match (typo tolerance applies strictly to titles, tags, and headings per FR-012); exact or prefix searches on body terms match reliably.
5. **Given** filter `type:note`, **Then** only note entries are listed. **Given** filter `in:body`, **Then** only note bodies and other entries' payloads are searched.

---

### User Story 2 — Read a note comfortably in the terminal (Priority: P1)

Carlos selects the note in the TUI list. The preview pane renders rich Markdown: styled headings, nested lists, a table drawn with box characters, a highlighted code block inside a framed border, and a "Note" callout box.

**Why this priority**: Notes are read far more frequently than they are authored; readability in the terminal determines daily utility.

**Independent Test**: Render test notes containing all supported CommonMark extensions in ratatui preview fixtures; verify correct character layout, box borders, syntax highlighting triggers, and smooth vertical/horizontal scrolling in full preview.

**Acceptance Scenarios**:

1. **Given** a selected note in the TUI, **Then** the preview renders all elements specified in FR-014.
2. **Given** a multi-page note, **Then** it scrolls smoothly in the preview pane via existing preview navigation keys (M3, and spec 013 when active), and full-preview toggle displays it full screen.
3. **Given** a link in note text, **Then** it renders as clickable hyperlink text (OSC 8, FR-019) where supported by the terminal emulator; otherwise the target URL is appended after the text.
4. **Given** `kb show <note>`, **Then** identical terminal rendering is emitted to standard output (honoring ANSI terminal capabilities and `NO_COLOR`); `kb show --raw <note>` outputs unmodified Markdown source.
5. **Given** a note of several hundred kilobytes, **Then** the TUI remains responsive during rendering, and scrolling maintains the 60 FPS frame budget (FR-016 through FR-018).

---

### User Story 3 — Note content can never take over my terminal (Priority: P1)

An automated agent creates a note via MCP that embeds raw escape sequences designed to clear the screen, rename the terminal window, fake OSC 8 hyperlinks, and inject `<script>` HTML tags. Viewing or displaying the note renders harmless sanitized text.

**Why this priority**: Notes accept arbitrary free-form input from external sources (MCP agents, remote git repositories). Malicious or accidental escape sequences can compromise terminal state, misrepresent commands, or spoof links.

**Independent Test**: Feed malicious fixture notes containing C0/C1 control characters, ANSI CSI/OSC/DCS/APC sequences, and raw HTML into preview rendering and `kb show`. Capture PTY output and assert zero unauthorized escape codes reach the terminal.

**Acceptance Scenarios**:

1. **Given** note content containing C0/C1 control characters, ANSI CSI/OSC/DCS/APC/PM escape sequences, **Then** they are stripped or replaced with visible `␛` placeholder characters in the TUI and `kb show`; no escape sequence reaches the terminal device.
2. **Given** raw HTML tags in a note, **Then** the HTML is rendered as literal plain text without interpretation.
3. **Given** a link with a scheme other than `http`, `https`, or `mailto`, **Then** the URL is rendered as plain text and is never emitted as an OSC 8 terminal hyperlink.
4. **Given** a note created or modified by an agent via MCP, **Then** it displays the "agent" badge and follows the review workflow established in spec 012.

---

### User Story 4 — Notes work with agents (Priority: P2)

An AI agent interacting via MCP searches the knowledge base for incident instructions, retrieves a detailed troubleshooting note, and updates it with newly verified steps.

**Why this priority**: Agents need access to unstructured operational documentation alongside structured queries and commands to assist effectively.

**Independent Test**: Execute MCP protocol tests calling `search`, `get`, `create`, and `update` against note entries, validating Markdown payload delivery, secret gate validation, and private tag exclusion.

**Acceptance Scenarios**:

1. **Given** an MCP `search` request, **Then** note entries are returned alongside other entries, unless filtered out by tag or entry type (spec 012 FR-016, FR-017).
2. **Given** an MCP `get` request targeting a note, **Then** its complete body is returned as standard Markdown text.
3. **Given** an MCP `create` or `update` request specifying `type: note`, **Then** the note is validated and saved through the shared validation pipeline, including the secret detection gate.
4. **Given** the TUI add form or `kb note` command, **Then** the user is prompted with an optional `private` tag accompanied by a concise explanation that private entries are excluded from agent access by default.

---

### User Story 5 — Link notes to other entries (Priority: P3)

Carlos writes "check [[sumo/5xx-by-authorizer]] first" inside a troubleshooting note. Viewing the note shows the Sumo query badge and title. Choosing the link in the action menu jumps directly to that entry.

**Why this priority**: Interlinking notes, queries, and commands creates a cohesive, navigable knowledge graph directly inside the terminal.

**Independent Test**: Construct notes referencing existing and non-existent entry IDs via `[[id]]` syntax. Assert badge rendering, action menu navigation, reference updates on `kb mv`, and lint diagnostics on broken links.

**Acceptance Scenarios**:

1. **Given** an entry reference `[[<entry-id>]]` in note text, **Then** the preview renders it with the referenced entry's type badge and title.
2. **Given** the action menu (M7) on a note with entry links, **When** "Follow entry link" is selected, **Then** if the note has exactly one entry link, it immediately selects that entry in the list; if it has multiple entry links, it opens a link picker modal listing the links, and selecting one navigates to that entry.
3. **Given** `kb mv` performed on a referenced entry, **Then** references in notes are updated within the same atomic operation (matching R1 FR-016). Given `kb rm`, kb lists referencing notes and requests confirmation before deletion.
4. **Given** an entry reference targeting a non-existent entry ID, **Then** `kb lint` reports a warning with a suggested fix, and preview renders the reference as `[?] <entry-id>` in warning/red text.

---

### Edge Cases

- **Variable syntax `{{` in a note**: Notes are never evaluated for variable interpolation (M5 FR-002). Literal `{{text}}` is rendered and copied exactly as authored, and `kb lint` does not flag undeclared variables in notes.
- **Variables declared in frontmatter**: Frontmatter declaring `vars` on a note is rejected as a lint error (notes do not support variables).
- **Very long notes**: The first 256 KB of the body is indexed for search (FR-013); `kb lint` issues a warning for bodies exceeding 64 KB; full rendering is supported regardless of file size.
- **Wide tables or long code lines**: Table cells wrap text to fit preview boundaries; if a table cannot fit within preview width, columns are laid out left-to-right and trailing columns that exceed the preview margin are clipped with a `→` continuation indicator. Full-preview mode allows horizontal scrolling across all columns. Code blocks never wrap; they clip at the preview margin with a `→` indicator and scroll horizontally in full-preview mode.
- **Images (`![alt](path)`)**: Rendered as textual fallback `[image: alt]` followed by the path in this milestone (terminal image graphics deferred to a future spec).
- **Frontmatter only with empty body**: Valid note structure; `kb lint` reports an informational finding.
- **Ambiguous note vs. prompt detection**: Automatic type detection (M6) differentiates notes from prompts per FR-025; if classification confidence is low, the authoring flow prompts the user to select the type.
- **Secret detection in prose**: Secret scanning rules apply across the entire note body; high-entropy token detection is restricted to assignment patterns (M1 FR-013) to prevent false positives on natural language prose.
- **Wide characters, emoji, and combining marks**: Text width calculations use unicode display width (`unicode-width`) to preserve alignment across table borders and framed code blocks.
- **Terminal resize and theme switches**: Cached rendered representations are reflowed for the new display width or theme without re-running the Markdown parser.
- **Runbook integration (R1)**: Runbook steps may reference notes (`entry: note/<slug>`), displaying the note body as contextual documentation within the runbook step view.

## Requirements *(mandatory)*

### Functional Requirements

#### Format

- **FR-001**: System MUST add `note` to the supported entry types as a minor, backward-compatible extension to `kb_format: 1`. A note entry has no separate payload field; its entire body represents Markdown content.
- **FR-002**: Note frontmatter MUST support: `kb_format`, `type: note`, `title`, optional `tags`, `created`, and `updated`. Frontmatter declaring `vars` MUST NOT be permitted and MUST fail schema validation.
- **FR-003**: Note frontmatter MAY omit the `title` field when the first non-empty line of the body is a Markdown level-1 heading (`# Title`). That heading MUST serve as the canonical title across all interfaces (TUI list, search results, CLI, MCP), with any inline Markdown formatting syntax stripped and internal whitespace collapsed to form a normalized plain-text title.
- **FR-004**: Markdown content MUST be parsed as CommonMark plus tables, task lists, strikethrough, footnotes, fenced code blocks with language identifiers, and GitHub-style callouts (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`). Raw HTML MUST NOT be interpreted as markup.
- **FR-005**: Text matching `[[<entry-id>]]` MUST be parsed as an **entry link** (User Story 5). Escaped syntax `\[[` MUST be treated as a literal `[[`.

#### Lint

- **FR-006**: System MUST introduce stable diagnostic lint codes for notes:
  - Error: Note lacking both frontmatter title and body level-1 heading.
  - Error: Note frontmatter containing `vars`.
  - Warning: Note body exceeding 64 KB in size.
  - Warning: Entry link referencing a non-existent entry ID (with suggested fuzzy-match fix).
  - Info: Note containing frontmatter but an empty body.
- **FR-007**: Secret detection rules from M1 MUST be evaluated across the entire note body upon save and during `kb lint`.

#### Search

- **FR-008**: Search indexing for note entries MUST map document elements to existing database columns:
  - Level 1–3 headings and referenced entry titles map to `meta`.
  - Body paragraphs, list items, table cell text, and code blocks map to `payload`.
  - The `notes` column remains empty for note entries.
- **FR-009**: Markdown syntax punctuation MUST NOT be indexed into search terms. Link URLs MUST contribute their extracted domain hostname and path terms into the `meta` column.
- **FR-010**: Search ranking for notes MUST follow M2 FR-012 with priority ordering: title matches first, followed by tags, followed by headings, followed by body text.
- **FR-011**: System MUST increment `INDEX_VERSION` so existing SQLite FTS5 search indexes automatically rebuild on startup (Constitution Principle II).
- **FR-012**: Words appearing solely within note body text MUST NOT be added to the typo-expansion vocabulary. Typo tolerance (M2 FR-011) MUST apply to note titles, tags, and headings; exact term matching and prefix matching MUST apply to all note content.
- **FR-013**: Indexing MUST process only the first 256 KB of a note's body. Search filter `in:body` MUST be accepted as a canonical alias for the `payload` search column. Search snippets for note entries MUST be generated from the highest-scoring matching paragraph with Markdown syntax markers stripped.

#### Rendering (TUI and `kb show`)

- **FR-014**: System MUST extend kb's existing Markdown renderer in `crates/kb/src/tui/markdown.rs` on top of `pulldown-cmark` (without introducing additional Markdown parser crates) to render:
  - Headings: distinct visual styling for levels 1–3, bold styling for levels 4–6, horizontal rule underlines beneath level 1 and 2 headings.
  - Paragraphs with word wrapping at boundary margins.
  - Inline text styling: emphasis (italic), strong (bold), strikethrough, and inline code.
  - Block quotes and GitHub-style callouts with distinctive icons and colors per callout kind.
  - Ordered, unordered, and nested list structures.
  - Task lists with check indicators (`☐` / `☑`) and dimmed styling on completed items.
  - Horizontal separator rules.
  - Tables rendered with Unicode box-drawing borders, distinctive header row styling, text-wrapped cells, and column alignment derived from table syntax (`:---`, `:---:`, `---:`).
  - Fenced code blocks enclosed within a box-drawn border frame, titled with the code language identifier, and highlighted via kb's existing syntect configuration.
  - Code blocks specifying languages without available syntax definitions (e.g., `mermaid`) rendered inside box-drawn frames as plain unhighlighted text.
  - Standard Markdown links (FR-019) and entry links (FR-005, FR-029).
  - Footnotes rendered with numbered superscript/bracketed references inline and footnote definitions grouped at the end of the document.
  - Images rendered as textual fallback `[image: alt]` accompanied by the image path.
- **FR-015**: No new third-party crate MUST be introduced for Markdown rendering. Any algorithms or styling techniques adapted from MIT-licensed implementations MUST preserve their respective copyright and license notices.
- **FR-016**: Rendering MUST output a cached list of styled terminal lines (`RenderedNote`) per note, keyed by file content hash, available preview display width, and active color theme. Parsing MUST execute at most once per content hash; subsequent width or theme changes MUST reflow the cached parsed tokens without reparsing Markdown.
- **FR-017**: Memory management for rendered notes MUST enforce the following constraints:
  - Rendered note cache MUST be bounded to a maximum of 20 notes or 16 MB of styled lines (whichever threshold is reached first), evicting least-recently-viewed entries.
  - TUI draw calls MUST process and paint only the subset of lines visible within the preview viewport.
  - Fenced code blocks MUST be highlighted lazily upon first scrolling into view, with highlighted spans retained in the cached note representation.
- **FR-018**: Notes with body size exceeding 16 KB MUST be parsed and laid out asynchronously off the main thread, presenting a "rendering…" placeholder indicator while keeping TUI frame rates within M3 budget.

#### Terminal Safety

- **FR-019**: Before rendering any note content to a terminal device, kb MUST sanitize text by stripping C0 and C1 control characters (preserving tab with column expansion and newline for line layout), ESC characters, and all ANSI escape sequences (CSI, OSC, DCS, APC, PM). Stripped escape sequences MUST be replaced with a visible `␛` placeholder in TUI and `kb show`, and omitted entirely in `kb show --raw`. Terminal hyperlinks (OSC 8) MUST only be emitted directly by kb's renderer, restricted to `http`, `https`, and `mailto` protocols, and emitted only when the terminal emulator advertises OSC 8 capability.
- **FR-020**: The escape sequence sanitization pipeline defined in FR-019 MUST be applied to note titles, tags, and all free-text fields across all entry types wherever drawn or output (TUI lists, headers, CLI output, MCP text).
- **FR-021**: Raw HTML markup in note content MUST be rendered as literal plain text and MUST never be interpreted or passed through as raw terminal markup.

#### Authoring

- **FR-022**: The command `kb note [title]` MUST open the user's configured editor on a note template and save through the M6 authoring pipeline; `kb add --type note` MUST exhibit identical behavior. The TUI add form (M6) MUST support notes with a multi-line body input area and a `Ctrl-E` shortcut to open the full editor.
- **FR-023**: Default storage paths for note entries MUST follow `note/<slug-of-title>` (M6 FR-008; configurable via `path_template`).
- **FR-024**: Both the TUI add form and the `kb note` editor template MUST present the `private` tag option with an accompanying explanation that private entries are shielded from agent access by default (User Story 4).
- **FR-025**: Entry type auto-detection (M6 FR-005) MUST classify imported content as `note` when Markdown structural elements (at least 2 distinct features among `#` headings, `- ` / `* ` lists, `|` tables, `> ` blockquotes, or multiple text paragraphs) score higher than conversational prompt markers (`"You are"`, `"Act as"`, `"Summarize"`, `"Explain"`, or `{{vars}}`). When neither score dominates or scores are tied, the system MUST prompt the user for explicit type selection.

#### Actions and Output

- **FR-026**: Note entries MUST support the following actions in the M7 action menu:
  - Copy Markdown source (primary action, bound to Enter/copy).
  - Copy rendered plain text (strips ANSI escape styling and Markdown syntax delimiters like `**`, `` ` ``, and `>` while preserving document structure, table column alignment, and list indentation).
  - Edit note in external editor.
  - "Follow entry link" (available when note contains one or more `[[entry-id]]` links; immediately jumps if exactly one link exists, or opens a link picker modal listing all document links when multiple exist).
  - Note entries MUST NOT expose an "open in browser" action.
- **FR-027**: The non-interactive pick output (M3 FR-017, `--pick` / `payload` mode) for a note entry MUST output its Markdown body without YAML frontmatter.

#### Entry Links (P3)

- **FR-028**: The index database MUST record note-to-entry link relationships (`note` → target `entry-id`). System commands MUST maintain link integrity:
  - `kb mv` on a referenced entry MUST update referencing note files in an atomic, format-preserving operation with a single git commit (matching R1 FR-016).
  - `kb rm` on a referenced entry MUST detect referencing notes and require user confirmation before removal.
- **FR-029**: Entry link rendering MUST NOT recursively render referenced entry contents; a linked entry is shown as its badge and title only. When an entry link targets a non-existent entry ID, the preview MUST render a `[?]` placeholder badge followed by the unresolved entry ID in warning/red text.

#### MCP

- **FR-030**: Note entries MUST be accessible via the Model Context Protocol (MCP) server endpoints (`search`, `get`, `create`, `update`) in conformance with spec 012. The `get` endpoint MUST return the note's Markdown body. All MCP text outputs MUST sanitize titles and text per FR-020.

### Key Entities

- **Note**: An entry of type `note` consisting of frontmatter (`title`, `tags`, timestamps) and a Markdown body. Contains no variable declarations or command payload.
- **EntryLink**: An internal reference `[[<target-id>]]` within a note linking to another knowledge base entry, storing byte span position, target entry ID, and cached resolved title/type badge.
- **RenderedNote**: A cached in-memory representation of a note containing layout-wrapped styled terminal lines, keyed by `(content_hash, width, theme)`, with lazily computed syntax-highlighted code block spans.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In a test knowledge base containing 30 fixture notes mixed with 300 existing queries and commands, a benchmark evaluation of 30 representative search queries ranks the target note first in at least 90% of cases and within the top three results in 100% of cases.
- **SC-002**: On a 10,000-entry knowledge base augmented with 1,000 notes averaging 8 KB of body text, `kb search` executes and returns results within the 50 ms budget on a warm index (maintaining M2 budget).
- **SC-003**: Sequentially browsing 200 notes in the TUI (including at least five notes exceeding 200 KB in size) maintains total process resident memory under 100 MB above startup baseline, and no frame render exceeds the 16 ms budget (maintaining M3 interactive responsiveness).
- **SC-004**: A dedicated security test suite containing malicious escape codes (CSI sequences, OSC title changes, clear-screen codes, forged OSC 8 hyperlinks, raw HTML script tags, and `javascript:` URLs) results in zero raw terminal escape sequences emitted beyond kb's own ratatui UI styles and verified OSC 8 links, verified via PTY byte-capture analysis.
- **SC-005**: Addition of the extended Markdown renderer and note support increases the release binary size by less than 100 KB compared to the baseline v0.2.0 build (no new external crate dependencies in default configuration).
- **SC-006**: Authoring, editing, and saving a short note via `kb note "<title>"` completes in under 30 seconds end-to-end (satisfying M6 SC-001).

## Assumptions

- **A-001**: Extending the existing ~360-line renderer in `crates/kb/src/tui/markdown.rs` by an estimated 400–600 lines requires significantly less binary overhead and maintenance complexity than integrating an external Markdown terminal crate.
- **A-002**: Users desiring to restrict AI agent access to sensitive internal notes will tag them with `private`, which spec 012 explicitly filters from agent visibility by default.
- **A-003**: The first 256 KB of a note body contains sufficient operational signal for indexing; notes exceeding this threshold remain fully readable and scrollable even though search indexing caps at 256 KB.
- **A-004**: Code syntax highlighting can be deferred until code block lines enter the visible scroll viewport, eliminating syntax highlighting overhead for large off-screen code blocks.

## Out of Scope

- HTML generation, web export, or browser-based rendering views (Constitution Principle IV).
- Visual diagram rendering (Mermaid, D2, PlantUML, Graphviz); diagram blocks are displayed as formatted plaintext code blocks.
- Inline terminal image rendering protocols (Kitty, iTerm2, Sixel, half-block graphics); image references display textual fallbacks and will be addressed in a future milestone.
- Mathematical equation rendering (LaTeX / MathJax).
- Embedded full-featured Markdown editing within the TUI; interactive editing delegates to the user's configured external `$EDITOR`.
- Binary attachments stored directly inside the knowledge base.
- Bi-directional backlink graphs, knowledge visualization, or daily-journal workflows.

## Constitution Check

- **Principle I (Plain Files Are the Source of Truth)**: Pass. Every note is stored as an individual UTF-8 Markdown file with standard YAML frontmatter under `~/kb/note/`.
- **Principle II (The Index Is Disposable)**: Pass. Note index data (mapped to `meta` and `payload` FTS5 columns) is fully reconstructible via `kb reindex`. `INDEX_VERSION` is bumped to trigger automated rebuilds.
- **Principle III (Local-Only, No Server)**: Pass. Note rendering, indexing, and search execute purely locally with zero outbound network calls.
- **Principle IV (Terminal Only, One Core)**: Pass. Markdown rendering is strictly terminal-focused (`ratatui` + ANSI). Core parsing and indexing live in `kb-core`; CLI and TUI consume the shared core.
- **Principle V (Portable by Construction)**: Pass. Platform-specific capabilities (terminal hyperlink emission, editor invocation) use existing platform abstractions.
- **Principle VI (Secrets Are Never Stored)**: Pass. The full note body is subjected to M1 secret scanning rules on save, rejecting credentials, bearer tokens, and private keys.
- **Principle VII (Keyboard-First and Fast)**: Pass. Notes support complete keyboard navigation, fast preview scrolling, and bounded search latency under 50 ms.
- **Principle VIII (Test-First Core)**: Pass. Acceptance test scenarios cover round-trip serialization, indexing, ranking, sanitization, and PTY rendering.
- **Principle IX (Simplicity First)**: Pass. No new external crates are added; diagrams and image rendering are explicitly out of scope.
- **Technical Constraints**: Pass. Minor amendment to `kb_format: 1` entry types adding `note`.
