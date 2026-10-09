# Contract: the variable form (screen 2)

## Opening

| Trigger | Behavior |
|---|---|
| Enter on an entry with variables, post-pick `Copy` or `Print(Payload)` | worker `LoadForm`; form opens with `then = Exit` |
| Ctrl-Y on an entry with variables | same, `then = Stay` |
| Enter/Ctrl-Y on an entry without variables | M4 behavior |
| Alt-C in the list | copies the unresolved payload (template), no form; message `Copied template`; records a use, remembers no values |
| `Show` or `--print id|path|json` | no form |

## Layout

Wide (≥ 100 columns): header `‹title›  field 2/3`; left column fields, right column preview. Narrow:
fields on top, preview under, max 8 lines. Footer key bar:
`Tab next · Enter copy · Alt-Enter print · ^Z reset · Esc back`.

Field row: `name  kind  [options or default]  ‹source›  [raw] [undeclared]`, then the editor line, then an
inline error line when invalid. Sources: `flag` never appears in the TUI; `env $NAME`, `remembered`,
`default`, `typed`, `—`. Secret from env: `•••• (from $SUMO_TOKEN)`, read-only; Ctrl-R clears it and
turns the field into masked input.

Preview: the resolved payload, long lines wrapped; substituted spans use the theme's variable style; secrets show `••••`;
quoted values show their quotes. It shows the top of the payload (no scrolling of its own yet:
`ponytail:` a payload taller than the pane is cut). Many fields: the field list scrolls, the preview stays.

## Keys (form mode)

| Key | Action |
|---|---|
| Tab, ↓ / Shift-Tab, ↑ | next / previous field |
| Alt-↓ | text field with suggestions: enter the list; then ↑/↓ choose, Enter/Tab accept (does not submit), Esc or typing closes it |
| ←, → | enum: previous/next option; text: cursor |
| Home/End, Ctrl-A, Ctrl-W, Ctrl-U | line editing as M3 FR-011 (Ctrl-E is not used here) |
| Enter | submit: validate all; first invalid gets focus and the message; else verify → resolved |
| Alt-Enter | same as Enter, outcome is print-and-exit |
| Ctrl-Z | reset focused field to its pre-filled value |
| Ctrl-R | secret from env: replace by typed input |
| Esc | close the suggestion list if it is open, else the form; back to the list with the same selection; typed values kept for the session |
| Ctrl-C | quit as elsewhere |

The form handles its own keys (`FormState::handle_key`); the shared KeyMap gains only `Alt-C`
(`Command::CopyTemplate`, list modes), so the build-time duplicate check covers it. `Mode::Form` exists
so the rest of the UI knows a form is open.

## Reading the entry (no worker round trip)

The effect handler in `tui/mod.rs` reads the entry, like it does for Ctrl-Y: `Effect::Pick` (Enter)
and `Effect::CopyPayload` (Ctrl-Y) call `FormSpec::load`, which returns `None` for an entry without
variables (then Enter picks and Ctrl-Y copies as before) and otherwise feeds `Action::FormLoaded` to
`update`. `Effect::SubmitForm` calls `recheck`, which loads the entry again and compares the plan
signature: the same → go on; different → `Action::FormChanged` (the form reopens with the notice);
no variables left → a message and the form closes. One file read, a millisecond; a worker thread
would only have added a protocol (the plan's R7 said "worker"; this is simpler and gives the same
two re-read points).

## Effects and outcomes

| Event | Effect |
|---|---|
| Submit OK, `then = Exit` | `Outcome::Resolved { id, values, print: bool }`; `main` re-resolves with the engine, copies/prints, remembers, records use |
| Submit OK, `then = Stay` | `Effect::CopyResolved { id, text, memory }` through the existing copy path; form closes; key bar shows the copy result; secret notice if any |
| Esc | `Overlay::None` |

`main` always re-runs `resolve` on `Outcome::Resolved` (authoritative); if that fails (entry changed
between verify and copy) it prints the errors to stderr and exits 1 with nothing copied.

## Tests

pty: fill each kind with keystrokes and compare the clipboard (fake tool on PATH) with the expected
text; invalid number blocks and focuses; Esc keeps values for the next open; Alt-Enter prints; Ctrl-Y
stays open; narrow layout at 80 columns; entry edited between open and submit reopens with a notice.
Snapshot tests for wide, narrow, secret-from-env, and error states.
