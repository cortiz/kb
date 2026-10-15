# Feature Specification: Vim Keymap and Configurable Key Bindings

**Feature Branch**: `013-vim-keymap`
**Created**: 2026-10-14
**Status**: Draft
**Milestone**: UX (spec 013; runbook run mode and run records move to 014 and 015)
**Depends on**: M3 (`003-tui-core`), M6 (`007-authoring`), M7 (`008-actions-code-links`), M9 (`009-git-sync`). Touches R1 (`011-runbook-format`) key bindings if it lands first.
**Input**: Offer an optional vim-style keymap in the TUI: single-letter actions (`o` open, `e` edit, `y` copy…) in a **normal mode**, while keeping type-to-search as the starting mode, and full navigation with both vim keys and arrow keys. Make every key binding configurable on top of either keymap.

## Overview

Today the TUI is emacs-style: the search line always has focus, so every action needs a modifier (Ctrl-O, Ctrl-E, Alt-A…). That keeps type-to-search instant, but modifiers are slower to type and some (Alt on macOS) depend on terminal settings.

This spec adds a second keymap, `vim`, chosen in config. It introduces two modes in the main list:

- **Search mode** (where kb starts): typing edits the query, exactly as today.
- **Normal mode** (after Esc): single letters act on the selected entry, and the query stays as it is.

Navigation works in both modes with arrow keys, and in normal mode also with `h j k l`. The default keymap stays `emacs`, so nothing changes for anyone who doesn't opt in. In `vim`, all existing Ctrl and Alt bindings keep working too, so the new mode only adds keys.

Both keymaps are **presets**. On top of the chosen preset, the user can rebind or unbind any command in any TUI context through a `[tui.keys.<context>]` section in config. A few keys are reserved so the user can never lock themselves out (Ctrl-C always quits) or weaken a safety check (run confirmations).

The keymap is already one table (`BINDINGS` in `tui/keys.rs`), with each binding gated by mode, and the key bar and help are generated from it. This spec adds a mode and rows to that table, and turns the table into the defaults for a keymap that is built at startup from the preset plus the user's overrides.

## Clarifications

### Session 2026-10-14

- Q: How should an unrecognized follow-up key be handled during a multi-key sequence (e.g., pressing any key other than `g` while waiting for the second `g` in `gg`)? (FR-010) → A: Cancel the sequence immediately and execute the new key as a standalone normal-mode command.
- Q: Does the `global` key context apply strictly to the main list modes (`search` and `normal`), or does it also cascade to sub-views like `action-menu`, `code-view`, and `help`? (FR-019) → A: Main list modes only (`search` and `normal`); sub-views are never affected by `global`.
- Q: What conditions constitute a failure causing `kb keys check` to exit with code 1? (FR-027) → A: Exit 1 on any configuration issue (unknown context/command/key name, duplicate overrides in the same context, reserved key rebind attempts, invalid printable keys/sequences in text contexts, or missing help/quit); exit 0 only when all overrides are completely clean.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Search, then act with single letters (Priority: P1)

Carlos runs `kb`, types `5xx auth`, presses Esc, then `o`. The Sumo query opens in the browser. He presses `j` twice and `y` to copy another entry, then `/` to search again.

**Why this priority**: This is the primary reason for the feature: enabling fast single-key actions without modifier chords while preserving instant type-to-search workflow.

**Independent Test**: PTY tests with `tui.keymap = "vim"` against the fixture KB: send keystrokes, assert on the mode indicator, the selection, and the action reported by the platform test double.

**Acceptance Scenarios**:

1. **Given** `tui.keymap = "vim"`, **When** kb starts, **Then** it is in search mode with the cursor in the query line, and the header shows `SEARCH`.
2. **Given** search mode, **When** the user presses Esc, **Then** the mode becomes normal, the header shows `NORMAL`, the query and results are unchanged, and the query line shows no cursor.
3. **Given** normal mode, **When** the user presses a bound letter (`o`, `e`, `y`, `!`…), **Then** the action runs on the selected entry, as its Ctrl or Alt equivalent does today.
4. **Given** normal mode, **When** the user presses `/`, **Then** search mode starts with the query cleared. **When** the user presses `i`, **Then** search mode starts with the query kept and the cursor at its end.
5. **Given** search mode, **When** the user presses Enter, **Then** the selected entry is picked, exactly as today (M3/M4).
6. **Given** normal mode with a non-empty query, **When** the user presses Esc, **Then** the query is cleared and the mode stays normal. **Given** an empty query, **When** the user presses Esc or `q`, **Then** kb exits with code 130.

