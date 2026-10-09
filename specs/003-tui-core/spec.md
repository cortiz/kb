# Feature Specification: TUI Core — Search and Preview

**Feature Branch**: `003-tui-core`
**Created**: 2026-10-08
**Status**: Draft
**Milestone**: M3 (was M4; swapped with the platform layer)
**Depends on**: M1 (`001-entry-format`), M2 (`002-index-search`)
**Input**: A full-screen terminal UI to search the KB as you type, browse results, read a rich preview, and pick an entry. This is screen 1 of the TUI design.

## Overview

This milestone delivers the main way kb will be used: run `kb`, type a few characters, see the right entry with its full preview, press Enter. It builds only on M1 and M2, so it deliberately avoids OS-specific features.

Because the native clipboard layer moves to M4, picking an entry in this milestone works the way `fzf` does: kb exits and prints the selection to stdout, so `kb | pbcopy` or `$(kb)` work right away. A terminal-native copy key (OSC 52) is included as a best-effort extra. M4 will add native clipboard support and make Enter copy by default.

## Clarifications

### Session 2026-10-08

- Q: Should the TUI check the disk for changed files on every keystroke, or only at certain moments? → A: When the TUI opens, on Ctrl-L, after Ctrl-E returns, and at most once per second while the user types or the TUI is idle.
- Q: When a pick is piped or captured, should kb add a newline after the payload? → A: No. Pick output is printed exactly as stored, with no added newline, whether or not stdout is a terminal.
- Q: If a type filter is active from Tab and the query also contains `type:`, how do they combine? → A: There is one type filter. Tab writes a `type:x` filter into the query (replacing any earlier type filter), and typing `type:x` makes the matching type chip active.
- Q: What should "last edited" in the preview show? → A: The entry's `updated:` date when present, otherwise the file's modification time.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Search as you type and pick an entry (Priority: P1)

Carlos runs `kb`, types `5xx auth`, sees the Sumo query at the top of the list with its preview on the right, and presses Enter. kb exits and the query text is printed, ready to pipe.

**Why this priority**: this is the core loop the whole product is designed around.

**Independent Test**: start the TUI against the fixture KB in a pseudo-terminal, send keystrokes, and assert on the rendered screen buffer and on stdout after exit.

**Acceptance Scenarios**:

1. **Given** the user runs `kb` in a terminal, **When** the TUI opens, **Then** it shows the header, an empty search line with a cursor, the type filter chips, the full entry list ordered by frecency, the preview of the first entry, and the key bar.
2. **Given** the user types `5xx auth`, **When** each key is pressed, **Then** the list updates to the M2 search results for the current text, the first result is selected, and the preview follows the selection.
3. **Given** an entry is selected, **When** the user presses Enter, **Then** the TUI closes, the terminal is restored exactly as before, the entry's **pick output** (FR-017) is printed to stdout, and the use is recorded.
4. **Given** the user runs `kb 5xx auth`, **Then** the TUI opens with that text already in the search line.
5. **Given** the user presses Esc with text in the search line, **Then** the search text is cleared. **Given** the search line is empty, **When** the user presses Esc, **Then** the TUI exits with code 130 and prints nothing.
6. **Given** the user runs `kb | pbcopy` or `x=$(kb)`, **Then** the TUI still opens and is drawn on the terminal, and only the pick output goes to the pipe or the substitution.

---

### User Story 2 — Read an entry fully before using it (Priority: P1)

Carlos moves through results and reads each preview: the title, metadata, the payload with syntax highlighting and variables marked, the declared variables, and the notes.

**Why this priority**: queries look alike. Picking the right one depends on seeing it clearly.

**Independent Test**: select each fixture entry type and compare the rendered preview with a stored snapshot.

**Acceptance Scenarios**:

1. **Given** a `sumo`, `cw`, `sql` or `cmd` entry is selected, **Then** the preview shows its payload with syntax highlighting suited to its type, line numbers, and every `{{variable}}` visibly marked.
2. **Given** an entry declares variables, **Then** the preview lists each one with its kind, options and default.
3. **Given** the entry has notes, **Then** they are shown below the payload with basic Markdown rendering (bold, italic, inline code, links, lists).
4. **Given** a `link` entry, **Then** the preview shows the URL prominently. **Given** a `code` entry, **Then** it shows repo, path, line, ref and symbol.
5. **Given** the payload is longer than the preview pane, **When** the user presses Shift-↑/Shift-↓ (one line) or Shift-PgUp/Shift-PgDn (one page), **Then** the preview scrolls without changing the selection.

