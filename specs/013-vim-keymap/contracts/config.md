# Contract: Configuration Schema

**Feature Branch**: `013-vim-keymap` | **Date**: 2026-10-14  
**Spec**: [specs/013-vim-keymap/spec.md](../spec.md)

---

## 1. Setting: `tui.keymap`

```toml
[tui]
keymap = "emacs" # "emacs" (default) | "vim"
```

- **Type**: Choice (`emacs`, `vim`)
- **Default**: `"emacs"`
- **Environment Variable**: `KB_TUI_KEYMAP`
- **CLI Flag**: `--keymap <name>`
- **Precedence**: Flag > Env > Config file > Default.

---

## 2. Table: `[tui.keys.<context>]`

### Syntax
```toml
[tui.keys.<context>]
<command-name> = ["<key1>", "<key2>", ...]
```

- Unbinding: `<command-name> = []` removes all keys for that command in that context.
- Replacing: Specifying an array replaces all preset keys for that command in that context. Unmentioned commands retain their preset keys.

### Permitted Contexts
1. `global`: Main list modes only (`search` and `normal`). Does not cascade into sub-views (Clarification 2).
2. `search`: Main list search mode (query input).
3. `normal`: Main list normal mode (vim navigation & actions; vim keymap only).
4. `action-menu`: Action menu modal.
5. `code-view`: Code entry viewer.
6. `history`: Entry git history viewer.
7. `conflict`: Git conflict resolution screen.
8. `form`: Variable prompt form (M5).
9. `add`: Add entry form (M6).
10. `help`: Help overlay modal.

---

## 3. Key Name Syntax

Key strings MUST match one of the following patterns:

1. **Modifiers**: `ctrl-`, `alt-`, `shift-` (case-insensitive, combinable, e.g. `ctrl-alt-x`).
2. **Named Keys**:
   - `enter`
   - `esc`
   - `tab`, `backtab`
   - `space`
   - `backspace`, `delete`
   - `up`, `down`, `left`, `right`
   - `home`, `end`
   - `pageup`, `pagedown`
   - `f1`, `f2`, ..., `f12`
3. **Single Printable Character**: Case-sensitive (`a`, `A`, `!`, `?`, `/`, `ñ`, `ç`).
4. **Two-Key Sequences**: Two key names separated by a single space (e.g., `"g g"`).
   - Only permitted in non-text-entry contexts (`normal`, `action-menu`, `code-view`, `help`).
   - Using a sequence in `search`, `form`, or `add` is rejected with a warning.

---

## 4. Conflict Resolution Rules

1. **Override vs. Preset in the Same Context**:
   - If user override assigns key `K` to command `A`, and the preset previously assigned `K` to command `B` in the same context:
   - Override wins: `A` receives `K`, `B` loses `K`.
   - A single informational warning is recorded noting that `B` lost key `K`.

2. **Override vs. Override in the Same Context**:
   - If two overrides in user configuration define the same key `K` for different commands in the same context:
   - Both overrides are ignored (keeping the preset keys for both commands).
   - A warning is emitted citing the configuration file and line numbers.

3. **Plain Printable Characters in Text-Input Contexts**:
   - Binding plain printable characters (e.g. `o`, `x`, `1`) in `search`, `form`, or `add` is invalid.
   - The override is ignored with a warning (printable characters are reserved for text typing).

---

## 5. Inviolable Safety Checks (Reserved Keys)

The following keys can NEVER be rebound or unbound:
1. `Ctrl-C`: Always quits the application or dismisses the current view/modal.
2. Confirmation responses: Prompt confirmations for `run`, `delete`, and unreviewed entry execution require their hard-coded answers (`y`, typing `run`).
3. Minimum navigation: In each list mode, at least one key for `help` and one key for `quit` must remain bound. If a configuration unbinds all help or quit keys, the offending unbinding is rejected with a warning.
