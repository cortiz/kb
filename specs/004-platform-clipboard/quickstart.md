# Quickstart: validating M4

Prerequisites: `cargo build --release`, the repo's fixture KB (`tests/fixtures/search`, 32 entries).
`$KB` below is the fixture copied to a temp dir; never run against `~/kb`.

```sh
export KB_ROOT=$(mktemp -d)/kb; cp -R tests/fixtures/search "$KB_ROOT"
```

## Automated

```sh
cargo test --workspace                      # unit + integration, fake tools on PATH
cargo test -p kb --test tui_pty             # Enter/Ctrl-Y end to end under a pty
cargo test --test platform_boundary         # FR-003
cargo test -p kb --release --test perf -- --ignored --test-threads=1   # SC-002
```

## By hand, one terminal (macOS here)

| Step | Command | Expect |
|---|---|---|
| 1 | `kb copy sumo/5xx-by-authorizer; pbpaste` | stderr `Copied … via pbcopy`; pbpaste prints the query |
| 2 | `kb copy 5xx auth` | resolves to the same entry (clear winner) |
| 3 | `kb copy latency </dev/null` | exit 1, lists up to 5 matches, nothing copied |
| 4 | `kb show 5xx auth` / `--raw` / `--json` / `--file` | preview / payload / JSON / path |
| 5 | `kb config init; kb config set clipboard.method osc52; kb config list` | file written, comments kept, source `file` |
| 6 | `KB_CLIPBOARD_METHOD=stdout kb config get clipboard.method` | `stdout` |
| 7 | add `clipboard.metod = "x"` and `theme = "blue"`; run `kb search x` | exactly two warnings, kb still works |
| 8 | `kb doctor`, `kb doctor --json`, `kb doctor --copy-test` | checklist, JSON, paste prompt |
| 9 | `kb`, type `5xx auth`, Enter | exits, `Copied … via pbcopy` on stderr, `pbpaste` matches |
| 10 | `kb --print id`, then `tui.enter = "print"` and plain `kb` | prints to stdout as in M3 |
| 11 | `kb`, Ctrl-Y | stays open, key bar says `Copied via pbcopy` for 2 s |

## Manual matrix (SC-001, cannot be automated here)

Record the result of pasting in each: macOS Terminal.app, iTerm2; Linux GNOME Wayland (`wl-copy`),
Linux X11 (`xclip`, then `xsel`); SSH Mac→Linux (OSC 52, iTerm2 and kitty); SSH inside tmux with
`allow-passthrough on` and off; GNU screen. Also check that `xclip`'s content survives kb exiting.
This is the M4 equivalent of M3's T070.
