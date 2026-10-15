# Contract: CLI Interface

**Feature Branch**: `013-vim-keymap` | **Date**: 2026-10-14  
**Spec**: [specs/013-vim-keymap/spec.md](../spec.md)

---

## 1. Global `--keymap` Flag

```text
kb [--keymap <emacs|vim>]
```

- **Description**: Overrides the active TUI keymap preset for a single invocation.
- **Values**: `emacs` | `vim`.
- **Precedence**: `--keymap` > `KB_TUI_KEYMAP` > `tui.keymap` in config file > default (`emacs`).
- **Error handling**: Invalid value outputs error to stderr and exits with code 2.

---

## 2. `kb keys` Command

```text
kb keys [--context <name>] [--keymap <emacs|vim>] [--json]
```

### Description
Inspects and displays the effective key bindings per context, marking whether each binding originates from the preset or from user configuration overrides.

### Arguments & Flags
- `--context <name>`: Filter display to a single context (e.g. `normal`, `search`, `action-menu`, `help`).
- `--keymap <emacs|vim>`: Preview effective bindings using a specific preset instead of the configured default.
- `--json`: Output structured JSON representation instead of human-readable text.

### Text Output Format
```text
Context: normal (keymap: vim)
  j, down              down                    [preset]
  k, up                up                      [preset]
  gg, home             first                   [preset]
  G, end               last                    [preset]
  x, !                 run                     [config: ~/.config/kb/config.toml:42]
  ctrl-p               action-menu             [config: ~/.config/kb/config.toml:45]
  o                    open                    [preset]
  ...
```

### JSON Output Schema
```json
{
  "keymap": "vim",
  "contexts": {
    "normal": [
      {
        "command": "run",
        "keys": ["x", "!"],
        "origin": "config",
        "source": {
          "file": "/Users/user/.config/kb/config.toml",
          "line": 42
        }
      },
      {
        "command": "open",
        "keys": ["o", "ctrl-o"],
        "origin": "preset",
        "source": null
      }
    ]
  },
  "warnings": []
}
```

---

## 3. `kb keys check` Command

```text
kb keys check [--json]
```

### Description
Validates the key binding configuration without starting the TUI. Used in automated tests, CI, and `kb doctor`.

### Exit Codes (Clarification 3)
- `0`: All key overrides, syntax, commands, contexts, and reserved key rules are completely valid and clean (0 warnings).
- `1`: One or more configuration issues, syntax errors, unknown commands/contexts, duplicate overrides, reserved key violations, or missing help/quit keys were detected.
- `2`: Command-line usage error.

### Output
- On success:
  ```text
  Key configuration valid (preset: vim, 4 overrides active, 0 warnings).
  ```
- On failure:
  ```text
  config.toml:42: [tui.keys.normal] unknown command 'runn'
  config.toml:48: [tui.keys.global] cannot unbind reserved key 'ctrl-c'
  Validation failed: 2 problems found.
  ```
- With `--json`:
  ```json
  {
    "ok": false,
    "problems_count": 2,
    "warnings": [
      {
        "file": "/Users/user/.config/kb/config.toml",
        "line": 42,
        "context": "normal",
        "message": "unknown command 'runn'"
      },
      {
        "file": "/Users/user/.config/kb/config.toml",
        "line": 48,
        "context": "global",
        "message": "cannot unbind reserved key 'ctrl-c'"
      }
    ]
  }
  ```

---

## 4. `kb keys defaults` Command

```text
kb keys defaults [--keymap <emacs|vim>]
```

### Description
Prints a ready-to-copy, commented TOML block containing all contexts, all commands, and their default key bindings for the specified keymap.

### Output
```toml
# Default key bindings for kb (keymap = "vim")
# Copy sections below to ~/.config/kb/config.toml to customize.

# [tui.keys.global]
# help = ["?"]
# quit = ["q", "esc"]
# refresh = ["ctrl-l"]

# [tui.keys.normal]
# down = ["j", "down"]
# up = ["k", "up"]
# first = ["gg", "home"]
# last = ["G", "end"]
# half-page-down = ["ctrl-d"]
# half-page-up = ["ctrl-u"]
# page-down = ["pagedown"]
# page-up = ["pageup"]
# type-next = ["l", "right"]
# type-prev = ["h", "left"]
# scroll-preview-down = ["J", "shift-down"]
# scroll-preview-up = ["K", "shift-up"]
# toggle-preview = ["p"]
# pick = ["enter"]
# copy = ["y", "ctrl-y"]
# copy-template = ["Y", "alt-c"]
# open = ["o", "ctrl-o"]
# run = ["!", "ctrl-g"]
# action-menu = ["m", ".", "alt-a", "f2"]
# edit = ["e", "ctrl-e"]
# add-entry = ["n", "ctrl-n"]
# rename-entry = ["R", "ctrl-r"]
# delete-entry = ["D", "ctrl-x"]
# sync = ["s", "alt-s"]
# history = ["H", "alt-h"]
# search-clear = ["/"]
# search-keep = ["i", "a"]
```

---

## 5. `kb doctor` Integration (FR-027)

`kb doctor` executes the verification performed by `kb keys check` as part of its diagnostic suite:
- If key configuration is clean: reports check `keys: ok`.
- If issues exist: reports check `keys: failed` with details, marking the doctor run as having warnings/errors.
