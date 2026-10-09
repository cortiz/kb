# Contract: TUI add form (screen 4) and list prompts

## Opening

| From | What happens |
|---|---|
| `Ctrl-N` in the list (Search mode) | `Effect::ReadClipboard` → `Action::ClipboardRead(ReadOutcome)` → `AddState::open(text)`; shows the form. Clipboard read blocks the effect thread ≤ 2 s; the list stays drawn. |
| `kb add` (no flags, terminal) | The TUI starts with the form already open on the draft built by `main`; closing it exits the UI with `Outcome::Added { id }` / `Outcome::Exit(130)`. |

Initial focus: **Title** when the draft has text and a detected type; **Type** when the body is empty,
the clipboard was unavailable, or confidence is `Low` (FR-006, US1.2). When the clipboard is
unavailable the header says why, once.

## Layout (wide terminal, ≥ the M3 wide breakpoint)

```text
┌ New entry · from clipboard · detected cw (high) ────────────────────────────────────────┐
│ Type    ( sql )( sumo )[ cw ]( cmd )( prompt )( link )( code )  │ Body                    │
│ Title   Authorizer p99 latency▌                                 │ fields @timestamp, ...  │
│ Tags    lambda, cw  ‹auth›‹api›                                 │ | filter ...            │
│                                                                 │ | stats pct(@dur,99)    │
│ Variables                                                       │   by bin({{ms}}ms)      │
│  fn   text        {{fn}}                                        │ ────────────────────    │
│  ms   number      {{ms}}                                        │ Notes (optional)        │
│  ▸ suggest  prod → {{env}}  enum   [space]                      │                         │
│                                                                 │                         │
│ Saves to  cw/authorizer-p99-latency.md                          │                         │
├─────────────────────────────────────────────────────────────────┴─────────────────────────┤
│ ^S save  ^E edit body  ^P preview  Tab next  Esc discard                                  │
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

- Left column (top to bottom): type chips (selected one in the type color, as the list badge), title
  input, tags input with completion chips under it, **Variables** (declared rows then suggestion rows),
  **Saves to** line (the derived path, dim; editable when focused).
- Right column: body (syntax highlighted by the existing highlighter, read-only here, scroll with
  `S-↑↓`), notes input (multi-line, plain). Secret findings highlight the offending range in the body
  with the error style and show `KB060 …` under the body.
- Narrow terminals: single column, body below the fields; `Ctrl-P` toggles between fields and body view.
- `link` and `code`: the body pane is replaced by their fields (`url` / repo, ref, path, line, end
  line, symbol), each a line input; no variables section for these types.
- Colors, `NO_COLOR` and the 16-color fallback follow M3's theme; no new theme tokens except
  `suggest` (reuses `dim` + `accent`).

## Fields and focus order

`Type → Title → Tags → Variables → Path → Notes → (Body view)`. Tab / Shift-Tab move through them.

| Field | Input |
|---|---|
| Type | `←` `→` change; changing re-renders chips, drops fields not valid for the new type (data-model rules), and keeps focus. |
| Title | line editor; live length counter near 120. |
| Tags | line editor, comma or space separates; typing shows up to 5 completions from `tag_counts` (prefix match, by frequency); `Tab` accepts the first completion when the cursor is in a partial tag, else moves to the next field. Invalid tags are shown in the error style. |
| Variables | `↑` `↓` select a row; on a declared row: `k` cycles kind (`text → number → enum → secret`), `d` edits the default (refused on secret), `o` edits options (enum), `l` marks literal (`\{{x}}`); on a suggestion row: `Space` accepts. |
| Path | line editor; shows the derived value dimmed; typing fixes it (`PathChoice::Fixed`); clearing it returns to auto. Clash and bad-ID errors appear here. |
| Notes | multi-line editor (`Enter` newline). |

## Keys (FR-026)

| Key | Action |
|---|---|
| Tab / Shift-Tab | next / previous field |
| ← → (Type) | change type |
| Ctrl-S | save |
| Ctrl-E | edit the body in the editor (suspends the UI like list Ctrl-E) |
| Ctrl-P | preview the entry as it will appear in the list (rendered from `Draft::render` through the M3 preview renderer); again or Esc returns |
| Space (suggestion row) | accept |
| Esc | discard; if `changed`, asks `Discard this entry? (y/N)` on the key bar |

The form handles its own keys (like M5's `FormState`); the shared KeyMap only gains the three list
bindings below. A test asserts no form key shadows a global one needed in forms (`Ctrl-C` still quits
with the same two-step as M3).

## Save

`Ctrl-S` → `AddState::submit()`:

1. Title empty → focus Title, key bar `a title is required`.
2. `Draft::target_id(|id| kb.id_taken(id))`; `NeedsPath` → focus Path.
3. `Draft::check`. `writable(false)` false → show the first finding on the key bar and in the body
   panel, focus the offending field (title/tags/path/url by rule; secrets focus the body view); nothing
   is written. **The form has no "save anyway"** (FR-017; that choice is only for the editor flows).
4. `Effect::SaveDraft(Box<Draft>)` → `Kb::create_entry` → `Action::Saved(id)`.
5. Success: the form closes, the list query is re-run, the new ID is selected (`Selection::By(id)`),
   key bar shows `Saved cw/authorizer-p99-latency` for the usual message time. Failure
   (`Exists`, `Io`) → back to the form with the message.

## Ctrl-E in the form

`Effect::EditBody(text)` → suspend terminal → `private_temp_file(ext_for_type)` → editor → read back →
resume. Exit code ≠ 0 or unchanged → body kept. Otherwise `Draft::set_body(new, redetect = true)`:
detection re-runs (a type the user chose explicitly is kept unless confidence becomes `High` and
differs; then the chip changes and the header notes it), variables and suggestions are rebuilt (existing
decisions kept by name), `changed = true`. An entry over 1,048,576 bytes → rejected with the limit message.

## List: Ctrl-N, Ctrl-R, Ctrl-X

| Key | Command | Behavior |
|---|---|---|
| Ctrl-N | `Command::Add` | opens the form (above). `↓` and `Ctrl-J` remain "next result". |
| Ctrl-R | `Command::Rename` | key bar becomes `Rename to: <id>▌`; `Enter` applies, `Esc` cancels. Pre-filled with the current ID, cursor at the end. Error (exists, bad ID) shows on the bar and keeps the prompt. |
| Ctrl-X | `Command::Delete` | key bar `Delete cw/foo? (y/N)`; only `y` confirms. |

Both act on the selected result (no-op with an empty list). After success the list re-queries; rename
selects the new ID; delete selects the next row (or the previous at the end). Messages: `Renamed to
cw/bar`, `Deleted cw/foo`. They use the same core functions and git behavior as `kb mv` / `kb rm`.

Bindings added to `keys.rs` (modes `SEARCH`; not in `FULL`, `HELP`, `SMALL` where the selection is not
editable): `ctrl('n')` → Add, `ctrl('r')` → Rename, `ctrl('x')` → Delete. `ctrl('n')` is removed from
`Down`. Help overlay rows: "add an entry from the clipboard", "rename the entry", "delete the entry"
(six `help.*` snapshots updated). `has_duplicates` must stay false.

## Effects and actions (names)

```text
Effect::ReadClipboard
Effect::EditBody(String)
Effect::SaveDraft(Box<Draft>)
Effect::RenameEntry { from, to }
Effect::RemoveEntry { id }
Action::ClipboardRead(ReadOutcome)
Action::BodyEdited(Option<String>)        // None = cancelled / unchanged
Action::Saved(String) | Action::SaveFailed(String)
Action::Renamed { from, to } | Action::Removed(String) | Action::OpFailed(String)
Outcome::Added { id }                      // only when started by `kb add`
```

## Tests this contract implies

- Unit (pure `AddState`): focus on open for each clipboard outcome; type change drops fields; accept
  suggestion edits the body and adds a declaration; dirty tracking; Esc confirm; submit error focus.
- Snapshot: wide, narrow, `link`, secret highlight, suggestion rows, preview (insta-style snapshots
  like `tui_snapshots.rs`).
- pty: SC-001 — with the fake clipboard holding a CW query, `Ctrl-N`, title text, `Ctrl-S` is ≤ 5
  non-text key presses (`Ctrl-N`, `Ctrl-S` = 2; with tags `Tab`, `Tab` = 4); Ctrl-E with a scripted
  editor; Ctrl-R and Ctrl-X end to end.
