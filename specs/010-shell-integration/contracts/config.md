# Configuration Contracts: Shell Integration

**Feature**: Shell Integration  
**Feature Branch**: `010-shell-integration`  
**Milestone**: M8  
**Status**: Completed  

---

## Settings Schema Additions

The following settings are added to `SETTINGS` in `crates/kb-core/src/config.rs`:

| Key | Type | Default | Environment Variable | Description |
|-----|------|---------|----------------------|-------------|
| `widget.key` | Text | `\ek` (Alt-K) | `KB_WIDGET_KEY` | Shell key combination bound to open the picker |
| `widget.save_key` | Text | `\eK` (Alt-Shift-K) | `KB_WIDGET_SAVE_KEY` | Shell key combination bound to save the previous command |
| `widget.default_type` | Text | `cmd` | `KB_WIDGET_DEFAULT_TYPE` | Default entry type filter in widget mode (`cmd` or `all`) |
| `widget.buffer` | Text | `query` | `KB_WIDGET_BUFFER` | Buffer strategy when prompt has text: `query` or `insert` |
| `widget.height` | Text | `40%` | `KB_WIDGET_HEIGHT` | Inline picker height (`<N>%` or `<N>` lines) |
| `widget.preview` | Bool | `true` | `KB_WIDGET_PREVIEW` | Show right preview pane in picker on wide terminals ($\ge 120$ cols) |

---

## Configuration File Example (`~/.config/kb/config.toml`)

```toml
[widget]
key = "\ek"
save_key = "\eK"
default_type = "cmd"
buffer = "query"
height = "40%"
preview = true
```
