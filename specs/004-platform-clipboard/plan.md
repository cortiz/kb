# Implementation Plan: Platform Layer, Clipboard and Basic CLI

**Branch**: `004-platform-clipboard` | **Date**: 2026-10-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/004-platform-clipboard/spec.md` (including its 2026-10-09 clarifications)

## Summary

M4 makes Enter copy. It grows the thin M3 `Platform` trait into the full platform layer
(clipboard, opener, paths, editor, environment detection), adds a TOML config with precedence and
source tracking, and adds `kb copy`, `kb config` and `kb doctor` plus a richer `kb show`.

Six decisions carry the design (reasons and rejected alternatives in [research.md](research.md)):

1. **Decide in pure code, act in thin code.** Environment detection, the clipboard method chain,
   config precedence and the OSC 52 byte sequence are pure functions of plain values, so they are
   unit-tested on any host (a Mac test run covers the Wayland and X11 rules). The OS-specific
   `impl Platform` files only read the real environment and spawn processes.
2. **One clipboard chain, forced methods included.** `chain(env, settings) -> Vec<Method>` returns
   the ordered attempts. A configured method goes first and the rest follow (clarification 1);
   `stdout` is always last, so a copy never silently loses the text. Every attempt has a 2 s
   deadline and records why it was skipped.
3. **The same function copies in the CLI and the TUI.** `kb copy`, Enter and Ctrl-Y all call
   `clipboard::copy`. Enter copies after the terminal is restored (no alternate-screen concerns);
   Ctrl-Y copies while the UI is open and writes OSC 52 through the existing TUI tty writer.
4. **Config is a table.** One `SETTINGS` table (key, type, default, env name, doc line) drives
   parsing, validation, `config init`, `config list`, `config get` and `config set`. The key map in
   M3 works the same way, and for the same reason: nothing can drift apart.
5. **`toml_edit` for the file.** It is the one library that keeps comments and order on write
   (FR-017) and reports the byte span of each value, which gives the line number for warnings
   (FR-016). The old `config.yml` is still read when no `config.toml` exists, with a warning.
6. **Target resolution lives in the core.** `Kb::resolve(text)` (exact ID, `.md` or path, then
   search with a clear-winner rule) is domain logic and sits in `kb-core` (constitution IV). The CLI
   and the TUI-launch path only decide what to do with `Resolved`, `Ambiguous` or `NotFound`.

Facts found in the code that shaped the plan:

- M1 and M3 already read a **YAML** config (`config.yml`: `root`, `theme`, `mouse`). The spec wants
  TOML. This plan migrates (decision 5, R3); it is the only breaking change.
- `kb show <id> [--print mode]` already exists from M3. M4 keeps its output modes and adds query
  resolution, `--raw`, `--file`, `--no-color`; the formatted default becomes the preview rendering.
- OSC 52 code (base64, tmux wrapper, 74,000-byte cap) lives in `tui/terminal.rs`. It moves to the
  platform layer and gains GNU screen wrapping.
- **Spec conflict resolved here:** User Story 1 scenario 3 said OSC 52 prints `Copied via terminal`
  while FR-008 says "sent, never copied". FR-008 wins (also M3's rule that kb must not claim more
  than it knows); the scenario text in the spec was changed to match.

## Technical Context

**Language/Version**: Rust 2024, MSRV 1.88, `unsafe_code = forbid` (unchanged).

**Primary Dependencies**: existing `ratatui`, `crossterm`, `syntect`, `signal-hook`, `clap`,
`rusqlite`, `serde`. **New: `toml_edit`** in `kb-core` (config parse, validate with spans, and
comment-preserving edit). No other new dependency: PATH lookup, process timeouts, base64 and the
box-drawing/ANSI dump are small enough to write (R6, R9). Removed: nothing.

**Storage**: files only. Config at the platform path (R3). No new state in the KB root or `.kb/`.

**Testing**: `cargo test` (unit + integration), the existing pty harness (`tests/support/pty.rs`)
for Enter and Ctrl-Y end to end, fake tools on `PATH` for the native tools, `FakePlatform` for
TUI/CLI logic, a boundary test for FR-003, release timing tests for SC-002.

**Target Platform**: macOS (arm64, x86_64) and Linux (x86_64; Wayland and X11), unchanged.

**Project Type**: Cargo workspace, `kb-core` (library) + `kb` (lib `kb_cli` + bin `kb`).

**Performance Goals**: `kb copy <id>` ≤ 150 ms end to end on 10,000 entries with a local clipboard
(SC-002); config load adds no measurable cost (one small file); TUI budgets from M3 unchanged
(first frame ≤ 100 ms, keystroke ≤ 50 ms).

**Constraints**: a copy attempt never blocks longer than 2 s (FR-009); nothing is written to the
KB root by any new command (constitution I); config problems never stop kb from starting (FR-016).

**Scale/Scope**: 7 new subcommands (`copy`, `doctor`, `config init|path|get|set|list`) and a
reworked `show`; 10 settings; 6 new modules in `platform/`; a config rewrite and a `resolve`
module in `kb-core`; `doctor.rs` and `show.rs` in `kb`.

## Constitution Check

*GATE: pass before research; re-checked after design (second column).*

| Principle | Before | After design | Notes |
|---|---|---|---|
| I. Plain files are the source of truth | Pass | Pass | No new files in the KB root. `config show/set` touch only the config file. `doctor` checks writability inside `.kb/`, never by writing to the root. |
| II. Index is disposable | Pass | Pass | No index change. Use counts still go to `.kb/state.json` through `record_use`. |
| III. Local-only, no server | Pass | Pass | Clipboard tools and `/dev/tty` only. The URL opener runs only on a user action (M7); M4 only provides it. No network. |
| IV. Terminal only, one core | Pass | Pass | `Kb::resolve` and config parsing are in `kb-core`; the CLI and TUI call them. Every TUI action has a command: Enter/Ctrl-Y = `kb copy`, Ctrl-E = `kb edit`, preview = `kb show`. **Every new command has `--json`** (including `copy` and `config *`, which the spec did not list; see contracts/cli.md). |
| V. Portable by construction | Pass | Pass | This milestone is the principle. All OS calls behind `Platform`; FR-003 is enforced by a test. Config paths follow R3. Clipboard falls back to OSC 52. |
| VI. Secrets never stored | Pass | Pass | `config set` stores only the ten settings. `clipboard.command` is stored as text; `doctor` and `config list` print it as the user wrote it, and the docs say not to put secrets in it. Copying a payload never logs it. |
| VII. Keyboard-first and fast | Pass | Pass | SC-002 budget has a release test. Worst case (a hung clipboard tool) is bounded to 2 s and shown, not silent. |
| VIII. Test-first core | Pass | Pass, with an open gate | `kb-core` additions (config, resolve) and the pure platform functions get failing tests first. Honest scope: the glue (`impl Platform`, command wiring, TUI changes) is tested after, as in M3. CI on Linux is unverified until the first push; task T062 is the explicit "CI green on macOS and Linux" gate the constitution requires, and it needs your push. |
| IX. Simplicity first | Pass | **Justified** | One new dependency (`toml_edit`, R2). Everything else is std. The legacy `config.yml` fallback is the one extra path kept for compatibility (R3); it has a removal note. See Complexity Tracking. |

**Gate result**: no violations. Two items need a conscious call, both recorded below.

## Project Structure

### Documentation (this feature)

```text
specs/004-platform-clipboard/
├── plan.md              # This file
├── research.md          # Decisions R1–R14
├── data-model.md        # Entities and rules
├── quickstart.md        # Runnable validation scenarios
├── contracts/
│   ├── platform.md      # The Platform trait and the pure functions behind it
│   ├── cli.md           # copy, show, config, doctor: arguments, output, exit codes
│   ├── config.md        # The ten settings, precedence, file location, warnings
│   └── doctor.md        # Checks, statuses, hints and the JSON shape
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks output (not created here)
```

### Source Code (repository root)

```text
crates/
├── kb-core/src/
│   ├── config.rs        # REWRITE: SETTINGS table, Settings, Source, load(), edit via toml_edit,
│   │                    #   legacy config.yml fallback, warnings with file+line
│   ├── resolve.rs       # NEW: Resolution, clear_winner(), Kb::resolve()
│   ├── root.rs          # CHANGE: resolve_root takes the config's `root` value, no file reading
│   │                    #   (its tests and kb-core/config.rs tests write config.yml: rewritten)
│   └── search.rs        # CHANGE: expose Kb::resolve (delegates to resolve.rs)
└── kb/src/
    ├── platform/
    │   ├── mod.rs       # EXTEND: Platform trait (R1), Environment, re-exports
    │   ├── env.rs       # NEW: Environment + detect() from an env source (pure)
    │   ├── clipboard.rs # NEW: Method, chain(), copy(), run_tool() with timeout, describe()
    │   ├── osc52.rs     # NEW: sequence() with tmux/screen wrapping (moved from tui/terminal.rs)
    │   ├── opener.rs    # NEW: opener choice, open_url(), open_path()
    │   ├── fake.rs      # NEW: FakePlatform test double (pub, used by unit and integration tests)
    │   ├── unix.rs      # EXTEND: tty writer, tool spawn with timeout, editor with resolved cmd
    │   ├── linux.rs     # EXTEND: XDG paths, Wayland/X11 tool lists
    │   └── macos.rs     # EXTEND: ~/.config + Library paths, pbcopy
    ├── commands.rs      # EXTEND: run_copy, run_show (modes, resolution), run_config_*, run_doctor
    ├── doctor.rs        # NEW: checks and their rendering
    ├── show.rs          # NEW: preview-as-text (ratatui Buffer → ANSI) for `kb show`
    ├── main.rs          # CHANGE: new subcommands, config load + warnings, --print optional,
    │                    #   post-pick action, `init` hint says `root = "…"` in config.toml
    └── tui/
        ├── app.rs       # CHANGE: CopyOutcome → CopyReport, header config warning
        ├── mod.rs       # CHANGE: Enter copies after restore, Ctrl-Y uses clipboard::copy
        ├── terminal.rs  # CHANGE: base64/copy_sequence removed (moved to platform/osc52.rs)
        └── view.rs      # CHANGE: header warning; draw_preview split so `kb show` can reuse it