---

### User Story 2 — Navigate with arrows or vim keys (Priority: P1)

Carlos moves through results with `j`/`k` sometimes and arrow keys other times, in either mode.

**Why this priority**: Arrow keys are universally familiar; vim navigation keys are expected by vim users. Both must function consistently.

**Independent Test**: Run TUI in automated PTY, sending combinations of `j`/`k`, `h`/`l`, arrow keys, `gg`/`G`, and PgUp/PgDn, verifying selection index, active type filter, and preview scroll offset.

**Acceptance Scenarios**:

1. **Given** either mode, **When** the user presses ↑ or ↓, **Then** the selection moves up or down.
2. **Given** normal mode, **When** the user presses `k` or `j`, **Then** the selection moves up or down.
3. **Given** normal mode, **When** the user presses ← / → or `h` / `l`, **Then** the type filter moves to the previous / next type (as Shift-Tab / Tab do).
4. **Given** search mode, **When** the user presses ← / →, **Then** the cursor moves within the query, as today.
5. **Given** normal mode, **When** the user presses Home / End or `gg` / `G`, **Then** the selection jumps to the first / last result. In search mode, Home / End move within the query, as today.
6. **Given** either mode, **When** the user presses PgUp / PgDn, **Then** the selection moves by a page. In normal mode, Ctrl-U / Ctrl-D also move by half a page.
7. **Given** either mode, **When** the user presses Shift-↑ / Shift-↓ or Shift-PgUp / Shift-PgDn, **Then** the preview scrolls, as today. In normal mode, `K` / `J` also scroll it by a line.

---

### User Story 3 — Nothing changes unless I opt in (Priority: P1)

A user who has not configured `tui.keymap = "vim"` experiences the exact same TUI behavior, keystrokes, and visual layout as before.

**Why this priority**: Existing muscle memory, documentation, automation, and test suites must not regress for current users.

**Independent Test**: Execute the entire existing suite of M3 TUI unit, PTY, and snapshot tests without any configuration changes and verify zero regressions.

**Acceptance Scenarios**:

1. **Given** no `tui.keymap` setting, **Then** the TUI behaves exactly as before this spec, key for key, and no mode indicator is shown.
2. **Given** `tui.keymap = "vim"`, **Then** every existing Ctrl and Alt binding still works in both modes (for example Ctrl-O opens, Alt-S syncs).
3. **Given** `kb --keymap vim` or `KB_TUI_KEYMAP=vim`, **Then** that keymap is used for that run (precedence as in M4 FR-015).

---

### User Story 4 — Discover the keys (Priority: P2)

Carlos forgets which key triggers an action in vim mode or wants to know how to navigate. He presses `?` or glances at the key bar at the bottom of the screen.

**Why this priority**: Modes and configurable bindings can cause confusion without clear in-context discovery.

**Independent Test**: Inspect rendered key bar in both search and normal modes in vim keymap, press `?` in both modes to inspect the help modal, and verify `g` multi-key sequence prompt in the key bar.

**Acceptance Scenarios**:

1. **Given** normal mode, **When** the user presses `?`, **Then** the help overlay lists every binding for the current keymap, grouped by mode.
2. **Given** either mode, **Then** the key bar shows the most useful keys for the current mode only (letters in normal mode, Ctrl/Alt keys in search mode).
3. **Given** a multi-key sequence is in progress (`g` pressed, waiting for the second `g`), **Then** the key bar shows `g…` until the sequence completes, is cancelled with Esc, or times out after 1 second.

---

### User Story 5 — Rebind any key to my liking (Priority: P1)

Carlos uses the vim keymap but wants `x` to run commands instead of `!`, `Ctrl-P` to open the action menu in search mode, and Alt-H unbound because his terminal intercepts it.

**Why this priority**: Terminals, keyboard layouts, and personal habits vary widely; hard-coded bindings inevitably collide with terminal emulators or window managers.

**Independent Test**: Provide fixture config files with overrides, conflicts, reserved keys, and invalid syntax. Start the TUI in a PTY and check effective bindings (`kb keys --json`), warnings, key bar, help, and verify rebound keys execute their commands.

**Acceptance Scenarios**:

1. **Given** this config:
   ```toml
   [tui]
   keymap = "vim"

   [tui.keys.normal]
   run = ["x", "!"]

   [tui.keys.search]
   action-menu = ["ctrl-p", "alt-a", "f2"]

   [tui.keys.global]
   history = []
   ```
   **Then** in normal mode both `x` and `!` run, in search mode Ctrl-P opens the action menu (and no longer moves up), and history has no key.