---

### User Story 3 — Narrow by type and tag without typing filters (Priority: P2)

Carlos presses Tab to cycle through type filters and sees the counts per type for the current search.

**Why this priority**: filtering by type is the fastest way to cut a long list, and the chips make the KB's shape visible.

**Independent Test**: type queries and press Tab against the fixture KB, then assert on the chips row and the list.

**Acceptance Scenarios**:

1. **Given** results are shown, **Then** each type chip shows how many current results have that type, and types with zero results are dimmed.
2. **Given** the user presses Tab or Shift-Tab, **Then** the active type filter moves to the next or previous type, cycling through "all", and the search line shows the matching `type:` filter (or none for "all"). There is a single type filter: Tab replaces any `type:` already typed, and typing `type:sql` makes the `sql` chip active. If several types are typed (`type:sql type:cw`), all their chips are active, and Tab then moves to the first type after them.
3. **Given** the user types `tag:prod` in the search line, **Then** it appears as a removable tag chip, and Backspace on an empty search line removes the last chip.
4. **Given** M2's query syntax (`type:`, `-tag:`, `"phrases"`) is typed, **Then** it works exactly as in `kb search`.

---

### User Story 4 — Fix an entry on the spot (Priority: P2)

While reading a preview, Carlos notices a mistake, presses Ctrl-E, edits the file in his editor, saves, and is back in the TUI with the change visible.

**Why this priority**: finding a wrong entry during an incident should never mean leaving the tool to fix it later.

**Independent Test**: set `EDITOR` to a script that modifies the file, press Ctrl-E in a pseudo-terminal, and assert on the screen after the editor exits.

**Acceptance Scenarios**:

1. **Given** an entry is selected, **When** the user presses Ctrl-E, **Then** the TUI suspends, `$VISUAL` (or `$EDITOR`, or `vi`) opens the entry's file, and after the editor exits the TUI resumes with the same query and selection.
2. **Given** the edited file now has lint errors, **Then** the TUI shows a notice naming the first error, and the entry drops out of results until fixed.

---

### User Story 5 — Learn the keys (Priority: P3)

**Why this priority**: the TUI is keyboard-only, so the keys must be discoverable without leaving it.

**Independent Test**: press `?` and F1 in a pseudo-terminal and compare the overlay with the key table.

**Acceptance Scenarios**:

1. **Given** the user presses `?` on an empty search line or F1 at any time, **Then** a help overlay lists every key binding, and any key closes it.
2. The key bar always shows the most important bindings that work in the current state, and nothing that doesn't work yet.

---

### Edge Cases

- **Empty KB**: the list area shows "No entries yet. Run `kb init` or add a file to ~/kb." and Enter does nothing.
- **No results**: the list shows "No matches" and the preview area shows search tips (filters and quoting).
- **Skipped entries**: when M2 reports entries skipped for lint errors, the header shows a warning count, such as `⚠ 3 with errors`.
- **Narrow terminal** (fewer than 100 columns): the preview is hidden and Alt-P toggles it full screen. Below 60×15 the TUI shows only "Terminal too small" until resized.
- **Resizing** while open re-lays out immediately without losing the query or selection.
- **Long titles and tags** are truncated with an ellipsis. Wide characters (CJK, emoji) never break the layout.
- **Files changing while the TUI is open**: M2's freshness check runs when the TUI opens, after Ctrl-E returns, on Ctrl-L, and at most once per second while the user types or the TUI is idle, never on every keystroke. A change made elsewhere therefore shows up within about a second. A deleted selected entry moves the selection to the nearest remaining result.
- **Crashes, panics, Ctrl-C and SIGTERM** MUST always restore the terminal (leave the alternate screen, show the cursor, disable raw mode).
- **No terminal available**: running `kb` with no arguments when there is no terminal to interact with (stdin is not a terminal and there is no controlling terminal either) prints the help text instead of starting the TUI, so scripts never hang. A redirected stdout alone is not a reason to skip the TUI.
- **Over SSH and in tmux**: the TUI works. OSC 52 copy works only where the terminal and tmux allow it.
- **`NO_COLOR` set**: the TUI uses no colors and relies on reverse video and bold to show selection and matches.