tests/
└── fixtures/…           # existing; one config fixture dir added
crates/kb/tests/
├── platform_boundary.rs # NEW: FR-003 check (in kb: repo-root tests/ are wired into kb-core)
├── cli_copy.rs  cli_config.rs  cli_doctor.rs   # NEW
├── cli_show.rs          # REWRITE: --print removed; --raw, --file, query resolution added
├── cli_init.rs          # CHECK: sets XDG_CONFIG_HOME only
├── tui_pty.rs   (extend: Enter copies, tui.enter=print, Ctrl-Y method message)
└── perf.rs      (extend: SC-002)
```

**Structure Decision**: keep the two-crate workspace. Pure rules in modules without `cfg`, thin
`cfg(target_os)` impls beside them, one test double. Config and resolution are domain logic, so
they go in `kb-core`; the clipboard chain is platform glue, so it stays in `kb`.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| New dependency `toml_edit` | FR-017 (keep comments/order on `config set`) and FR-016 (line numbers in warnings) | The `toml` crate parses but cannot write back preserving comments; hand-editing text lines would break on quoted keys, tables and inline tables. |
| Legacy `config.yml` read-only fallback | Without it, an existing `root:` in `config.yml` is silently ignored after upgrade and kb opens the wrong KB (`~/kb`) | A hard break needs no code but fails silently. The fallback reuses the M3 reader (~30 lines) and warns once; remove it in a later milestone. |
| `FakePlatform` is compiled into the library (not `#[cfg(test)]`) | Integration tests in `crates/kb/tests` can only use `pub` items of `kb_cli` | A `test-support` feature adds Cargo plumbing for ~120 lines of code. |
| Blocking copy on the UI thread (Ctrl-Y) | Keeps the event loop unchanged | A background copy thread is about 40 more lines. Worst case is a 2 s freeze with a hung X11 tool, with the message shown afterwards. `ponytail:` marker in code; upgrade if it ever bites. |

