# Technical Research: Shell Integration

**Feature**: Shell Integration  
**Feature Branch**: `010-shell-integration`  
**Milestone**: M8  
**Status**: Completed  

---

## 1. Inline Picker Terminal Mechanics (Normal Screen Rendering)

### Decision
Render the compact picker in the **normal screen buffer** directly below the shell prompt line using raw terminal I/O on `/dev/tty`, without switching to the alternate screen (`EnterAlternateScreen`), unless the terminal height is below 8 lines.

### Rationale
- **Prompt Continuity**: When a user invokes a shell widget (e.g., Alt-K), they expect an inline experience similar to `fzf`'s `--height` mode or Starship/zsh-autosuggestions menus—appearing directly below their prompt and scrolling the buffer up if the prompt is at the bottom of the screen.
- **Clean Teardown**: Upon exit (pick or cancel), all lines occupied by the picker are cleared (`Clear(ClearType::CurrentLine)`), any scrolled lines remain intact in terminal history, and the cursor is returned to the original prompt line and column position.
- **Fallback**: When terminal rows $< 8$, an inline layout cannot fit header, search input, chips, results, and status line. It falls back to full-screen alternate buffer mode (`EnterAlternateScreen`).

### Architecture & Terminal Operations
1. **Device Handling**: Open `/dev/tty` for both reading raw crossterm events and writing ANSI escape sequences. When `kb pick` runs in a shell backtick or pipe (e.g. `cmd=$(kb pick)`), standard output (stdout / stdout fd 1) is a pipe, but `/dev/tty` remains connected to the interactive terminal.
2. **Scroll & Room Calculation**:
   - Query cursor position via `cursor::position()`.
   - Compute desired picker height: $\min(\max(10, \text{rows} \times 40 / 100), \text{rows} - 1)$.
   - Calculate available rows below cursor: $\text{available} = \text{terminal\_height} - 1 - \text{cursor\_row}$.
   - If $\text{available} < \text{picker\_height}$: emit newline characters $(\text{picker\_height} - \text{available})$ times to scroll the terminal buffer up, then move cursor back up accordingly.
3. **Teardown**:
   - Move cursor to the top line of the picker area.
   - For each line of the picker height: move down, clear line (`Clear(ClearType::CurrentLine)`).
   - Restore cursor to the saved prompt row and column position.
   - Restore terminal raw mode and cursor visibility.

### Alternatives Considered
- *Always use alternate screen (`EnterAlternateScreen`)*: Rejected because it causes a full-screen flash, completely hides the user's prompt and current terminal context, and breaks the inline mental model.
- *ANSI save/restore cursor (`CSI s` / `CSI u` or `ESC 7` / `ESC 8`)*: Unreliable across terminal emulators when vertical scrolling occurs between save and restore. Explicit coordinate tracking relative to terminal viewport is deterministic.

---

## 2. Shell Widget IPC Protocol

### Decision
`kb pick --widget` communicates with the shell wrapper function using a strict, structured JSON payload emitted on stdout (FD 1), while all UI rendering and key reading occurs exclusively on `/dev/tty`.

### Payload Specification
```json
{
  "status": "picked" | "cancelled" | "refused",
  "command": "string containing resolved command",
  "cursor": 42,
  "message": "optional error or warning string"
}
```

### Exit Codes
- `0`: Picked successfully. Payload contains resolved `command` and target `cursor` position.
- `130`: Cancelled (Esc, Ctrl-C). Prompt line remains unchanged.
- `1`: Refused (e.g., secret without environment variable source, or empty KB). Widget displays `message` below prompt.
- `2`: Usage or fatal I/O error.

### Rationale
- Parsing JSON with standard shell tools (or built-in shell pattern matching / `jq` / Python / simple string parsing) prevents arbitrary code execution risks.
- Passing multi-line commands or commands with special characters (quotes, backslashes, tabs) through raw environment variables or unquoted arguments is prone to shell injection and whitespace corruption. JSON strings preserve multi-line content and escape sequences byte-for-byte.

---

## 3. Shell Dialect Widget Implementations

### Zsh (ZLE Widget)
- **Mechanism**: Define `_kb_widget()` function and bind with `zle -N _kb_widget`.
- **State Access**: `$BUFFER` (full command line), `$CURSOR` (cursor index, 0-based in ZLE byte offset).
- **Update**:
  ```zsh
  _kb_widget() {
    local out ret
    out=$(kb pick --widget --shell zsh --buffer "$BUFFER" --cursor "$CURSOR" 2>/dev/null)
    ret=$?
    if [[ $ret -eq 0 ]]; then
      BUFFER=$(echo "$out" | jq -r .command)
      CURSOR=$(echo "$out" | jq -r .cursor)
      zle reset-prompt
    elif [[ $ret -eq 1 ]]; then
      local msg
      msg=$(echo "$out" | jq -r .message)
      zle -M "$msg"
    fi
  }
  zle -N _kb_widget
  bindkey '\ek' _kb_widget          # emacs keymap
  bindkey -M vicmd '\ek' _kb_widget  # vi command keymap
  bindkey -M viins '\ek' _kb_widget  # vi insert keymap
  ```

### Bash 4.0+ (`bind -x` with Readline)
- **Mechanism**: `bind -x '"\ek": _kb_widget'`.
- **State Access**: `$READLINE_LINE` and `$READLINE_POINT`.
- **Update**:
  ```bash
  _kb_widget() {
    local out ret
    out=$(kb pick --widget --shell bash --buffer "$READLINE_LINE" --cursor "$READLINE_POINT" 2>/dev/null)
    ret=$?
    if [[ $ret -eq 0 ]]; then
      READLINE_LINE=$(echo "$out" | jq -r .command)
      READLINE_POINT=$(echo "$out" | jq -r .cursor)
    fi
  }
  bind -x '"\ek": _kb_widget'
  ```

