# Implementation Plan: Markdown Notes

**Branch**: `014-markdown-notes` | **Date**: 2026-10-14 | **Spec**: [specs/014-markdown-notes/spec.md](spec.md)

**Input**: Feature specification from `/specs/014-markdown-notes/spec.md`

---

## Summary

Milestone Notes (`014-markdown-notes`) introduces a first-class `note` entry type for free-form Markdown content (incident notes, architectural decisions, operational how-tos):
1. **Entry Format**: Adds `note` to `EntryType` in `kb_format: 1`. Notes have no separate payload block; their entire body is Markdown content. Frontmatter `title` is optional when the body starts with a level-1 heading.
2. **Terminal Markdown Renderer**: Extends kb's existing streaming `pulldown-cmark` renderer (`crates/kb/src/tui/markdown.rs`) to support tables (Unicode box borders, cell wrapping), task lists, callouts, fenced code block frames with syntax highlighting, strikethrough, footnotes, and entry links (`[[entry-id]]`).
3. **Terminal Safety**: A zero-allocation streaming sanitizer in `kb-core` intercepts C0/C1 control codes and ANSI escape sequences (CSI, OSC, DCS, APC, PM), replacing them with `␛` in TUI / `kb show` and stripping them in `--raw`. Raw HTML is rendered as literal plain text; terminal hyperlinks (OSC 8) are emitted only for verified `http`, `https`, and `mailto` URLs.
4. **Search & Typo Vocabulary Isolation**: Maps note headings to `meta` and body text to `payload` (indexed up to 256 KB); isolates note body words from the SQLite FTS5 `vocab` table to preserve the 50 ms search budget. Bumps `INDEX_VERSION` to 3.
5. **Memory Bounds & Performance**: Bounded LRU rendered line cache (20 notes or 16 MB), viewport-clipped drawing, lazy code block syntax highlighting, and asynchronous parsing for notes over 16 KB to protect M3's 16 ms interactive frame budget.
6. **Entry Links & Refactoring**: Internal `[[entry-id]]` references render as type badges and titles; broken links render as `[?] <entry-id>`; `kb mv` refactors links atomically with a single git commit; `kb rm` checks references before removal.
7. **Authoring & MCP**: `kb note [title]` command, multi-line TUI editor with `Ctrl-E`, optional `private` tag for AI agent protection, and full MCP server tool support (`search`, `get`, `create`, `update`).

---

## Technical Context

**Language/Version**: Rust 1.88 (2024 edition)  
**Primary Dependencies**: `pulldown-cmark` (0.13), `ratatui` (0.30), `crossterm` (0.29), `syntect` (5.x), `rusqlite` (0.40 bundled), `unicode-width` (0.2), `serde` (1.0), `serde_json` (1.0), `clap` (4.x). Zero new external crate dependencies.  
**Storage**: UTF-8 plain Markdown files (`~/kb/note/<slug>.md`) and SQLite FTS5 disposable index (`.kb/index.db`).  
**Testing**: `cargo test --workspace` with automated unit tests, roundtrip serialization tests, security PTY capture tests (`portable-pty`), and search ranking benchmarks.  
**Target Platform**: macOS (arm64, x86_64) and Linux (x86_64, aarch64); Terminal.app, iTerm2, kitty, Alacritty, tmux.  
**Project Type**: Terminal knowledge base CLI and TUI binary (`crates/kb`) over shared core library (`crates/kb-core`).  
**Performance Goals**:
- Search results returned in $\le 50$ ms on a warm 10,000-entry KB containing 1,000 notes (SC-002).
- TUI interactive frame budget maintained at $\le 16$ ms (60 fps) while scrolling large notes (SC-003).
- Notes $>16$ KB parsed asynchronously off the main thread with immediate placeholder.
- Resident memory growth $\le 100$ MB when browsing 200 notes consecutively (SC-003).  
**Constraints**:
- Terminal only, no HTML or browser view (Constitution Principle IV).
- Shared core: Parsing, sanitization, indexing, and entry link refactoring reside in `kb-core` (Constitution Principle IV).
- Strict safety: Malicious escape codes and unverified terminal hyperlinks never reach the terminal device (Constitution Principle VII, SC-004).
- Binary size growth $<100$ KB over v0.2.0 release (SC-005, Constitution Principle IX).  
**Scale/Scope**: 1 new entry type (`note`), 30 functional requirements, 6 success criteria, full MCP parity.

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evaluation |
|---|---|---|
| **I. Plain Files** | PASS | Notes are single UTF-8 Markdown files with standard YAML frontmatter in `~/kb/note/<slug>.md`. Valid without kb. |
| **II. Disposable Index** | PASS | Search and entry link relationships reside in `.kb/index.db`. Fully reconstructible via `kb reindex`. `INDEX_VERSION` bumped to 3 to trigger automatic rebuild. |
| **III. Local-Only** | PASS | Note authoring, rendering, sanitization, and indexing execute purely locally with zero network activity. |
| **IV. Terminal Only, One Core** | PASS | No HTML, webviews, or browser views. Markdown parsing, sanitization, and link management live in `kb-core`. CLI and TUI share rendering logic. |
| **V. Portable by Construction** | PASS | Cross-platform text measurement via `unicode-width`, platform-abstracted OSC 8 emission, standard POSIX/macOS path handling. |
| **VI. Secrets Never Stored** | PASS | Full note bodies are scanned against M1 secret detection rules during save and `kb lint`. |
| **VII. Keyboard-First and Fast** | PASS | 50 ms search latency preserved by isolating note body words from `vocab`. 16 ms render frame budget preserved by lazy highlighting and async parsing. |
| **VIII. Test-First Core** | PASS | Comprehensive test suite: roundtrip serialization, security PTY escape capture, search ranking accuracy (SC-001), and link refactoring tests. |
| **IX. Simplicity First** | PASS | Reuses existing `pulldown-cmark` and `syntect` dependencies with zero new crates. Diagrams and terminal image graphics are explicitly out of scope. |