## Requirements *(mandatory)*

### Launching

- **FR-001**: `kb` with no subcommand MUST open the TUI whenever a terminal is available for interaction: stdin is a terminal, or the controlling terminal can be opened. Whether stdout is a terminal MUST NOT matter. `kb tui [query...]` MUST open it explicitly. Positional words after `kb` that are not subcommands MUST become the initial query.
- **FR-002**: The TUI MUST draw in the terminal's alternate screen and restore the terminal fully on every exit path.
- **FR-003**: The first frame MUST be drawn before the index freshness check completes when that check is slow, showing a "refreshing" indicator in the header.
- **FR-004a**: The TUI MUST draw on the terminal, never on stdout. Stdout carries only the pick output (FR-017) and only after the terminal has been restored, so `kb | pbcopy` and `$(kb)` receive exactly the pick and nothing else.

### Layout (screen 1 of the design)

- **FR-004**: The screen MUST contain, top to bottom:
  - a **header** with the KB root, entry count and any warning count,
  - the **search line** with a cursor and a result counter (`7 / 284`),
  - the **filter chips** row (types and active tags),
  - a **body** split into the result list (left) and the preview (right),
  - the **key bar**.
- **FR-005**: Each list row MUST show a type badge colored by type, the title, and a second line with the tags and use count. The selected row MUST be visually distinct by background, not only by color of text.
- **FR-006**: Matched characters in titles and tags MUST be highlighted using the match ranges provided by M2's search API.
- **FR-007**: The preview MUST show: title; a metadata line (type, tags, use count, last edited: the entry's `updated:` date when it has one, otherwise the file's modification time); the file path relative to the KB root; the payload, highlighted and with line numbers; variables; type-specific fields; and notes.
- **FR-008**: Type colors MUST be consistent everywhere: `sql`, `sumo`, `cw`, `cmd`, `prompt`, `link`, `code` each have one fixed color, readable on both dark and light terminal backgrounds.
- **FR-009**: The TUI MUST support `theme = auto | dark | light` in config. `auto` detects the terminal background where the terminal reports it and falls back to `dark`.

### Interaction

- **FR-010**: Key bindings for this milestone:

  | Key | Action |
  |---|---|
  | typing | edit the query |
  | ↑ ↓, Ctrl-P/Ctrl-N, Ctrl-K/Ctrl-J | move the selection |
  | PgUp PgDn, Ctrl-Home Ctrl-End | move by page, jump to first or last result |
  | Enter | pick: print the pick output and exit |
  | Ctrl-Y | copy the payload with OSC 52 and stay open |
  | Tab, Shift-Tab | cycle the type filter |
  | Shift-↑ Shift-↓, Shift-PgUp Shift-PgDn | scroll the preview |
  | Alt-P | toggle full-screen preview (narrow terminals) |
  | Ctrl-E | edit the selected entry in `$VISUAL`/`$EDITOR` |
  | Ctrl-L | refresh from disk |
  | ? (empty query), F1 | help overlay |
  | Esc | clear query, or exit when empty |
  | Ctrl-C | exit immediately |

- **FR-011**: The search line MUST support standard line editing: Left/Right to move the cursor, Ctrl-A to jump to the start of the line, Ctrl-W to delete the previous word, and Ctrl-U to clear the line. Ctrl-E is reserved for editing the entry (US4), so the End key jumps to the end of the line (and Home to the start). No binding may be listed twice in the KeyMap; the build MUST fail if it is.
- **FR-012**: Results MUST update on every keystroke. If a search is still running when the next key arrives, the older search MUST be discarded rather than drawn.
- **FR-013**: Mouse support is optional in this milestone. If included, wheel scrolling and click-to-select MUST be the only behaviors, and it MUST be possible to turn mouse capture off in config so terminal text selection still works.

### Picking and copying

