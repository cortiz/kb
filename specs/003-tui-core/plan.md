# Implementation Plan: TUI Core — Search and Preview

**Branch**: `003-tui-core` | **Date**: 2026-10-08 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/003-tui-core/spec.md` (including its 2026-10-08 clarifications)

## Summary

M3 adds the full-screen terminal UI: run `kb`, type, see ranked results and a rich preview, press
Enter to print the pick. It builds only on M1 and M2 and adds no OS-specific code beyond opening
the controlling terminal.

The design rests on five decisions (reasons and rejected alternatives in [research.md](research.md)):

1. **State, view and effects are separate.** `AppState` changes only through
   `update(Action) -> Vec<Effect>`; `draw(&AppState, &Theme)` is a pure function of state and size
   that renders into a ratatui buffer. The event loop is the only impure part. This is what makes
   FR-021 (snapshot tests with no terminal) and the 16 ms frame budget checkable.
2. **Two threads, latest wins.** The UI thread never touches the index. A worker thread owns a
   `Kb`, runs refreshes, searches and preview building, coalesces queued requests to the newest
   one, and tags every reply with a generation number; the UI drops stale replies (FR-012). The
   first frame is drawn before the worker has finished its first refresh (FR-003, SC-001).
3. **The UI is drawn on `/dev/tty`, never on stdout.** stdout carries only the pick, after the
   terminal is restored, so `kb | pbcopy` and `$(kb)` work (spec FR-001, FR-004a). Raw mode is
   restored by a guard, a panic hook and a signal flag (SC-003).
4. **Highlighting is syntect's parser with our own colors.** syntect parses SQL and shell with its
   standard grammars and Sumo and CloudWatch with two small embedded grammars; we map scopes to a
   fixed set of token classes, and the theme maps classes to colors. That makes dark, light and
   `NO_COLOR` the same code path. Highlighted lines are cached per entry and built on the worker
   (first parse of a language costs 20 to 35 ms, measured).
5. **M2 grows a few read-only hooks, not a second search.** A cached-index `refresh`/`query`
   split, per-type counts, use count and last-edited time on results, an entry-to-JSON function,
   and config parsing for `theme` and `mouse`. All stay in `kb-core` (constitution IV); the TUI
   only calls them (FR-020).

Because constitution IV requires every TUI action to have a scriptable CLI equivalent, the plan also
adds two tiny commands that share the TUI's code: `kb show <id> [--print mode]` (what Enter prints)
and `kb edit <id>` (what Ctrl-E does).

## Technical Context

**Language/Version**: Rust stable, edition 2024, `rust-version = "1.88"` (unchanged). The dependency
set below was built and its tests compiled with `cargo +1.88` in a scratch crate before planning.

**Primary Dependencies** (new in M3; each justified in [research.md R14](research.md#r14-dependencies)):

- `kb`: `ratatui` 0.30 (TUI, constitution), `syntect` 5 with `parsing`, `default-syntaxes`,
  `regex-fancy`, `yaml-load` (highlighting, constitution; pure Rust, no C), `signal-hook` 0.3
  (SIGTERM/SIGHUP flags without `unsafe`; already in the tree through crossterm), `unicode-width` 0.2
  (display width for truncation; already in the tree through ratatui), and `crossterm` 0.29 as a
  direct dependency with the `use-dev-tty` feature (the same crate ratatui uses) so that `echo x | kb`
  reads keys from `/dev/tty`.
- `kb-core`: nothing new. `pulldown-cmark` (already a dependency) also renders notes in the `kb` crate.
- Dev only: `vt100` 0.15 (terminal screen emulator for end-to-end tests), `portable-pty` 0.9
  (pseudo-terminal to drive the real binary).

**Storage**: none new. Index version goes 1 → 2 (adds the `updated` value to each entry row), which
the existing "version mismatch means rebuild" rule handles with no migration.

**Testing**: `cargo test`. Pure unit tests for `update`, key map, query/chip model, layout and
truncation. Snapshot tests of `draw` through ratatui's `TestBackend` (text plus styles) for every
entry type at 80×24, 120×40, 200×60 in dark, light and `NO_COLOR`. End-to-end tests drive the real
`kb` in a pseudo-terminal and read the screen with `vt100`. Timing tests are ignored by default and
run in release mode in CI, like M1 and M2.

**Target Platform**: macOS arm64/x86_64 and Linux x86_64/aarch64. The only OS-touching code is
opening `/dev/tty` and launching the editor through `sh`, both in `crates/kb/src/platform/`.

**Project Type**: Cargo workspace; `kb-core` library plus the `kb` binary (unchanged).

**Performance Goals**: first frame ≤ 100 ms (SC-001); keystroke to redrawn list ≤ 50 ms (SC-002);
one `draw` ≤ 16 ms at 10,000 entries (FR-019). Draw cost depends on the visible area, not the KB size.

**Constraints**: no network, no telemetry. Never write to stdout while the UI is up. Never read
entry files or the index from the TUI layer (FR-020). No `unsafe` (workspace lint).

**Scale/Scope**: 10,000 entries, 7 entry types, 1 screen, 1 overlay, 13 key bindings plus line
editing, 3 new commands (`kb`/`kb tui`, `kb show`, `kb edit`).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| # | Principle | Status | How this plan complies |
|---|-----------|--------|------------------------|
| I | Plain Files Are the Source of Truth | Pass | The TUI reads entries and never writes them. The only write is usage in `.kb/state.json` via M2's API. Ctrl-E hands the file to the user's editor. |
| II | The Index Is Disposable | Pass | Index version bump 1 → 2 is handled by the existing rebuild rule. Nothing new is stored outside `.kb/`. |
| III | Local-Only, No Server | Pass | No network. OSC 52 only writes escape codes to the user's own terminal. No daemon: the worker thread lives and dies with the process. |
| IV | Terminal Only, One Core | Pass | Search, ranking, counts, usage, entry JSON, config parsing and the payload/variable splitting stay in `kb-core`. The TUI is rendering, key handling and terminal glue. Every TUI action has a CLI equivalent: pick → `kb show`, edit → `kb edit`, search/filters → `kb search`, refresh → `kb reindex`. |
| V | Portable by Construction | Pass | Opening the controlling terminal and spawning the editor sit behind the `Platform` trait with one implementation per OS. OSC 52 is the clipboard fallback the principle names. Works over SSH and tmux (tmux passthrough wrapper). |
| VI | Secrets Are Never Stored | Pass | Nothing new is persisted. Entries with secret findings are not indexed (M2), so they never reach the list. `kb show --print json` prints only what is in the entry. |
| VII | Keyboard-First and Fast | Pass (budgeted) | Every action is a key. Budgets: first paint 100 ms, 50 ms to results, 16 ms per frame, each with a release-mode test. Running commands with confirmation is M7. |
| VIII | Test-First Core | Pass | Tasks order failing tests first. New `kb-core` code (counts, JSON, config, payload pieces) is written test-first. Snapshots cover every type. CI unchanged: fmt, clippy `-D warnings`, tests on both OSes. |
| IX | Simplicity First | Pass | Four runtime dependencies, each already half-present or mandated. No async runtime, no plugin system, no custom key binding. Paging by re-query instead of a cursor protocol. Two small extra commands justified by IV. |

**Gate result**: pass, no exceptions. Complexity Tracking stays empty.

**Post-design re-check**: still pass. The design adds no process, no file outside `.kb/`, and no
dependency beyond those listed.

**Post-implementation re-check (2026-10-09)**: still pass, with these notes.

- I: the TUI reads entries only through `Kb`; the only writes are `state.json` (via `record_use`),
  the index in `.kb/`, and the user's editor on the file the user asked to edit.
- II: index version 2 rebuilds on first use (tested).
- III: `cargo tree -e normal --depth 1 -p kb` shows `ratatui`, `syntect`, `signal-hook`,
  `unicode-width` and `crossterm` (feature `use-dev-tty`) as the new direct dependencies (and `pulldown-cmark`, already in the tree through
  `kb-core`); no networking crate anywhere. OSC 52 writes only to the user's terminal.
- IV: pick output, entry JSON, config, payload pieces, counts and `lint_entry` are in `kb-core`. The
  scriptable twins are `kb show`, `kb edit`, `kb search`, `kb reindex`.
- V: opening the controlling terminal and running the editor are `Platform` methods with one
  implementation per OS (sharing a private unix helper).
- VII (release build, macOS arm64, 10,000 entries): first frame 25 ms (budget 100), keystroke to
  updated list 3 ms median with the refresh timer on or off (budget 50), one `draw` with 10,000
  results 0.15 ms (budget 16), type a title and pick it 0.3 s (budget 5 s).
- VIII: **not followed to the letter.** Most tests were written together with the code of their
  module rather than seen failing first; the exceptions that were red first are the `kb-core`
  additions (compile errors before the API existed), the scroll, type-cycling, editor, help,
  full-preview and mouse behaviors in `app.rs`, and the changed-file preview reload (found by an
  end-to-end test and fixed with a test). Coverage the principle asks for is there: 440 tests pass on
  stable and on 1.88, snapshots for 8 entry kinds × 3 sizes × 3 themes plus the special screens,
  a 100-run terminal-restore loop, pseudo-terminal tests of every story.
- IX: no async runtime, no custom key binding; the only additions beyond the spec are `kb show` and
  `kb edit` (constitution IV) and a two-column compact help for short terminals.

**Not done**: the manual terminal matrix of SC-006 (T070) needs a person at Terminal.app, iTerm2, a
VTE terminal, Alacritty or kitty, tmux and an SSH session; it is unticked in `tasks.md` and listed in
`quickstart.md`. Linux CI has not run. Also not built: the worker's "fatal index error twice ends the
thread" rule (a failed refresh shows a notice and the next timer tick retries).

## Project Structure

### Documentation (this feature)

```text
specs/003-tui-core/
├── plan.md              # This file
├── research.md          # Phase 0: decisions, measurements, rejected alternatives
├── data-model.md        # Phase 1: AppState, messages, preview model, theme, key map
├── quickstart.md        # Phase 1: runnable validation, including the manual terminal matrix
├── contracts/
│   ├── cli.md                    # kb / kb tui / kb show / kb edit: flags, tty rules, exit codes
│   ├── keys.md                   # every binding, by mode; key bar and help overlay rules
│   ├── screen.md                 # layout, regions, row and preview formats, states
│   ├── core-api.md               # additions to kb-core (M2 hooks, entry JSON, config)
│   ├── config.md                 # theme and mouse keys
│   └── entry.schema.json         # kb show --print json / TUI pick json
├── checklists/requirements.md
└── tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
Cargo.toml                         # + ratatui, syntect, signal-hook, unicode-width; dev: vt100, portable-pty

