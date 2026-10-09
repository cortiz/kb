# Data Model: TUI Core — Search and Preview

Types in `crates/kb/src/tui/` unless a path says `kb-core`. Spec entities: AppState, Theme, KeyMap.

## AppState

| Field | Type | Notes |
|-------|------|-------|
| `root` | `PathBuf` | shown in the header (with `~` for home) |
| `input` | `LineEditor` | free text and cursor (character index) |
| `chips` | `Vec<Chip>` | filters, in the order they were added |
| `type_filter` | derived | the `Chip::Type` entries; empty means "all" |
| `results` | `Vec<SearchResult>` | the current page from the worker (kb-core type) |
| `total` | `usize` | matches before paging (≤ 1,000 for text queries) |
| `type_counts` | `[usize; 7]` | per-type counts for the current text and non-type chips |
| `selected` | `usize` | index into `results`; kept by entry ID across refreshes |
| `list_offset` | `usize` | first visible row, so the selection stays on screen |
| `preview` | `Option<PreviewModel>` | for `results[selected]`; `None` while loading |
| `preview_scroll` | `usize` | first visible preview line; reset on selection change |
| `layout_mode` | `Split \| FullPreview` | Alt-P toggles; `FullPreview` only matters when narrow |
| `overlay` | `None \| Help` | `?` on empty text, F1 |
| `status` | `Status` | `refreshing: bool`, `indexed`, `skipped`, `last_refresh: Instant` |
| `message` | `Option<(Message, Instant)>` | transient key-bar text and its expiry (2 s for copy; "until next key" for lint notices) |
| `search_gen`, `preview_gen` | `u64` | incremented for every search / preview request; replies older than the current one are dropped |
| `preview_id`, `preview_mtime` | | the entry (and file time) the preview was last requested for; a changed file time reloads the preview |
| `size` | `(u16, u16)` | terminal size, updated by resize actions |
| `empty_kb` | `bool` | nothing indexed and nothing skipped |
| `options` | `Options` | `print_mode`, `mouse`, `theme` choice |

Invariants (checked by `update` unit tests): `selected < results.len()` or `results.is_empty()`;
`selected` is `0` after any change to text or chips; `preview_scroll` is `0` after any change of
selection; at most one overlay; `type_filter` has no duplicates.

## Chip and LineEditor

`Chip = Type(EntryType) | Tag(String) | NotTag(String) | Var(String) | Repo(String) | In(Field)`.

- A filter token followed by a space leaves the text and becomes a chip (`tag:prod `, `-tag:stage `,
  `type:sql `, `var:region `, `repo:acme/api `, `in:title `). A chip is never created for an
  invalid value (`type:nonsense` stays text, and M2 reports the hint).
- `Chip::Type` is a group: adding one by typing keeps existing type chips (OR, as in M2); Tab and
  Shift-Tab replace the whole group by one type.
- `to_query_string(chips, text)` renders chips as M2 syntax followed by the text. The worker calls
  `Query::parse` on it; tests assert equality with the equivalent `kb search` arguments.
- `LineEditor { text: String, cursor: usize }` in characters. Operations: insert, backspace, delete,
  left, right, home, end, delete-word-back (Ctrl-W: skips spaces, then deletes back to a space),
  clear (Ctrl-U). Every operation keeps the cursor on a character boundary and is tested on wide and
  combining characters.

## Action and Effect

`Action`: `Key(KeyEvent)`, `Mouse(MouseEvent)`, `Resize(w, h)`, `Tick(Instant)`,
`Results(generation, Page)`, `PreviewReady(generation, PreviewModel)`, `Refreshed(RefreshInfo)`,
`WorkerFailed(String)`, `EditorReturned(Option<FirstError>)`.

`Effect`: `Search { generation, query, limit }`, `LoadPreview { generation, id }`, `Refresh`,
`Pick(PrintMode)`, `CopyPayload`, `EditEntry(id)`, `RecordUse(id)`, `Quit(i32)`.

`update(state, action, now) -> Vec<Effect>` is a pure function of its inputs.

## Worker messages (`worker.rs`)