2. **Given** an override, **Then** it replaces the preset's keys for that command in that context only; commands that are not mentioned keep their preset keys.
3. **Given** an override binds a key that the preset uses for another command in the same context, **Then** the override wins, the other command loses that key, and kb warns once naming both commands, so the change is visible.
4. **Given** two overrides bind the same key in the same context, **Then** kb starts, ignores both of those overrides (keeping the preset keys), and warns with the file and lines.
5. **Given** an override uses a reserved key (FR-024) or an unknown command, context or key name, **Then** kb starts, ignores that override, and warns with the file, line and the accepted values.
6. **Given** `kb keys`, **Then** kb prints the effective bindings per context, marking which come from the preset and which from config. `--json` is supported. `kb keys check` validates the config without opening the TUI, exiting 1 if any configuration issue or warning is found (and 0 only when all overrides are clean). `kb keys defaults --keymap vim` prints a commented `[tui.keys.*]` block with every command and its default keys, ready to copy into config.
7. **Given** overrides are in effect, **Then** the key bar and help show the user's keys, not the preset's.

---

### Edge Cases

- **Typing a letter that is bound in normal mode while in search mode**: it is typed into the query; letters act only in normal mode.
- **Esc in sub-views** (action menu, code view, variable form, add form, conflict screen, help, history): closes that view and returns to the list in the mode it was opened from. These views keep their own keys unchanged.
- **Code view letters** (`g v i y u F`): unchanged; they apply only inside the code view, which already uses single-letter keys.
- **Run confirmation**: `!` opens the same confirmation as Ctrl-G (M7); typing `run` for destructive or unreviewed commands is unchanged.
- **Empty results in normal mode**: action keys do nothing and the key bar says why (`no entry selected`); `/` and `i` still work.
- **Multi-key sequence interruption**: If any key other than `g` is pressed while `g` is pending, the sequence cancels immediately and the new key executes in normal mode without waiting for the 1-second timeout.
- **Esc delay**: terminals send Esc as the start of Alt sequences. The TUI MUST tell a lone Esc from an Alt sequence within 50 ms, so switching modes feels instant and Alt bindings keep working.
- **Narrow terminals**: the mode indicator takes at most 8 columns and is never dropped from the header.
- **`NO_COLOR`**: the mode is shown as text (`NORMAL` / `SEARCH`), never by color alone.
- **Inline shell picker (M8)**: keeps the emacs-style keys in this version, since it runs inside the shell's own line editor. Its trigger key is already configurable (`widget.key`, M8).
- **Binding a plain printable key in search mode** (for example `o` in `[tui.keys.search]`): rejected with a warning, because it would stop that character from being typed into the query. Printable keys are allowed in every context that has no text input.
- **Unbinding every way to reach help or quit**: rejected; at least one help key and one quit key must remain in each list mode (FR-025).
- **Overrides for the emacs keymap that mention normal mode**: ignored with a warning, since emacs has no normal mode.
- **Custom actions (M7 FR-034)** keep their own `key` setting in the action menu; a clash between a custom action key and a configured action-menu binding is reported as in M7 FR-036.
- **Config edited while the TUI is open**: bindings are read at startup; Ctrl-L (refresh) reloads them, with the same warnings.
- **Keyboard layouts**: keys are matched on the character the terminal sends (`ñ`, `ç` are valid key names), not on physical key positions.

## Requirements *(mandatory)*

### Functional Requirements

#### Keymaps and Modes

- **FR-001**: The TUI MUST support `tui.keymap = "emacs"` (default) and `"vim"`, settable in config, by `KB_TUI_KEYMAP`, and by `kb --keymap <name>`.
- **FR-002**: With `emacs`, behavior MUST be identical to before this spec.
- **FR-003**: With `vim`, the main list MUST have two modes, search and normal, starting in search mode.
- **FR-004**: Mode switches:
  - search → normal: Esc;
  - normal → search: `/` (clears the query), `i` or `a` (keeps the query, cursor at the end);
  - picking, running or opening an entry does not change the mode;
  - returning from a sub-view restores the mode it was opened from.
- **FR-005**: In vim mode, Esc in normal mode MUST clear a non-empty query, and with an empty query MUST exit (code 130), as `q` does.
- **FR-006**: All bindings that exist in the `emacs` keymap MUST also work in both modes of the `vim` keymap, except where this spec redefines a key for normal mode (FR-008).

#### Normal-Mode Bindings

