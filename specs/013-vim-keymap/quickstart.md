# Quickstart & Validation Guide: Vim Keymap and Configurable Key Bindings

**Feature Branch**: `013-vim-keymap` | **Date**: 2026-10-14  
**Spec**: [specs/013-vim-keymap/spec.md](spec.md)

---

## 1. Prerequisites

- Build `kb` and run automated checks:
  ```bash
  cargo test -p kb-core -p kb
  cargo clippy --all-targets --all-features -- -D warnings
  ```
- Use a throwaway environment or test directory for configuration testing.

---

## 2. Validation Scenarios

### Scenario 1: Default Emacs Keymap Unchanged (SC-001)

1. Run `kb` without any keymap configuration:
   ```bash
   cargo run --bin kb
   ```
2. **Verify**:
   - The TUI starts in search mode with cursor active in the query bar.
   - No mode indicator (`NORMAL` / `SEARCH`) is shown in the header.
   - Keystrokes behave identically to previous versions (e.g., `Ctrl-Y` copies, `Ctrl-O` opens, `Ctrl-E` edits, arrow keys navigate).
   - Pressing `j` or `k` types the characters into the search query.

---

### Scenario 2: Vim Keymap Mode Switching and Single-Letter Actions (SC-002, SC-003)

1. Launch `kb` with the vim preset:
   ```bash
   cargo run --bin kb -- --keymap vim
   ```
2. **Verify initial state**:
   - The header displays `SEARCH` in bold text.
   - Query line displays the cursor.
3. Type a search query, e.g. `auth`:
   - Characters are inserted into the search query.
4. Press `Esc`:
   - Header switches to `NORMAL` in distinct styling within $\le 50$ ms.
   - Query cursor disappears.
   - Query text `auth` and search results are preserved.
5. Press `j` and `k`:
   - Selection moves down and up through the results.
6. Press `o`:
   - The selected entry triggers the primary open action (browser, deep link, or code view).
7. Press `y`:
   - The selected entry payload is copied to clipboard; key bar briefly confirms copy.
8. Press `/`:
   - Switches back to `SEARCH` mode; query is cleared and cursor is at position 0.
9. Type `sql`, press `Esc`, then press `i`:
   - Switches to `SEARCH` mode; query remains `sql`, cursor placed at the end of `sql`.
10. Press `Esc` (while in normal mode with query `sql`):
    - Query is cleared to empty; mode remains `NORMAL`.
11. Press `q` or `Esc` (while in normal mode with empty query):
    - TUI exits with status code 130.

---

### Scenario 3: Navigation and Multi-Key Sequences (SC-004, SC-005)

1. Launch `kb --keymap vim`:
2. Press `Esc` to enter normal mode.
3. Press `j` repeatedly until selection is near middle.
4. Press `g`:
   - Key bar immediately displays `g…`.
5. Press `g` within 1 second:
   - Selection instantly jumps to the very first result (`First`).
6. Press `G`:
   - Selection instantly jumps to the last result (`Last`).
7. Press `Ctrl-D` / `Ctrl-U`:
   - Selection scrolls half a page down / up.
8. Interruption test:
   - Press `g`, then immediately press `j`.
   - The pending sequence cancels immediately and selection moves down one row (no timeout delay or dropped keystroke).

---

### Scenario 4: User Configuration Overrides (SC-006, SC-007)

1. Create a test configuration file `test-config.toml`:
   ```toml
   [tui]
   keymap = "vim"

   [tui.keys.normal]
   run = ["x", "!"]

   [tui.keys.search]
   action-menu = ["ctrl-p", "alt-a"]

   [tui.keys.global]
   history = []
   ```
2. Run `kb keys check`:
   ```bash
   HOME=$PWD/home cargo run --bin kb -- keys check   # with the file at home/.config/kb/config.toml
   ```
   **Verify**: Exits with code 1 and reports one notice: `ctrl-p` moved from `up` to `action-menu` (a key taken from another command is always reported, FR-027). Remove `ctrl-p` from the list to get exit code 0 and 0 warnings.
3. Run `kb keys --json`:
   ```bash
   HOME=$PWD/home cargo run --bin kb -- keys --json
   ```
   **Verify**: `run` in `normal` context is marked as `origin: "config"`, and `history` in `global` is empty (`[]`).
4. Start TUI with `test-config.toml`:
   - In normal mode, press `x`: runs selected command entry.
   - In search mode, press `Ctrl-P`: opens action menu modal.

---

### Scenario 5: Error Detection and Reserved Keys Validation (SC-008)

1. Create an invalid configuration `bad-keys.toml`:
   ```toml
   [tui.keys.global]
   quit = [] # Unbinding quit without replacement

   [tui.keys.normal]
   run = ["ctrl-c"] # Attempting to bind reserved key

   [tui.keys.search]
   down = ["o"] # Printable character in text-input context
   ```
2. Run `kb keys check`:
   ```bash
   HOME=$PWD/home cargo run --bin kb -- keys check   # bad-keys.toml saved as home/.config/kb/config.toml
   ```
   **Verify**: Exits with code 1, reporting all three warnings with line numbers.
3. Start TUI with `bad-keys.toml`:
   ```bash
   HOME=$PWD/home cargo run --bin kb
   ```
   **Verify**:
   - `kb` starts successfully despite invalid overrides.
   - Invalid overrides are ignored; preset defaults remain active.
   - Pressing `Ctrl-C` exits cleanly.
