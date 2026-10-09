# Contract: `kb doctor`

| id | Pass when | Warn when | Fail when | Hint |
|---|---|---|---|---|
| `kb` | always (shows version, OS, arch) | | | |
| `root` | root exists and is readable | | missing or unreadable | `run kb init` or set KB_ROOT / --root |
| `root-writable` | `.kb/` probe write works, or root dir is writable when `.kb/` is absent | read-only | | `entries can be read but not edited` |
| `index` | index current (M2 status) | stale or missing (will rebuild on first use) | unreadable / corrupt that a rebuild cannot fix | `run kb reindex` |
| `config` | file parsed with no warnings, or no file | any warning, ignored second macOS file, legacy `config.yml` | file exists but unreadable | `kb config init`, the warning text |
| `display` | detected (macOS, Wayland, X11) | none and not SSH | | `no display; clipboard will use OSC 52` |
| `session` | info: ssh yes/no, tmux yes/no, screen yes/no | | | |
| `clipboard` | chosen method is native and the tool is on `PATH` | chosen method is OSC 52 (Terminal.app, tmux passthrough off), or `stdout` | | per case below |
| `clipboard-alternatives` | info: lists the other usable methods | | | |
| `true-color` | `COLORTERM` truecolor/24bit | not advertised | | `set COLORTERM=truecolor if your terminal supports it` |
| `editor` | resolved command's first word found on `PATH` | not found | | `set editor in config or $EDITOR` |
| `opener` | opener found, or SSH-without-display ("will print URLs") | not found | | `install xdg-utils` |
| `copy-test` | only with `--copy-test`: pasted text matches | | mismatch or timeout | the attempts list |

Clipboard hints (SC-003 matrix):

| Situation | Status | Hint |
|---|---|---|
| native tool missing on Wayland | Warn | `install wl-clipboard` |
| native tool missing on X11 | Warn | `install xclip or xsel` |
| no display, not SSH | Warn | `no clipboard tool applies; using OSC 52 (works in iTerm2, kitty, Alacritty, WezTerm, recent GNOME Terminal)` |
| OSC 52, `TERM_PROGRAM=Apple_Terminal` | Warn | `Terminal.app ignores OSC 52; use another terminal` |
| OSC 52 in tmux, `allow-passthrough` off | Warn | `add "set -g allow-passthrough on" to ~/.tmux.conf` |
| method is `stdout` | Warn | `no clipboard method available; text is printed instead` |
| payload size | Info line in `clipboard` | `OSC 52 limit: 74000 bytes (clipboard.osc52_max_bytes); larger: kb show --raw <id> \| <tool>` |

JSON (`--json`):

```json
{"version": "0.1.0", "ok": true,
 "environment": {"os": "macos", "arch": "aarch64", "display": "quartz", "ssh": false,
                 "tmux": false, "screen": false, "true_color": true},
 "paths": {"config": "/…/config.toml", "data": "…", "cache": "…"},
 "checks": [{"id": "clipboard", "status": "pass", "summary": "pbcopy", "hint": null, "details": ["OSC 52 limit: …"]}]}
```

`ok` is false iff any check is `fail`. Exit `1` exactly then. `details` holds extra facts shown under the check (the OSC 52 limit and workaround, each config warning). With `--copy-test` a `copy-test` check is added; it needs a terminal on stdin (exit 2 without one), prints `Paste it here and press Enter:` on stderr, records no use, and never prints the test text to stdout.
