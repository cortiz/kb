# Quickstart: TUI Core — Search and Preview

Runnable checks that prove M3 works. Contracts: [cli.md](contracts/cli.md), [keys.md](contracts/keys.md),
[screen.md](contracts/screen.md), [core-api.md](contracts/core-api.md).

## Setup

```sh
cargo build --release && export PATH="$PWD/target/release:$PATH"
export KB_ROOT="$(mktemp -d)/kb"
cp -R tests/fixtures/search "$KB_ROOT" && kb init "$KB_ROOT" --no-examples
```

## 1. Search and pick (US1)

```sh
kb                       # type: 5xx auth   → sumo/5xx-by-authorizer first, preview on the right
                         # Enter            → TUI closes, the Sumo query is printed
kb 5xx auth              # opens with "5xx auth" already in the search line
kb | pbcopy              # (macOS) pick lands in the clipboard, no newline; try `kb | wc -c`
x=$(kb --print id); echo "$x"
kb --print path          # absolute path of the picked file
kb --print json | jq .title
# Esc with text clears it; Esc on an empty line exits: echo $?  → 130, nothing printed
```

Expected: while the TUI is open nothing reaches the pipe; after Enter the pipe gets exactly the pick.

## 2. Read the preview (US2)

Select one entry of each type (`type:sql`, `type:sumo`, `type:cw`, `type:cmd`, `type:link`,
`type:code`): payload highlighted with line numbers, `{{region}}` marked in `cmd/kubectl-pods`,
variables listed with kind and default, notes rendered (bold, code, lists), the URL prominent for
links, repo/path/line/ref/symbol for code. Shift-↓ / Shift-↑ scroll a long payload without moving the
selection.

## 3. Types and tags (US3)

Press Tab / Shift-Tab: the chip row moves through `all, sql, sumo, …` and `type:x` appears as the
active type; counts per type update as you type. Type `tag:prod ` (with the trailing space): a chip
appears; Backspace on an empty line removes it. `"role cache"` and `-tag:stage ` behave as in
`kb search`.

## 4. Edit in place (US4)

```sh
EDITOR='sed -i.bak s/Cache/Caching/' kb      # select prompt/cache, Ctrl-E → back in the TUI, title changed
EDITOR='sh -c "echo bad > \"\$1\"" _' kb     # Ctrl-E on any entry → notice names the first error
kb edit prompt/cache                         # the scriptable form
```

## 5. Help and states (US5)

`?` on an empty line or F1 opens the overlay; any key closes it. Resize below 60×15: "Terminal too
small". Narrow terminal (< 100 columns): Alt-P toggles the preview. `NO_COLOR=1 kb` uses no colors.
`kb </dev/null >/dev/null 2>&1; echo $?` outside a terminal → prints help, exit 2 (needs no
controlling terminal, e.g. under `setsid`).

## 6. Scriptable equivalents

```sh
kb show sumo/5xx-by-authorizer                 # what Enter prints
kb show sumo/5xx-by-authorizer --print json    # same document as the TUI's json pick
kb search auth --ids | head -1 | xargs kb show --print path
```

## Automated checks

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo test                                          # unit, snapshots, pty end to end, SC-003 loop
KB_UPDATE_SNAPSHOTS=1 cargo test -p kb --test tui_snapshots   # only when a screen change is intended
cargo test --release -p kb --test perf -- --ignored --nocapture   # SC-001, SC-002, FR-019
```

| Check | Proves |
|-------|--------|
| `tui_snapshots.rs` (every type × 3 sizes × 3 themes, plus states) | SC-004, US2, FR-005 to FR-008 |
| `tui_pty.rs` US1 to US5 flows | acceptance scenarios, FR-001, FR-004a, FR-017, FR-018 |
| `tui_pty.rs` restore loop (100 runs: Enter, Esc, Ctrl-C, SIGTERM, panic) | SC-003, FR-002 |
| `cli_show.rs` | scriptable equivalents (constitution IV) |
| `perf.rs` (ignored) | SC-001, SC-002, FR-019 |
| unit tests in `tui/` and `kb-core` | `update`, key map, chips, line editing, counts, JSON, config |

## Manual terminal matrix (SC-006)

Run sections 1, 2 and the checks below once on each terminal and tick the box. This cannot be
automated; do it before merging the milestone.

| Terminal | Typing + pick | Colors (dark & light) | Resize | Shift-↑/↓ scroll | Ctrl-Y (OSC 52) | `kb \| pbcopy` | Restores on Ctrl-C |
|----------|---------------|----------------------|--------|------------------|-----------------|----------------|--------------------|
| macOS Terminal.app | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| iTerm2 | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| Linux VTE terminal (GNOME Terminal) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| Linux GPU terminal (Alacritty or kitty) | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| Any of the above inside tmux | ☐ | ☐ | ☐ | ☐ | ☐ (needs `set -g set-clipboard on`) | ☐ | ☐ |
| Over SSH | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |

Terminals that keep Shift-PgUp/Shift-PgDn (or Ctrl-Home/Ctrl-End) for themselves are expected to
miss those keys; that is not a failure.