| Direction | Message | Fields |
|-----------|---------|--------|
| UI → worker | `Refresh` | none (coalesced) |
| | `Search` | `generation`, `query: String`, `limit: usize` (60 first; 0 = all) |
| | `Preview` | `generation`, `id` |
| | `Shutdown` | none |
| worker → UI | `Refreshed` | `indexed`, `skipped`, `duration`, `notices` |
| | `Results` | `generation`, `Page { results, total, type_counts, indexed, skipped, notices }` |
| | `Preview` | `generation`, `PreviewModel` |
| | `Failed` | `message` (the TUI shows it and keeps running; the fatal case is the index being unusable twice in a row, which exits 2) |

## PreviewModel

Built on the worker from `Kb::entry(id)`, theme-independent.

| Field | Type | Notes |
|-------|------|-------|
| `id`, `path` | `String` | path relative to the root |
| `title` | `String` | |
| `meta` | `Meta` | `entry_type`, `tags`, `use_count`, `edited: Option<String>` (the entry's `updated:` or the file time as `YYYY-MM-DD`) |
| `url` | `Option<String>` | for `link`: shown prominently, above everything else |
| `code` | `Option<CodeLine>` | for `code`: repo, path, line (and end line), ref, symbol |
| `payload` | `Vec<Vec<Span>>` | highlighted lines (no line numbers in the model); empty when the type has no payload |
| `vars` | `Vec<VarRow>` | name, kind, options, default, description, `required` hint; `secret` shows "(prompted)" and never a default |
| `fields` | `Vec<(String, String)>` | type-specific: dialect, database, log groups, range, region, shell, cwd |
| `notes` | `Vec<Block>` | Markdown blocks |
| `mtime` | `i64` | cache key part |

`Span { text, class }`. `Class` is: `Plain, Keyword, String, Number, Comment, Operator, Function,
Field, Type, Variable, Punctuation, Heading, Bold, Italic, Code, Link, Quote, Muted`.
`Block = Paragraph(Vec<Span>) | List { ordered, items: Vec<Vec<Span>> } | Quote(Vec<Span>) |
Code(Vec<String>) | Heading(Vec<Span>)`. Wrapping to the pane width happens in `view`, not here.

The worker caches `PreviewModel` by `(id, mtime)` (at most 64 entries, oldest dropped).

## Theme

`Theme { name: Dark | Light | NoColor, type_color: [Color; 7], text, muted, selection_bg, selection_fg,
match_style, warning, border, chip_active, chip_dim, var_style, class_style: fn(Class) -> Style }`.
Built by `Theme::resolve(choice, env)` from `theme` in config, `COLORFGBG`, `COLORTERM` and
`NO_COLOR`. Color values are in [contracts/screen.md](contracts/screen.md#type-colors-and-classes).

## KeyMap

`Binding { key: KeySpec, modes: ModeSet, action: Action, group: Group, label: &'static str,
bar: Option<u8> }`. `KeySpec { code: Code, mods: u8 }` uses the crate's own `Code` enum so the
duplicate check can run in a `const fn`. `Mode`: `Search`, `Help`, `TooSmall`, `FullPreview`. The
table, key bar order and help text: [contracts/keys.md](contracts/keys.md).

## kb-core additions (types)

- `SearchResult` gains `use_count: u64`, `updated: Option<String>`, `modified: Option<i64>` (unix
  seconds, from the file row).
- `SearchOutcome` gains `type_counts: [usize; 7]` (index = `EntryType::ALL` order), `indexed`,
  `skipped`.
- `Config { theme: ThemeChoice, mouse: bool }`, `ThemeChoice = Auto | Dark | Light`.
- `PayloadPiece = Text(String) | Var(String)` from `payload_pieces(&Entry)`.
- `Refresh { indexed, skipped, notices, duration }`.

## State transitions

```text
opening ──first frame──▶ loading (refreshing…, empty list) ──Results──▶ browsing
browsing ──text/chip change──▶ browsing (generation+1, selection 0)
browsing ──? / F1──▶ help ──any key──▶ browsing
browsing ──Ctrl-E──▶ editing (terminal released) ──editor exit──▶ browsing (+refresh, +notice)
browsing ──Alt-P (narrow)──▶ full preview ──Alt-P / Esc──▶ browsing
any ──Enter on a result──▶ picked ──▶ exit 0
any ──Esc on empty / Ctrl-C / signal──▶ cancelled ──▶ exit 130 (143, 129 for signals)
any ──size < 60×15──▶ too small ──resize──▶ previous
```
