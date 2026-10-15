# Phase 0 Research: Vim Keymap and Configurable Key Bindings

**Feature Branch**: `013-vim-keymap` | **Date**: 2026-10-14  
**Spec**: [specs/013-vim-keymap/spec.md](spec.md)

---

## 1. Architectural Distribution: `kb-core` vs `kb`

### Decision
Place the core keymap domain models, command definitions, key syntax parser, configuration override loader, conflict detector, and `EffectiveKeymap` in `crates/kb-core/src/keys/`. Keep terminal input integration (translating `crossterm::event::KeyEvent` into `kb_core::keys::Key`) and UI rendering inside `crates/kb/src/tui/`.

### Rationale
- **Constitution Principle IV (Terminal Only, One Core)** mandates: *"All domain logic (parsing, validation, indexing, search, variable resolution, actions, code-link resolution) MUST live in the kb-core crate."*
- `kb keys`, `kb keys check`, `kb keys defaults`, and `kb doctor` must inspect and validate key bindings from the CLI without depending on `ratatui` or terminal raw mode.
- `Key`, `Code`, and `Command` are independent of terminal rendering. `Code` in `crates/kb/src/tui/keys.rs` is already a pure data enum (`Char(char)`, `Enter`, `Esc`, `Up`, `Down`, etc.).
- Moving the core definitions into `kb-core` enables single-source-of-truth validation, linting in `kb doctor`, and test generation without code duplication.

### Alternatives Considered
- *Keep all key definitions in `crates/kb` and only store raw TOML strings in `kb-core`:* Rejected because `kb-core` configuration loading wouldn't be able to validate command names, context names, key syntax, or conflicts, violating Principle IV and forcing CLI commands in `kb` to duplicate validation logic.

---

## 2. Key Syntax, Parsing, and Normalization

### Decision
Implement a dedicated key string parser in `kb_core::keys::parse` supporting:
1. **Modifiers**: `ctrl-`, `alt-`, `shift-` (case-insensitive, combinable, e.g. `ctrl-alt-x`).
2. **Named keys**: `enter`, `esc`, `tab`, `backtab`, `space`, `backspace`, `delete`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `f1`..`f12` (case-insensitive).
3. **Single printable characters**: Case-sensitive (`g` vs `G`, `ñ`, `ç`).
4. **Sequences**: Up to two keys separated by whitespace (e.g., `"g g"`), restricted to non-text-entry contexts.

### Normalization Rules
- Modifiers and named keys are parsed case-insensitively.
- `shift-` combined with a lowercase letter normalizes to uppercase character (`shift-g` $\to$ `G`) without modifier bit, matching terminal emulator event encoding.
- `ctrl-` combined with a letter normalizes the character to lowercase with the `CTRL` modifier bit.
- Unbound command representation: empty list `[]` in TOML unbinds the command in that context.

### Alternatives Considered
- *Ad-hoc string matching in TOML parsing:* Rejected because error messages must report exact character spans, file, line number, and accepted token formats.

---

## 3. Disambiguating Lone-Esc vs Alt Sequences within 50 ms

### Decision
Rely on `crossterm`'s internal escape sequence parser in combination with the event loop's 50 ms polling window (`event::poll(Duration::from_millis(50))`).

### Rationale
- Terminals transmit Alt-chords as an ESC byte (`0x1B`) followed by the character code (e.g. `Alt-a` $\to$ `\x1b a`).
- `crossterm`'s event reader inspects incoming bytes on stdin. When `0x1B` arrives, it buffers and checks whether follow-up bytes complete an escape sequence within its internal timeout window.
- If a subsequent character arrives in the sequence, `crossterm` returns `KeyEvent { code: KeyCode::Char('a'), modifiers: KeyModifiers::ALT }`.
- If no bytes arrive before timeout, `crossterm` yields a standalone `KeyEvent { code: KeyCode::Esc, modifiers: KeyModifiers::empty() }`.
- In `crates/kb/src/tui/mod.rs`, `event::poll` already runs with a 50 ms timeout budget. By capturing `KeyCode::Esc` directly, mode switching from search to normal occurs within $\le 50$ ms across Terminal.app, iTerm2, kitty, Alacritty, and tmux (SC-004).

### Alternatives Considered
- *Custom raw stdin escape byte timer in kb:* Rejected because crossterm already maintains platform-tested escape sequence parsing across Unix and macOS.

---

## 4. Multi-Key Sequence State Machine (`gg`)

### Decision
Maintain an explicit `Option<PendingKey>` state within the TUI state struct:
```rust
pub struct PendingKey {
    pub key: Key,
    pub pressed_at: Instant,
}
```
- Timeout: 1 second (`Duration::from_secs(1)`).
- Interruption Behavior (Clarification 1): If any key other than `g` arrives while `g` is pending, the sequence is cancelled immediately, and the incoming key is executed as a regular normal-mode command without waiting for the timeout.
- Cancellation: Pressing `Esc` clears `pending_key` and remains in normal mode.
- Visual Cue (FR-013): When `pending_key` is active, the key bar displays `g…`.

### Alternatives Considered
- *Drop follow-up keystrokes during pending state:* Rejected; fast typists inadvertently hitting `g` before an intended command (e.g. `j` or `o`) would lose their keystroke. Immediate cancellation and dispatch matches standard Vim behavior.

---

## 5. Scope of `global` Overrides vs Sub-views

### Decision
The `[tui.keys.global]` table applies strictly to the two main list modes (`search` and `normal`). Sub-views (`action-menu`, `code-view`, `history`, `conflict`, `form`, `add`, `help`) do not inherit `global` bindings (Clarification 2).

### Rationale
- Sub-views have distinct interaction paradigms (e.g., forms accept arbitrary text, the code view has single-key inspectors `g`, `v`, `i`, `u`, `F`).
- Bleeding `global` bindings into sub-views would cause unintended key conflicts or break input handling in dialogs. Sub-views are customized strictly via their own context table (e.g. `[tui.keys.action-menu]`).

---

## 6. Strict Validation and Error Reporting (`kb keys check`)

### Decision
`kb keys check` exits with code `1` if any warning or error is detected (Clarification 3):
- Unknown context name.
- Unknown command name.
- Invalid key name or syntax.
- Duplicate key overrides defined in the same context.
- Attempting to rebind or unbind reserved keys (`Ctrl-C`, confirmation answers).
- Printable keys or key sequences configured in text-input contexts.
- Configuration leaving a list mode without at least one key for `help` and `quit`.

At runtime (`kb` launch), invalid overrides produce a warning (file, line number, accepted values), the specific invalid override is ignored, and `kb` successfully starts with the fallback preset bindings intact.

---

## 7. Compile-Time and Static Preset Integrity

### Decision
Preserve compile-time duplicate checking for both `emacs` and `vim` presets via a const table or static assertion test, verifying:
- No duplicate key assignments exist within any mode/screen in either preset.
- In normal mode of the `vim` keymap, every action reachable via Ctrl or Alt in `emacs` is reachable via a single key or `gg` (SC-002).
- The README documentation test covers all bound keys in both presets (FR-016).
