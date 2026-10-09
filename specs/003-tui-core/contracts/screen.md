# Contract: screen layout and formats

## Regions (top to bottom)

```text
 kb  ~/kb · 284 entries · ⚠ 3 with errors · refreshing…        row 1  header
 › 5xx auth█                                          7 / 284   row 2  search line
 all 284 · sql 41 · sumo 33 · cw 20 · … [tag:prod ×]            row 3  chips
┌ results ───────────────┬ preview ───────────────────────────┐ rows 4 .. h-1
│ sumo  5xx by authorizer│ 5xx by authorizer                  │
│ #auth #lambda  ×7      │ sumo · #auth #lambda · ×7 · 2026-09-30
│ cw    Authorizer latency│ sumo/5xx-by-authorizer.md          │
│ …                      │ ── payload ────────────────────    │
│                        │  1 _sourceCategory=prod/api | …    │
└────────────────────────┴────────────────────────────────────┘
 ↑↓ select · Enter pick · Tab type · ^Y copy · ^E edit · ? help row h   key bar
```

- **Header**: `kb`, KB root with the home directory shown as `~`, `N entries`, `⚠ M with errors` when
  M2 reports skipped entries, `refreshing…` while a refresh is running. Truncated with `…` from the
  middle of the root path if too long.
- **Search line**: `› ` prompt, the text, the cursor (a reverse-video cell; hidden cursor otherwise),
  and on the right `matches / entries` (`7 / 284`): how many entries match the query (before paging,
  at most 1,000 for a text query) out of all indexed entries. With no results `0 / 284`.
- **Chips row**: `all N` and one chip per type with its count (`sql 41`); the active type chips are
  highlighted, types with count 0 are dimmed; then one chip per other filter with its value and `×`.
  When the row is too narrow the chips shrink in this order: types with no results go, then the counts on
  the remaining types, then the inactive types. Active filters (the active type, `tag:`, `-tag:`, ...)
  stay; only if they still do not fit are filters dropped from the right. Never wrapped.
- **Body**: a bordered list on the left and a bordered preview on the right.
  - Width ≥ 100: list is 40% (at least 34 columns), preview the rest.
  - 60 ≤ width < 100: list only; Alt-P shows the preview full screen (`FullPreview` mode, no list).
  - Height or width below 60×15: the whole screen is replaced by `Terminal too small` (centered)
    until resized.
- **Key bar**: [keys.md](keys.md#key-bar-bottom-row). Always one row.

Rows used by non-body regions: 4 (header, search, chips, key bar). Body height = height − 4.

## List rows (FR-005, FR-006)

Two lines per result, plus no blank line between results:

```text
 sumo  5xx by authorizer          ← line 1: type badge, title
 #auth #lambda            ×7      ← line 2: tags (dim), use count right-aligned (×N, blank when 0)
```

- Badge: the type name padded to 6 columns in the type color, bold (just bold with `NO_COLOR`). On
  the selected row it keeps its color on top of the selection background.
- Selected row: both lines have the selection background (not only the text color); with `NO_COLOR`
  both lines are reverse video.
- On a terminal too short for the full list the help overlay becomes two columns of one short line per binding; both forms list every binding.
- Match highlighting: characters in the title and tags that fall in M2's `matches` ranges for
  `title` and `tags` are drawn with the match style (bold + underline + accent color; with
  `NO_COLOR` bold + underline). Tag ranges are over the tags joined by spaces and are mapped onto the
  displayed `#tag` list.
- Truncation: by display width, `…` at the end; a wide character that doesn't fit is replaced by a
  space; the use count is dropped before the tags.
- Scrolling: `list_offset` moves minimally to keep the selection visible. Empty KB and no-results
  messages replace the list contents (see below).

## Preview (FR-007)

From top to bottom, each part omitted when empty:

1. **Title** (bold). For `link`: the URL on the next line in the link class, underlined, full width,
   wrapped (never truncated).
2. **Metadata line**: type (type color) · tags · `used N times` (omitted when 0) · `edited
   YYYY-MM-DD` (the entry's `updated:` when present, otherwise the file's modification date).
3. **Path** relative to the KB root (muted).
4. For `code` entries: `repo`, `path`, `line` (and `–end`), `ref`, `symbol` as a small aligned list.
5. **Payload**: a rule line `payload`, then lines with right-aligned line numbers (muted) and
   highlighted text; lines longer than the pane are cut with `›` at the right edge (no wrapping, no
   horizontal scroll). `{{variable}}` is drawn in the variable style wherever it appears.
6. **Variables**: rule line `variables`, one row per variable: `name  kind  options  default  — description`.
   `secret` shows `secret (prompted)` and never a value.
7. **Fields**: type-specific values (dialect, database, log groups, range, region, shell, cwd).
8. **Notes**: rule line `notes`, then rendered Markdown wrapped to the pane width.

Scrolling (Shift-↑/↓, Shift-PgUp/PgDn) moves a line offset over the whole preview; the offset resets
when the selection changes. When the preview is longer than the pane a muted `↓ more` / `↑ more`
appears on the right edge of the last / first visible line.

While the preview is loading (cold highlighter), the previous preview stays and the title row shows
`…`; the new one replaces it as soon as it arrives.

## Empty and error states

| State | List area | Preview area |
|-------|-----------|--------------|
| Empty KB | `No entries yet. Run kb init or add a file to ~/kb.` (`~/kb` is the real root) | blank |
| No results | `No matches` | search tips: `word word  all words must match`, `"exact phrase"`, `type:sql  tag:prod  -tag:old`, `var:region  repo:owner/name  in:title` |
| Refresh failed | previous list kept | previous preview kept; key bar shows the failure |
| Too small | whole screen: `Terminal too small` | |

## Type colors and classes

Fixed per type, readable on both backgrounds (ANSI-256 index; the `NO_COLOR` theme uses none):

| Type | Color | Index |
|------|-------|-------|
| sql | blue | 33 |
| sumo | orange | 208 |
| cw | olive green | 106 |
| cmd | green | 34 |
| prompt | magenta | 127 |
| link | teal | 30 |
| code | brown-red | 131 |

Syntax classes (dark / light): Keyword 141 / 54, String 107 / 22, Number 173 / 130, Comment 244 / 244
(italic), Operator 250 / 240, Function 110 / 25, Field 179 / 94, Type 79 / 30, Variable (bold,
reverse accent), Heading bold, Link 74 / 25 underlined. With `COLORTERM=truecolor` the same colors may be
used as RGB equivalents; the snapshot tests run in 256-color mode.

## Wide characters and resizing

All widths use display columns (`unicode-width`). A resize action re-computes the layout from the
state alone and keeps query, chips, selection (by ID) and preview scroll (clamped).

## Snapshot format

`tests/snapshots/tui/<case>.<WxH>.<theme>.txt`: the screen as lines of text (trailing spaces trimmed),
then a `--- styles ---` section listing, per line, runs `col..col fg bg mods` for cells whose style
differs from the default. Update with `KB_UPDATE_SNAPSHOTS=1 cargo test -p kb --test tui_snapshots`.
Cases: one per entry type from the fixture KB, plus empty KB, no results, help overlay, narrow
terminal, full preview, too small, long titles with CJK and emoji. Sizes 80×24, 120×40, 200×60;
themes `dark`, `light`, `nocolor` (SC-004).
