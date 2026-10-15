# Contract: Key Commands and Presets

**Feature Branch**: `013-vim-keymap` | **Date**: 2026-10-14  
**Spec**: [specs/013-vim-keymap/spec.md](../spec.md)

---

## 1. Stable Command Names and Preset Mappings

| Command Name | Description | `emacs` Default | `vim` (Search) | `vim` (Normal) |
|---|---|---|---|---|
| `pick` | Pick selected entry (copy or open) | `enter` | `enter` | `enter` |
| `copy` | Copy entry payload to clipboard | `ctrl-y` | `ctrl-y` | `y`, `ctrl-y` |
| `copy-template` | Copy payload with raw variables | `alt-c` | `alt-c` | `Y`, `alt-c` |
| `open` | Primary open (console/browser/code) | `ctrl-o` | `ctrl-o` | `o`, `ctrl-o` |
| `run` | Execute command entry safely | `ctrl-g` | `ctrl-g` | `!`, `ctrl-g` |
| `action-menu` | Open action menu modal | `alt-a`, `f2` | `alt-a`, `f2` | `m`, `.`, `alt-a`, `f2` |
| `edit` | Edit entry in `$EDITOR` | `ctrl-e` | `ctrl-e` | `e`, `ctrl-e` |
| `add-entry` | Open add entry form | `ctrl-n` | `ctrl-n` | `n`, `ctrl-n` |
| `rename-entry` | Rename selected entry | `ctrl-r` | `ctrl-r` | `R`, `ctrl-r` |
| `delete-entry` | Delete selected entry | `ctrl-x` | `ctrl-x` | `D`, `ctrl-x` |
| `down` | Move selection down | `down`, `ctrl-j`, `ctrl-n` | `down`, `ctrl-j`, `ctrl-n` | `j`, `down`, `ctrl-j`, `ctrl-n` |
| `up` | Move selection up | `up`, `ctrl-k`, `ctrl-p` | `up`, `ctrl-k`, `ctrl-p` | `k`, `up`, `ctrl-k`, `ctrl-p` |
| `page-down` | Move selection one page down | `pagedown` | `pagedown` | `pagedown` |
| `page-up` | Move selection one page up | `pageup` | `pageup` | `pageup` |
| `half-page-down` | Move selection half page down | — | — | `ctrl-d` |
| `half-page-up` | Move selection half page up | — | — | `ctrl-u` |
| `first` | Jump to first entry | — | — | `gg`, `home` |
| `last` | Jump to last entry | — | — | `G`, `end` |
| `type-next` | Advance type filter | `tab` | `tab` | `l`, `right`, `tab` |
| `type-prev` | Previous type filter | `backtab` | `backtab` | `h`, `left`, `backtab` |
| `scroll-preview-down` | Scroll entry preview down | `shift-down` | `shift-down` | `J`, `shift-down` |
| `scroll-preview-up` | Scroll entry preview up | `shift-up` | `shift-up` | `K`, `shift-up` |
| `scroll-preview-page-down`| Scroll entry preview page down| `shift-pagedown` | `shift-pagedown` | `shift-pagedown` |
| `scroll-preview-page-up` | Scroll entry preview page up | `shift-pageup` | `shift-pageup` | `shift-pageup` |
| `toggle-preview` | Toggle full preview overlay | `alt-p` | `alt-p` | `p`, `alt-p` |
| `left` | Move cursor left in search query | `left`, `ctrl-b` | `left`, `ctrl-b` | — |
| `right` | Move cursor right in search query | `right`, `ctrl-f` | `right`, `ctrl-f` | — |
| `line-start` | Move cursor to query start | `home`, `ctrl-a` | `home`, `ctrl-a` | — |
| `line-end` | Move cursor to query end | `end`, `ctrl-e` | `end`, `ctrl-e` | — |
| `delete-word` | Delete previous word in query | `alt-backspace`, `ctrl-w` | `alt-backspace`, `ctrl-w` | — |
| `clear-line` | Clear search query | `ctrl-u` | `ctrl-u` | — |
| `normal-mode` | Switch to normal mode | — | `esc` | — |
| `search-clear` | Switch to search mode, clear query | — | — | `/` |
| `search-keep` | Switch to search mode, keep query | — | — | `i`, `a` |
| `refresh` | Reload index and configuration | `ctrl-l` | `ctrl-l` | `ctrl-l` |
| `sync` | Trigger git sync | `alt-s` | `alt-s` | `s`, `alt-s` |
| `history` | View git history of entry | `alt-h` | `alt-h` | `H`, `alt-h` |
| `help` | Open help modal | `?` (empty line), `f1` | `?` (empty line), `f1` | `?`, `f1` |
| `quit` | Exit kb | `esc` (empty line), `ctrl-c` | `esc` (empty line), `ctrl-c` | `q`, `esc` (empty query), `ctrl-c` |

---

## 2. Sub-View Command Names

### 2.1 Action Menu (`action-menu`)
- `down`: `["down", "j"]`
- `up`: `["up", "k"]`
- `pick`: `["enter"]`
- `quit`: `["esc", "q"]`

### 2.2 Code View (`code-view`)
- `open-web`: `["g"]`
- `open-vscode`: `["v"]`
- `open-idea`: `["i"]`
- `copy-path-line`: `["y"]`
- `copy-permalink`: `["Y"]`
- `repin`: `["u"]`
- `fetch`: `["F"]`
- `edit`: `["ctrl-e"]`
- `quit`: `["esc", "q"]`

### 2.3 Help Modal (`help`)
- `close`: Any key (by default) or `["esc", "q"]`