## Breaking changes and documentation

- **Config file**: `config.yml` → `config.toml`; `mouse` → `tui.mouse`. Legacy file still read with a
  warning (R3). Code touching `config.yml`: `platform/{linux,macos}.rs`, `kb-core/src/config.rs`,
  `kb-core/src/root.rs` and their tests, `tests/tui_pty.rs:184`, the `init` hint in `main.rs`.
- **`kb show --print MODE` is removed** (clarification 3); M3's `cli_show.rs` is rewritten.
- **`Outcome::Picked`** carries the ID, not the finished output (post-pick action in `main`).
- **README** (constitution: docs in the same change): the root-resolution list and `config.yml`
  example (lines ~49–54), the TUI section's Enter/Ctrl-Y text, the pointer to M3's
  `contracts/config.md` (~266), and new sections for `copy`, `show`, `config`, `doctor`.

## Post-design re-check

Re-read after `data-model.md`, `contracts/*` and `quickstart.md` were written: the table above
stands. Two risks that are real and not hidden by the design:

- **GNU screen OSC 52 wrapping is from documented practice, not tested against a real screen.** The
  unit tests check the bytes, not that screen forwards them. Listed in the manual matrix.
- **`kb show` preview rendering reuses `draw_preview`.** That function is private and takes
  `AppState` today. Splitting it so it takes a `PreviewModel` and a width is a refactor of M3 code
  that the existing 91 snapshots will catch if it drifts.