crates/kb-core/src/
├── config.rs                      # Config { theme, mouse }; read_config(path)
├── entry_json.rs                  # entry_json(&Entry, abs_path) -> serde_json::Value
├── payload.rs                     # payload_pieces(&Entry): text and {{variable}} pieces
├── search.rs                      # + refresh()/query(), type_counts, use_count, updated, modified, lint_entry()
└── index/{schema,doc,mod}.rs      # INDEX_VERSION 2: entries.updated

crates/kb/src/
├── main.rs                        # optional subcommand, `--print`, tty check, show/edit commands
├── platform/                      # + open_tty(), run_editor() (unix; one impl per OS file)
├── commands.rs                    # run_show, run_edit (shared with the TUI's pick/edit effects)
└── tui/
    ├── mod.rs                     # run(): terminal guard, event loop, effect execution
    ├── terminal.rs                # setup/restore guard, panic hook, signal flag, OSC 52
    ├── app.rs                     # AppState, Action, Effect, update()
    ├── keys.rs                    # KeyMap table, const duplicate check, key bar and help text
    ├── input.rs                   # LineEditor (cursor, Ctrl-A/W/U, Home/End) and chips
    ├── worker.rs                  # request/reply types, coalescing, generations
    ├── view.rs                    # draw(): header, search, chips, list, preview, key bar, overlays
    ├── layout.rs                  # regions per size, narrow and too-small modes, truncation
    ├── theme.rs                   # type colors, dark/light/no-color, class → style
    ├── highlight.rs               # syntect parse → token classes, {{var}} marking, cache
    ├── markdown.rs                # notes → styled blocks with wrapping
    ├── preview.rs                 # PreviewModel from Entry (metadata, payload, vars, fields, notes)
    └── syntaxes/{sumo,cwi}.sublime-syntax

crates/kb/tests/
├── tui_snapshots.rs               # SC-004 matrix, snapshots in tests/snapshots/
├── tui_pty.rs                     # US1 to US5 end to end in a pseudo-terminal; SC-003 runs
├── cli_show.rs                    # kb show / kb edit
└── perf.rs                        # + first frame, keystroke latency, frame time (ignored)
tests/snapshots/tui/               # stored screens: <case>.<size>.<theme>.txt
```

**Structure Decision**: all UI code goes in a `tui` module of the existing `kb` crate (the constitution
puts the CLI and TUI in the binary crate); everything that is data or policy goes in `kb-core`. No
new crate.

## Complexity Tracking

> Empty: no constitution violations.
