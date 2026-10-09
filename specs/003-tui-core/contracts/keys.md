# Contract: key bindings

One table in `tui/keys.rs` generates the key bar, the help overlay and this document. A key may
appear only once per mode; the crate does not compile otherwise (FR-011).

## Modes

`Search` (default), `Help` (overlay open), `FullPreview` (narrow terminal, Alt-P), `TooSmall`
(below 60×15). In `Help` any key closes the overlay and is consumed. In `TooSmall` only Ctrl-C and
resizing do anything.

## Bindings

| Group | Key | Action | Modes |
|-------|-----|--------|-------|
| Pick | Enter | pick: print the pick output, exit 0 | Search, FullPreview |
| Pick | Ctrl-Y | copy payload with OSC 52, stay open | Search, FullPreview |
| Pick | Ctrl-E | edit the entry in the editor | Search, FullPreview |
| Move | ↓, Ctrl-N, Ctrl-J | selection down | Search |
| Move | ↑, Ctrl-P, Ctrl-K | selection up | Search |
| Move | PgDn / PgUp | selection by one page | Search |
| Move | Ctrl-End / Ctrl-Home | last / first result | Search |
| Filter | Tab / Shift-Tab | next / previous type filter | Search |
| Preview | Shift-↓ / Shift-↑ | scroll preview one line | Search, FullPreview |
| Preview | Shift-PgDn / Shift-PgUp | scroll preview one page | Search, FullPreview |
| Preview | Alt-P | toggle full-screen preview (narrow terminals) | Search, FullPreview |
| Line | ← / → | move the cursor | Search |
| Line | Home, Ctrl-A / End | start / end of line | Search |
| Line | Backspace | delete before cursor; on empty text removes the last chip | Search |
| Line | Delete | delete at cursor | Search |
| Line | Ctrl-W | delete previous word | Search |
| Line | Ctrl-U | clear the text line | Search |
| Line | typing | insert text; a space after a filter token turns it into a chip | Search |
| App | Ctrl-L | refresh from disk | Search, FullPreview |
| App | ?, F1 | help overlay (`?` only when the text line is empty; F1 always) | Search (`?`), Search and FullPreview (F1) |
| App | Esc | clear text and chips; exit 130 when both are empty; in FullPreview return to the list | Search, FullPreview |
| App | Ctrl-C | exit 130 immediately | all |

Notes:

- Ctrl-E is only "edit entry". End is the end-of-line key. The two never conflict.
- Terminals that don't send Ctrl-Home, Ctrl-End, Shift-PgUp or Shift-PgDn make those keys
  unavailable; the alternatives are PgUp/PgDn (move), Shift-↑/↓ (scroll) and Alt-P (zoom). The help
  overlay prints this sentence.
- Mouse (only with `mouse = true`): wheel over the list moves the selection by 3 rows, wheel over the
  preview scrolls by 3 lines, left click on a row selects it. Nothing else.

## Key bar (bottom row)

Built from the table by priority, left to right, dropping the lowest priorities until the row fits
the width; a transient message replaces it while it lasts.

| State | Key bar |
|-------|---------|
| Results shown | `↑↓ select · Enter pick · Tab type · ^Y copy · ^E edit · ? help · Esc clear` |
| Results shown, text empty | same, with `Esc quit` |
| Wide terminal, preview longer than the pane | adds `S-↑↓ scroll` before `? help` |
| No results | `Esc clear · ? help` |
| Empty KB | `Esc quit · ? help` |
| FullPreview | `Enter pick · ^Y copy · ^E edit · S-↑↓ scroll · Alt-P back` |
| Help overlay | `any key closes` |
| Too small | none |

Only bindings that work in the current state appear (no `^E edit` with no selection, no `Enter pick`
on an empty list).

## Transient messages

| Message | When | Lasts |
|---------|------|-------|
| `Copied (terminal clipboard)` | after Ctrl-Y | 2 s |
| `Too large for the terminal clipboard` | payload over 74,000 bytes | 2 s |
| `<path>:<line> <code> <text>` (warning style) | the file has an error after Ctrl-E | until the next key |
| `Refresh failed: <text>` | worker failure | until the next key |
| `Nothing to pick` | Enter/Ctrl-Y/Ctrl-E with no result | 2 s |

## Help overlay

A centered box listing every row of the table above, grouped, plus the clarification about
terminals that swallow keys. Rendered from the same table, so its content is a snapshot test.