## Post-implementation re-check

What changed between the plan and the code, so the documents stay true:

- **Constitution**: unchanged result. One new dependency (`toml_edit`, builds on 1.88); nothing else
  added. FR-003 is enforced by `crates/kb/tests/platform_boundary.rs` (unit-test modules are exempt:
  they have to name the fakes they build). VIII's CI-green gate is task T062 and is still open.
- **Ctrl-Y never prints.** Inside the UI the `stdout` method is removed from the chain
  (`Place::Ui`); a copy with nowhere to go says so in the key bar. This closes a hole in R12: a
  forced `clipboard.method = "stdout"` would have written onto the UI's screen.
- **A forced `osc52` still falls back to a tool** (clarification 1), so a test that wants the
  size refusal must use `auto` over SSH, where no tool is in the chain.
- **`Platform::resolve_editor`** takes the `editor` setting (which already includes `KB_EDITOR`,
  because the environment layer of the config wins over the file) instead of reading the variable
  itself; `$VISUAL`, `$EDITOR` and `vi` are the fallbacks (`pick_editor`).
- **`config set` carries settings along** when the file it would create hides one that was in use
  (the macOS `~/Library` file, or the legacy `config.yml`), and says so on stderr.
- **`kb doctor` text marks** are `✓ ⚠ ✗` only when stdout is a terminal with colors; piped or with
  `NO_COLOR` they are `ok`, `warn`, `FAIL` (so scripts can grep them).
- **`kb show` width** is the terminal's width, else 100 columns; the preview is drawn by the same
  function as the UI's pane (`view::render_preview`) and the 91 M3 snapshots are unchanged.
- **`Kb::resolve`** lives in `crates/kb-core/src/resolve.rs`; `CLEAR_WINNER_RATIO = 1.5` was
  calibrated on the fixture KB (headline query: one match, 8.4; `latency`: four matches within
  0.2% of each other), so any ratio from about 1.01 to 8 works and 1.5 stays.
- **Not built**: the background thread for Ctrl-Y (the 2 s worst case stays, marked `ponytail:`),
  and anything that needs a person at a terminal (T061) or a push (T062).