- **FR-007**: Normal mode MUST add these bindings:

  | Keys | Command |
  |---|---|
  | `j`, ↓ / `k`, ↑ | down / up |
  | `gg`, Home / `G`, End | first / last result |
  | Ctrl-D / Ctrl-U, PgDn / PgUp | half page down / up; page down / up |
  | `h`, ← / `l`, → | previous / next type filter |
  | `J` / `K` (and Shift-↓ / Shift-↑) | scroll preview down / up |
  | `p` | toggle full preview |
  | Enter | pick |
  | `y` / `Y` | copy / copy template (unresolved) |
  | `o` | open (primary open action, M7) |
  | `!` | run (with confirmation, M7) |
  | `m`, `.` | action menu |
  | `e` | edit in `$EDITOR` |
  | `n` | new entry (add form) |
  | `R` | rename |
  | `D` | delete (with confirmation) |
  | `s` | sync (M9) |
  | `H` | history of the selected entry (M9) |
  | `v` | mark / unmark for a new runbook (R1, when available) |
  | `B` | new runbook from marked entries (R1, when available) |
  | `/` / `i`, `a` | search mode (clear / keep query) |
  | `?` | help |
  | `q` | quit |
  | Ctrl-L | refresh |

- **FR-008**: In normal mode, Ctrl-U and Ctrl-D MUST mean half-page movement (vim convention); in search mode Ctrl-U keeps its line-editing meaning (clear line, M3 FR-011). Home and End MUST mean first and last result in normal mode, and line start and end in search mode.
- **FR-009**: Arrow keys MUST work in both modes: ↑ / ↓ move the selection; ← / → change the type filter in normal mode and move the query cursor in search mode.
- **FR-010**: Multi-key sequences (`gg`) MUST be supported through a pending-key state with a 1-second timeout, cancellable with Esc. If any key other than `g` is pressed while waiting for the second key, the pending sequence MUST be cancelled immediately and the new key executed as a standalone normal-mode command. No other sequences are defined in this version.

#### Display

- **FR-011**: In vim mode the header MUST show the current mode as `NORMAL` or `SEARCH`, in text, with a distinct style.
- **FR-012**: The query line MUST show a cursor only in search mode.
- **FR-013**: The key bar and help MUST be generated from the binding table for the current keymap and mode (as they are today), and MUST NOT offer keys that do nothing in the current state.

#### Implementation Constraints

- **FR-014**: Bindings MUST stay in the single binding table, with a mode bit for normal mode; the compile-time duplicate check MUST cover both keymaps and every mode.
- **FR-015**: The lone-Esc versus Alt-sequence decision MUST complete within 50 ms (Edge Cases).
- **FR-016**: The README's key reference MUST document the vim keymap; the existing test that checks every bound key is documented MUST cover it.

#### Spec Amendments

- **FR-017**: The key tables of these specs MUST note the vim equivalents when this spec is merged: 003 FR-010, 007 FR-027, 008 FR-037, 009 FR-029, and 011 FR-026 if R1 has landed. They MUST also note that the keys listed are defaults and can be changed (FR-018).

#### Configurable Bindings

- **FR-018**: The effective keymap MUST be built at startup from the preset (`tui.keymap`) plus overrides in config. Every command in the binding table MUST be configurable, in every context where it is bound.
- **FR-019**: Contexts MUST be named after the TUI's existing modes and views:
  - `global` (applies strictly to main list modes `search` and `normal` unless a mode-specific context overrides it; never cascades into sub-views),
  - `search`, `normal` (vim only),
  - `action-menu`, `code-view`, `history`, `conflict`,
  - `form` (variable form, M5), `add` (add form, M6),
  - `help`.
- **FR-020**: Overrides MUST be written as `[tui.keys.<context>]` tables mapping a command name to a list of keys. A list replaces that command's keys in that context; an empty list unbinds it there.
- **FR-021**: Command names MUST be stable, kebab-case, and documented (for example `pick`, `copy`, `copy-template`, `open`, `run`, `action-menu`, `edit`, `add-entry`, `rename-entry`, `delete-entry`, `down`, `up`, `page-down`, `half-page-down`, `first`, `last`, `type-next`, `type-prev`, `scroll-preview-down`, `toggle-preview`, `search-clear`, `search-keep`, `normal-mode`, `refresh`, `sync`, `history`, `help`, `quit`). Renaming a command later MUST keep the old name as an alias with a deprecation warning.
- **FR-022**: Key names MUST use one documented syntax:
  - modifiers `ctrl-`, `alt-`, `shift-`, combinable (`ctrl-alt-x`);
  - named keys `enter`, `esc`, `tab`, `backtab`, `space`, `backspace`, `delete`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `f1`–`f12`;
  - any single printable character, case-sensitive (`G` differs from `g`);
  - a sequence of up to two keys separated by a space (`"g g"`), in contexts without text input only.
