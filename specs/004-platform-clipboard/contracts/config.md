# Contract: configuration

Supersedes `specs/003-tui-core/contracts/config.md` (YAML, `theme`, `mouse`).

## Files

| OS | Read order (first existing `config.toml` wins; never merged) | Write path |
|---|---|---|
| Linux | `$XDG_CONFIG_HOME/kb/config.toml` (absolute only), else `~/.config/kb/config.toml` | the first one |
| macOS | `~/.config/kb/config.toml`, else `~/Library/Application Support/kb/config.toml` | `~/.config/kb/config.toml` |

If no `config.toml` exists, the same directories are searched for a legacy `config.yml`; it is read
for `root`, `theme` and `mouse` (as `tui.mouse`) only, with the deprecation warning, and is never
written.

No file is created until `kb config init` or `kb config set`.

## Settings

See [data-model.md](../data-model.md#setting-and-settings-kb_coreconfig) for keys, types and defaults.

```toml
root = "~/work/kb"
editor = "nvim"
theme = "light"

[tui]
enter = "copy"      # or "print"
mouse = false

[clipboard]
method = "auto"     # auto | native | osc52 | stdout
command = ""        # e.g. "xclip -selection clipboard"
osc52_max_bytes = 74000
trim_trailing_newline = true

[opener]
command = ""

[vars]
remember = true     # M5: remember the values used for variables (never secrets)
```

## Precedence (FR-015)

flag → environment variable → file → default, per key. Environment names: `KB_ROOT`,
`KB_EDITOR`, `KB_THEME`, `KB_TUI_ENTER`, `KB_TUI_MOUSE`, `KB_CLIPBOARD_METHOD`,
`KB_CLIPBOARD_COMMAND`, `KB_CLIPBOARD_OSC52_MAX_BYTES`, `KB_CLIPBOARD_TRIM_TRAILING_NEWLINE`,
`KB_OPENER_COMMAND`, `KB_VARS_REMEMBER` (added by M5). An empty variable counts as unset.

The editor additionally falls back to `$VISUAL`, `$EDITOR`, `vi` (FR-013) below the file value.

## Warnings (FR-016, SC-005)

Format, one line each, on stderr:

```
kb: /home/me/.config/kb/config.toml:7: clipboard.method: "xclipp" is not valid (expected auto, native, osc52 or stdout); using "auto"
kb: /home/me/.config/kb/config.toml:9: unknown key "clipbaord.method" (ignored)
kb: /home/me/.config/kb/config.toml:2: could not parse the file: <toml message>; using defaults
kb: /home/me/.config/kb/config.yml: deprecated; run `kb config init` and move your settings to config.toml
```

Exactly one per problem per run. Commands that print results (`--json`) still print the warnings
on stderr only. The TUI defers them (header + print after exit).
