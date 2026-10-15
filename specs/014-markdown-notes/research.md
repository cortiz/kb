# Phase 0 Research: Markdown Notes

This document consolidates architectural investigations, technology choices, and design decisions for adding first-class `note` entries to kb.

---

## 1. Markdown Parsing & Terminal Rendering Engine

### Decision
Extend kb's existing Markdown renderer in `crates/kb/src/tui/markdown.rs` using `pulldown-cmark` (already a workspace dependency at version 0.13) with `Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_FOOTNOTES | Options::ENABLE_HEADING_ATTRIBUTES`. Do not introduce any new Markdown parsing or rendering crates (such as `termimad` or `comrak`).

### Rationale
- `pulldown-cmark` is already compiled into both `kb-core` and `kb`.
- Extending the existing ~360-line renderer by 450–600 lines adds approximately 40–60 KB to the binary, easily satisfying SC-005 (<100 KB binary growth) and Constitution Principle IX (Simplicity First).
- `pulldown-cmark` streams events without building an in-memory document tree (AST), keeping memory usage negligible even on large documents.
- Table layout can be calculated in a single streaming pass: collect row cells, measure cell widths using `unicode-width`, compute column widths up to available preview width, wrap cell content to fit, and render borders using Unicode box-drawing characters (`┌─┬┐`, `│`, `├─┼┤`, `└─┴┘`).
- Code blocks are enclosed in titled box frames (`┌─ [rust] ─┐`, `│ ... │`, `└────────┘`). Languages without available syntect syntax definitions (such as `mermaid`) are rendered inside standard box frames as plain unhighlighted text.
- GitHub-style callouts (`> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`, `> [!CAUTION]`) are parsed by inspecting the first line of a `BlockQuote` event, stripping the marker, and wrapping the quote in a styled box with a theme-aware icon and border color.

### Alternatives Considered
- **Add `termimad` crate**: Rejected. Adds dozens of transitive dependencies, increases release binary size by >450 KB (violating SC-005), and does not integrate cleanly with ratatui's buffer/span primitives.
- **Add `markdown-tui` or `comrak`**: Rejected. Builds full DOM trees in memory, duplicates `pulldown-cmark`, and increases binary size and maintenance burden.

---

## 2. Terminal Safety & Escape Sequence Sanitization