- **FR-023**: Validation MUST follow M4 FR-016: kb MUST always start; every problem (unknown context, command or key name; duplicate keys between overrides; reserved keys; printable keys in text-input contexts; sequences in text-input contexts) produces one warning with the file and line, and the affected override is ignored.
- **FR-024**: Reserved keys MUST NOT be rebound or unbound:
  - Ctrl-C always quits (or cancels the current view);
  - the confirmation prompts of run, delete and unreviewed entries (M7 FR-013, FR-020; MCP FR-031) keep their fixed answers (`y`, typing `run`), so a binding can never weaken a safety check.
- **FR-025**: After applying overrides, each list mode MUST still have at least one key for `help` and one for `quit`; otherwise the offending overrides are ignored with a warning.
- **FR-026**: The compile-time duplicate check (FR-014) MUST still cover the presets. Duplicates introduced by user overrides MUST be caught by the startup validation (FR-023).
- **FR-027**: `kb keys [--context <name>] [--json]`, `kb keys check`, and `kb keys defaults [--keymap emacs|vim]` MUST behave as in User Story 5, scenario 6. `kb keys check` MUST exit 1 if any configuration issue, warning, duplicate override, reserved key violation, or syntax error is present, and exit 0 only when all overrides are completely clean. `kb doctor` (M4) MUST include the result of `kb keys check`.
- **FR-028**: The key bar, help overlay and every hint that names a key (for example "press `u` to re-pin") MUST use the effective binding, never a hard-coded key.

### Key Entities

- **Keymap**: The base binding preset, either `emacs` (default) or `vim`.
- **Mode**: The interactive operational state in the main list: `search` or `normal` (vim only), in addition to sub-views (`help`, `form`, `add`, `action-menu`, etc.).
- **PendingKey**: State tracking the first key in a multi-key sequence (e.g. `g` waiting for `g`) along with timestamp for the 1-second expiration timeout.
- **KeyOverride**: A configured mapping from a command name to one or more key combinations within a specific context, including source file and line for diagnostics.
- **EffectiveKeymap**: The runtime resolved key-to-command dispatch table combining the chosen preset with all validated user overrides, tracking provenance and any diagnostic warnings emitted.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With the emacs keymap, the full existing TUI test suite (unit, PTY and snapshots) passes unchanged.
- **SC-002**: With the vim keymap, every action reachable by a Ctrl or Alt key is also reachable by a single key (or `gg`) in normal mode, verified by a test over the binding table.
- **SC-003**: Switching from search to normal mode and running an action takes two key presses (Esc, then the letter).
- **SC-004**: Esc switches modes within 50 ms in Terminal.app, iTerm2, kitty, Alacritty and tmux, while Alt bindings still work in the same terminals.
- **SC-005**: Arrow-key navigation behaves identically in both keymaps for ↑, ↓, PgUp and PgDn (unless the user rebinds them).
- **SC-006**: Every command in the binding table can be rebound in every context where it exists, verified by a generated test that rebinds each one to an unused key and triggers it in a PTY.
- **SC-007**: A config file with one valid override and one of each kind of invalid override starts the TUI with the valid override applied and exactly one warning per invalid one.
- **SC-008**: No configuration can rebind Ctrl-C, change a confirmation answer, or leave a list mode without help and quit.

## Assumptions

- Starting in search mode keeps kb's main strength (type and pick) intact for vim users, at the cost of one Esc before the first single-letter action.
- Keeping all Ctrl and Alt bindings active in vim mode avoids a second set of documentation for people who switch keymaps.
- `!` for run follows vim's convention for running shell commands and is harder to press by accident than a plain letter.
- Plain printable characters can only be bound in contexts that do not accept free-form text input (e.g., normal mode, action menu, help), preventing keystrokes from being swallowed during search query or form entry.

## Out of Scope

- Vim keys in the inline shell picker (M8), the add form, the variable form and other text-entry views.
- Visual mode, counts (`5j`), registers, macros, and command-line mode (`:`).
- Sequences longer than two keys, and user-defined sequences in text-input contexts.
- Binding keys to arbitrary shell commands from the keymap (custom actions, M7 FR-034, cover that through the action menu).
- Configurable keys for the inline shell picker beyond its trigger key.
- Changing the default keymap from `emacs`.