---

## Project Structure

### Documentation (this feature)

```text
specs/014-markdown-notes/
├── spec.md              # Feature specification & clarifications
├── plan.md              # This implementation plan
├── research.md          # Phase 0 research & technology decisions
├── data-model.md        # Phase 1 data entities & schemas
├── quickstart.md        # Phase 1 end-to-end validation scenarios
├── contracts/           # Phase 1 interface contracts
│   ├── cli.md           # CLI commands (kb note, kb show, search filters)
│   ├── note-format.md   # File syntax, frontmatter & CommonMark extensions
│   └── mcp.md           # Model Context Protocol tool contracts
└── checklists/
    └── requirements.md  # Specification quality checklist
```

### Source Code Layout

```text
crates/kb-core/src/
├── entry.rs             # Add EntryType::Note to ALL, type keys, from_name, title extraction
├── entry/
│   └── link.rs          # NEW: EntryLink extraction, parsing [[id]] and \[[
├── terminal/
│   ├── mod.rs           # Terminal utilities
│   └── sanitize.rs      # NEW: Streaming C0/C1, ANSI CSI/OSC/DCS/APC/PM sanitization
├── index/
│   ├── schema.rs        # Bump INDEX_VERSION to 3, add entry_links table
│   ├── doc.rs           # Note column mapping (headings to meta, body to payload)
│   └── sync.rs          # Typo vocabulary isolation (exclude note payload from vocab)
├── author/
│   ├── draft.rs         # Note draft creation & template generation
│   ├── detect.rs        # Type detection (markdown structure vs prompt prose)
│   └── fsops.rs         # Note slug generation (note/<slug>)
├── lint/
│   └── rules.rs         # Lint rules: E-NOTE-TITLE, E-NOTE-VARS, W-NOTE-OVERSIZED, W-ENTRY-LINK-BROKEN
└── mcp/
    ├── engine.rs        # MCP search/get note support and private tag filtering
    └── write.rs         # MCP create/update note validation and persistence

crates/kb/src/
├── author.rs            # run_mv atomic link refactoring & run_rm reference checks
├── show.rs              # kb show <id> terminal rendering & --raw handling
├── tui/
│   ├── markdown.rs      # Extend renderer: tables, tasklists, callouts, frames, entry links
│   ├── render_cache.rs  # NEW: Bounded LRU RenderedNote cache (20 notes / 16 MB)
│   ├── preview.rs       # Viewport-clipped preview drawing & lazy syntect highlighting
│   ├── async_render.rs  # NEW: Background thread parsing for notes >16 KB
│   └── action_view.rs   # "Follow entry link" action & link picker modal
├── commands/
│   └── note.rs          # NEW: kb note [title] command implementation
└── main.rs              # CLI command registration (kb note)

crates/kb/tests/
├── tui_markdown_notes.rs # PTY tests for note preview, scrolling, and link following
├── security_sanitization.rs # PTY capture tests for malicious escape codes & fake links
├── cli_note.rs          # Integration tests for kb note, show, and search in:body
└── fixtures/notes/      # Test fixture notes (simple, complex, oversized, malicious)
```

---

## Complexity Tracking

*No constitution violations or unjustified complexity. Zero new crate dependencies.*