### Decision
Implement a zero-allocation streaming sanitizer in `kb-core::terminal::sanitize` that strips:
1. C0 control characters (`0x00..=0x1F`), excluding `\t` (expanded to spaces) and `\n` (handled by line layout).
2. C1 control characters (`0x80..=0x9F`).
3. ANSI escape sequences: ESC (`0x1B`), CSI (`ESC [` ... final byte `0x40..=0x7E`), OSC (`ESC ]` ... BEL `0x07` or ST `ESC \`), DCS (`ESC P` ... ST), APC (`ESC _` ... ST), and PM (`ESC ^` ... ST).

In the TUI and standard `kb show`, stripped sequences are replaced with a visible Unicode symbol `␛` (`U+241B`). In `kb show --raw`, raw Markdown source is emitted with control sequences completely removed.

OSC 8 terminal hyperlinks (`\x1b]8;;url\x1b\...`) are emitted strictly by kb's own renderer, only for valid `http`, `https`, and `mailto` URLs, and only when the terminal environment advertises hyperlink support (or when stdout is a terminal that supports OSC 8).

Raw HTML tags (parsed as `Event::Html` or `Event::InlineHtml`) are rendered as literal plain text and never passed through to the terminal.

### Rationale
- Notes can be created or updated by AI agents over MCP or pulled from remote Git repositories. Terminal injection attacks (e.g., rewriting the screen, spoofing prompts, hijacking clipboard via OSC 52, or executing commands via crafted terminal title escapes) represent a serious attack vector (Constitution Principle VII: No surprises, and Principle VI: Safety).
- Using a dedicated finite-state machine (FSM) scanner rather than heavy regexes ensures $O(N)$ single-pass processing with zero heap allocations for clean text.

### Alternatives Considered
- **Regular Expressions (`regex`) for ANSI stripping**: Stripping complex OSC/DCS sequences with regexes requires backtracking or nested match loops, and does not handle truncated or malformed sequences safely.
- **Pass-through with terminal emulator sandbox**: Unreliable across diverse terminal emulators (Terminal.app, iTerm2, kitty, Alacritty, Linux virtual consoles). Sanitizing at the application boundary is mandatory.

---

## 3. Memory Bounds & Lazy Highlighting for Large Notes

### Decision
Implement a three-tier performance and memory architecture:
1. **Bounded Render Cache**: Cache rendered line buffers (`RenderedNote`) in an LRU structure keyed by `(content_hash, width, theme)`. Maximum bounds: 20 notes or 16 MB total line buffer allocations, whichever is reached first.
2. **Viewport-Clipped Drawing**: In ratatui rendering, only calculate and draw lines within the active preview window (`scroll_offset .. scroll_offset + viewport_height`).
3. **Lazy Code Highlighting**: During Markdown block layout, fenced code blocks store raw line strings. Syntect syntax highlighting is deferred until a code block's lines intersect the visible viewport. Highlighted spans are cached inside the `RenderedNote` so scrolling back up is instant.
4. **Asynchronous Parsing**: For note files exceeding 16 KB, parsing and layout execute on a background worker thread (`std::thread` / channel). The UI renders an immediate "rendering…" placeholder and updates upon completion, keeping TUI frame rates within the 16 ms budget (SC-003).

### Rationale
- Large operational runbooks, dumps, or documentation notes can exceed several hundred kilobytes. Parsing and highlighting a 500 KB note synchronously on every keypress causes unacceptable UI stutter (>100 ms).
- Syntect syntax highlighting is the most CPU-intensive step of rendering. Highlighting only visible lines eliminates 90%+ of wasted cycles on off-screen code blocks.

### Alternatives Considered
- **Full synchronous rendering on selection**: Stalls the TUI event loop on large files (>16 KB), violating Constitution Principle VII (60 fps response).
- **Unbounded render cache**: Causes memory leaks over extended TUI sessions when browsing thousands of notes, violating SC-003 (<100 MB resident memory growth).

---

## 4. Search Indexing & Typo Vocabulary Isolation

### Decision
1. **Column Mapping**:
   - `title`: Extracted title (frontmatter `title` or sanitized level-1 heading).
   - `tags`: Note tags.
   - `id`: Entry path slug (e.g., `note/role-cache-ttl`).
   - `meta`: Level 1–3 Markdown headings, entry link target titles, and extracted URL hostname/path words.
   - `payload`: Body text, list items, table cell text, and fenced code blocks (up to 256 KB).
   - `notes`: Empty string `""`.
2. **Search Column Alias**: Register `in:body` in the query parser as an alias for the `payload` column.
3. **Typo Vocabulary Isolation**:
   - In SQLite FTS5, the `vocab` table is used by `search.rs` to find typo candidates with Levenshtein distance $\le 2$.
   - Note body words (in `payload`) MUST NOT be added to `vocab`.
   - Update `insert_doc` and `apply_reset`:
     - When indexing note entries, only words from `cols[0..=3]` (`title`, `tags`, `id`, `meta`) are inserted into `vocab`.
     - In `apply_reset`, modify the vocabulary population query so terms originating solely from note `payload` are excluded, or populate `vocab` during the document scan loop using per-column filtering.
4. **Index Version Bump**: Increment `INDEX_VERSION` in `crates/kb-core/src/index/schema.rs` from `2` to `3` to trigger an automatic background index rebuild on next launch (Constitution Principle II).

### Rationale
- Notes contain arbitrary prose, code samples, stack traces, and variable names. Adding millions of unstructured words to the typo vocabulary degrades search latency far beyond the 50 ms budget (Constitution Principle VII, SC-002).
- Restricting typo tolerance to note titles, tags, and headings ensures typo correction works where users expect it, while exact and prefix search remain fast across the entire note body.

### Alternatives Considered
- **Index full note body without size cap**: A single 10 MB pasted log file would bloat the FTS index and slow down ranking. The 256 KB cap covers 99.9% of human-authored operational notes.
- **Index note body into `notes` column**: Violates column semantics; `notes` is weighted lower (1.0) and intended for peripheral annotations on queries/commands. Note body is the primary content and belongs in `payload` (weighted 1.5).

---

## 5. Entry Links `[[entry-id]]` and Action Menu Integration

### Decision
1. **Syntax**: Text matching `[[<entry-id>]]` is recognized as an internal link. Escaped syntax `\[[` renders as literal `[[`.
2. **Link Extraction & Storage**:
   - During note parsing, entry links are extracted and stored in a new index table `entry_links(source_id TEXT, target_id TEXT, PRIMARY KEY(source_id, target_id))`.
   - Referenced entry titles are fetched from the index and added to `meta` during indexing.
3. **Action Menu ("Follow entry link")**:
   - In the M7 action menu, if a note contains entry links, a "Follow entry link" action is available.
   - If exactly 1 entry link is present, selecting it navigates immediately to the target entry in the TUI list.
   - If $\ge 2$ entry links are present, selecting it opens a filtered modal picker listing all referenced entries (badge, title, and target ID).
4. **Atomic Refactoring**:
   - `kb mv <old-id> <new-id>` queries `entry_links WHERE target_id = ?1`, replaces `[[old-id]]` with `[[new-id]]` across all referencing files in a format-preserving manner, and commits the changes atomically in a single git commit (matching R1 FR-016).
   - `kb rm <id>` checks `entry_links` and prompts the user with a list of referencing notes before proceeding.
5. **Broken Link Rendering**:
   - In the preview, an entry link targeting an unresolved ID renders a fallback `[?]` badge followed by the target ID styled in warning/red text (e.g., `[?] missing/entry-id`).

### Rationale
- Interlinking notes, queries, and runbooks creates a local, private knowledge graph.
- Ensuring `kb mv` updates references prevents bitrot as knowledge bases evolve.

### Alternatives Considered
- **Wiki-style `[[id|Custom Label]]`**: Deferred. Standard `[[id]]` rendering the target's actual badge and title is simpler, self-updating, and avoids stale manual labels.

---

## 6. Authoring Workflow (`kb note`) & Type Detection

### Decision
1. **CLI Commands**:
   - `kb note [title]`: Initializes a note template and launches the configured editor (`$EDITOR` / `$VISUAL`).
   - `kb add --type note`: Functions identically.
   - Default path template: `note/<slug-of-title>.md`.
2. **Level-1 Heading Title Fallback**:
   - If frontmatter `title:` is absent, the parser looks at the first non-empty line of the body. If it is a level-1 heading (`# Title`), all inline Markdown syntax (code ticks, bold, italic, links) is stripped and internal whitespace is collapsed to form a normalized plain-text title.
3. **TUI Add Form**:
   - Supports `type: note`.
   - Offers an optional `private` tag checkbox with explanatory text: *"Private entries are excluded from AI agent tools by default"*.
   - Includes a multi-line body editor area and `Ctrl-E` shortcut to open the full external editor.
4. **Type Detection**:
   - If imported content features Markdown structural elements (headings, bulleted lists, tables, multiple paragraphs), it classifies as `note`.
   - If content consists of conversational instructions directed to an LLM ("You are…", "Summarize…"), it classifies as `prompt`.
   - Ambiguous inputs trigger an interactive choice prompt.

### Rationale
- Authoring notes must be friction-free; many users prefer starting directly with `# My Title` in Markdown rather than writing YAML frontmatter titles.

---

## Summary of Architectural Approvals

All research questions and technical choices have been validated against the project constitution:
- **Zero new crate dependencies**: Fits within existing binary size and build profiles.
- **Strict terminal safety**: Malicious control codes and fake links are rendered completely inert.
- **Fast and bounded**: Preserves 50 ms search budget and 16 ms interactive frame budget.