- **FR-014**: Picking an entry (Enter) MUST record a use through M2's usage API.
- **FR-015**: Ctrl-Y MUST copy the payload using the OSC 52 escape sequence, show "Copied (terminal clipboard)" in the key bar for 2 seconds, and record a use. kb cannot confirm that the terminal accepted it, so the message MUST NOT claim more than that.
- **FR-016**: Payloads that still contain `{{variables}}` MUST be output as-is in this milestone, with variables unresolved. Resolution arrives in M5.
- **FR-017**: `--print <mode>` MUST select the pick output:
  - `payload` (default): the payload for `sql`, `sumo`, `cw`, `cmd`, `prompt`; the URL for `link`; `path:line` for `code`;
  - `id`: the entry ID;
  - `path`: the absolute file path;
  - `json`: the entry as JSON.
  - Whatever the mode, kb adds nothing to the pick output, in particular no trailing newline, so `kb | pbcopy` copies exactly the payload. (`json` output ends with a newline like any JSON document printed by kb.)
- **FR-018**: The exit code MUST be `0` after a pick, `130` when the user cancels (Esc or Ctrl-C), and `2` on errors.

### Performance and robustness

- **FR-019**: The TUI MUST render a single frame in at most 16 ms on a KB of 10,000 entries, so typing never stutters.
- **FR-020**: All entry access MUST go through the M1 and M2 core APIs. The TUI layer MUST NOT read entry files or the index directly.
- **FR-021**: Rendering MUST be testable without a real terminal: the screen MUST be reproducible from a given state and size, so snapshot tests can verify layout.

### Key Entities

- **AppState**: query text, parsed filters, active type filter, result list, selected index, preview scroll offset, layout mode (split or full preview), transient messages.
- **Theme**: the colors for each type, selection, matches, text, muted text and warnings, in a dark and a light variant.
- **KeyMap**: the mapping from keys to actions, defined in one place so the help overlay and key bar are generated from it.

## Success Criteria *(mandatory)*

- **SC-001**: On a KB of 10,000 entries, the first frame appears within 100 ms of running `kb`.
- **SC-002**: The time from a keystroke to the updated result list being drawn is 50 ms or less.
- **SC-003**: In 100 automated runs that exit by Enter, Esc, Ctrl-C, SIGTERM and a forced panic, the terminal is restored correctly every time.
- **SC-004**: Snapshot tests cover every entry type's preview at 80×24, 120×40 and 200×60, in dark, light and `NO_COLOR` modes.
- **SC-005**: With the fixture KB, finding and picking a named entry from a cold start takes under 5 seconds for a user who knows its title.
- **SC-006**: The TUI behaves the same on macOS (Terminal.app, iTerm2) and Linux (one VTE-based terminal and one GPU terminal such as Alacritty or kitty), and inside tmux.

## Out of Scope

- Native clipboard (pbcopy, wl-copy, xclip) and making Enter copy by default (M4).
- Opening links, running commands, opening console URLs (M7).
- Filling in variables (M5).
- The add-entry form (M6) and the code-link actions (M8).
- Git status in the header (M10).
- Customizable key bindings.

## Assumptions

- Printing the pick to stdout, as `fzf` does, is an acceptable stand-in for clipboard copy until M4, and is useful afterwards for scripting.
- Users run a terminal with at least 256 colors. True color is used when the terminal advertises it.
- Syntax highlighting for Sumo and CloudWatch Insights uses small custom grammars, since no common grammar exists for them. SQL and shell use standard grammars.
- The TUI needs a few read-only additions to M2's core API: the entry count, use count and last-edited time of a result, and per-type counts for the current search that ignore the active type filter and are not cut off by the result limit. They are part of this milestone, not a separate one.
- Esc with text or chips in the query clears all of it (text and tag chips); Esc on a fully empty query exits.
- The config keys are `theme` (`auto`, `dark`, `light`) and `mouse` (`true` or `false`, default `false`) in the existing config file.
- Some terminals keep Shift-PgUp/Shift-PgDn for their own scrollback and never pass them to kb. Shift-↑/Shift-↓ and Alt-P still scroll and zoom the preview there, and the help overlay lists the keys either way.
