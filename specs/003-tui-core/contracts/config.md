> **Superseded by [`specs/004-platform-clipboard/contracts/config.md`](../../004-platform-clipboard/contracts/config.md)** (M4): the config is now TOML (`config.toml`) and `mouse` is `tui.mouse`. This file describes the M3 YAML format.

# Contract: configuration (M3)

The config file is the one M1 already reads for `root` (`~/.config/kb/config.yml` on Linux or under
`$XDG_CONFIG_HOME`; `~/Library/Application Support/kb/config.yml` on macOS).

```yaml
root: ~/kb          # M1
theme: auto         # auto | dark | light   (default auto)
mouse: false        # true enables wheel scrolling and click-to-select (default false)
```

| Key | Values | Default | Effect |
|-----|--------|---------|--------|
| `theme` | `auto`, `dark`, `light` | `auto` | `auto` uses `COLORFGBG` when the terminal sets it (background 0–6 or 8 → dark, 7 or 15 → light), otherwise dark |
| `mouse` | `true`, `false` | `false` | `true` turns on terminal mouse reporting; off keeps normal text selection |

Rules:

- `NO_COLOR` (any non-empty value) overrides `theme`: no colors are used.
- `COLORTERM=truecolor|24bit` allows RGB equivalents of the palette; otherwise 256-color indexes.
- Unknown keys are ignored (later milestones add their own).
- An invalid value is an error: `kb: <file>: theme must be auto, dark or light` and exit 2, before
  any terminal is touched.
- A missing file or missing keys mean the defaults.