### Bash 3.2 (macOS Legacy Fallback)
- **Limitation**: In bash 3.2, `bind -x` does not support modifying `READLINE_LINE` or `READLINE_POINT`.
- **Fallback**: Detect `${BASH_VERSINFO[0]} -lt 4`.
  Instead of direct line mutation, push resolved command to shell history with `history -s "$cmd"`, print a one-line hint `kb: command added to history (press Up arrow to recall)`, and advise upgrading to modern bash for direct insertion.

### Fish (3.0+)
- **Mechanism**: `commandline` builtin with `bind \ek _kb_widget`.
- **State Access**: `commandline` (gets buffer), `commandline -C` (gets cursor position).
- **Update**:
  ```fish
  function _kb_widget
    set -l buf (commandline)
    set -l cur (commandline -C)
    set -l out (kb pick --widget --shell fish --buffer "$buf" --cursor "$cur" 2>/dev/null)
    if test $status -eq 0
      set -l cmd (echo "$out" | jq -r .command)
      set -l new_cur (echo "$out" | jq -r .cursor)
      commandline -r "$cmd"
      commandline -C "$new_cur"
    end
    commandline -f repaint
  end
  bind \ek _kb_widget
  bind -M insert \ek _kb_widget
  ```

---

## 4. Secret Variable Environment Reference Syntax

### Decision
When `SecretGate` or `VarResolver` resolves a variable of kind `secret` for a shell target (widget mode):
1. If the secret specifies an `env` attribute (e.g. `env: SUMO_TOKEN`):
   - Replace the variable with a reference to the environment variable:
     - **Bash & Zsh**: `"$SUMO_TOKEN"`
     - **Fish**: `"$SUMO_TOKEN"`
   - The inline picker form displays the field as `[from $SUMO_TOKEN]` without prompting for plaintext value input.
   - If the environment variable is not currently set in the executing environment, warn `variable $SUMO_TOKEN is not set in environment` in the inline form, but still insert the reference.
2. If the secret has **no** `env` attribute:
   - Refuse insertion into the command line buffer.
   - Return status `"refused"` with message: `Secret {{token}} has no environment variable source; cannot place secret on command line. Use 'kb run <id>' instead.`

### Rationale
- Constitution Principle VI dictates secrets must never be placed in plain text where they can be logged or persisted.
- Shell history files (`~/.zsh_history`, `~/.bash_history`, `~/.local/share/fish/fish_history`) store every line executed by the user. Inserting raw secret tokens exposes them to disk and shell history leaks.
- Referencing `"$ENV_VAR"` ensures the secret is resolved dynamically by the shell when executed, leaving only the reference in shell history.

---

## 5. Dynamic Tab Completion Architecture (`kb __complete`)

### Decision
Provide a fast hidden command `kb __complete <kind> [args...]` designed to respond in $\le 50$ ms using direct read-only SQLite queries against `.kb/index.db`.

### Supported Kinds
1. `entries [prefix]`: Returns `id\ttitle` for entries starting with prefix.
2. `tags [prefix]`: Returns distinct tags matching prefix from the SQLite tags table.
3. `types`: Returns fixed list: `sql`, `sumo`, `cw`, `cmd`, `prompt`, `link`, `code`.
4. `vars <id> [prefix]`: Parses frontmatter for the given entry ID and returns declared variable names formatted as `name=`.
5. `var-values <id> <name> [prefix]`: Returns enum options for an enum variable in the given entry.

### Index Guarantees
- `kb __complete` opens `.kb/index.db` in `SQLITE_OPEN_READONLY` mode.
- It never initiates index rebuilds or file system scans. If the index does not exist or is stale, it serves available data immediately.

---

## 6. Startup File Management (`kb shell install`/`uninstall`)

### Target Files by Platform
- **Zsh**: `~/.zshrc`
- **Bash**:
  - macOS: `~/.bash_profile` if it exists, else `~/.bashrc`.
  - Linux: `~/.bashrc`.
- **Fish**: `~/.config/fish/config.fish`

### Block Markers
```sh
# >>> kb >>>
# Auto-generated by kb shell install; edits outside this block will be overwritten.
eval "$(kb shell init zsh)"
# <<< kb <<<
```

### Safety & Idempotency Rules
1. Check if the block markers already exist in the target file.
2. If block exists: replace only the text between `# >>> kb >>>` and `# <<< kb <<<`.
3. If block does not exist: append to the end of the file.
4. `kb shell uninstall`: strip the block and markers cleanly, preserving all surrounding content and whitespace.
5. Create a backup file (e.g. `~/.zshrc.kb-bak`) before any mutation.

---

## 7. tmux Popup Integration (`--tmux`)

### Detection & Command
- Check `$TMUX` environment variable.
- Verify tmux version via `tmux -V`: requires tmux $\ge 3.2$ (`display-popup` support).
- When `--tmux` is passed inside a valid tmux session, re-executes `kb pick` inside a popup:
  ```sh
  tmux display-popup -E -w 85% -h 50% -T " kb pick " kb pick --no-tmux "$@"
  ```
- If outside tmux or tmux $< 3.2$, falls back to inline terminal picker gracefully without error.
