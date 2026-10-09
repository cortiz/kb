---
description: "Task list for M4: Platform Layer, Clipboard and Basic CLI"
---

# Tasks: Platform Layer, Clipboard and Basic CLI

**Input**: Design documents from `specs/004-platform-clipboard/`
**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Required. Constitution VIII makes red-green-refactor mandatory for `kb-core` (config,
resolve). The pure platform functions (chain, environment, OSC 52, paths) get the same treatment.
The glue (`impl Platform`, command wiring, TUI changes) is tested after or alongside, as in M3, and
the report says so. A "test" task is written and seen failing before its implementation task starts.
Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file they test.

**Organization**: Grouped by user story. US1 (Enter copies) is the MVP and needs the foundation
below; US2 adds `kb copy`/`kb show`; US3 config commands; US4 `kb doctor`; US5 moves the editor and
Ctrl-Y onto the platform layer. The FR-003 boundary check goes in early and turns green in US5.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: US1 to US5

## Path Conventions

Core: `crates/kb-core/src/`. Binary: `crates/kb/src/` (platform layer in `platform/`, UI in `tui/`).
Binary tests: `crates/kb/tests/`. Fixtures: `tests/fixtures/`. FR/SC numbers refer to
[spec.md](spec.md); R-numbers to [research.md](research.md).

## Ground rules for every task

- **Never run `kb` or a test against the default root `~/kb`, or the real config directory.**
  Always set `KB_ROOT` (a temp copy of a fixture), `HOME` and `XDG_CONFIG_HOME` to temp dirs. A bare
  `kb` or `kb config set` would touch the real files.
- **The harness is hermetic by default (T015)**: `KB_CLIPBOARD_METHOD=stdout`, temp `HOME` and `XDG_CONFIG_HOME`, no real tools on `PATH` ahead of the fakes. **Never write to the real clipboard from a test** except the one `#[ignore]`d local test (T060).
  Tests put fake `pbcopy`, `wl-copy`, `xclip`, `xsel` scripts in a temp dir at the front of `PATH`.
- Fake tools are shell scripts that write stdin to a file: `cat > "$OUT"`; hang cases use
  `sleep 30`; the forking case is `(sleep 30 &) ; exit 0` to prove stdout is not inherited.
- Pty tests set `HOME`, `KB_ROOT`, `TERM=xterm-256color`, `NO_COLOR` and `PATH` explicitly.
- Every change keeps `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`
  green and the 447 existing tests passing (some are rewritten on purpose, listed per task).

---

## Phase 1: Setup

- [X] T001 Add `toml_edit` to `[workspace.dependencies]` in `Cargo.toml` and to `crates/kb-core/Cargo.toml` (`default-features = false` with only the features needed to parse and edit; check its docs). Run `cargo +1.88 build --workspace`; if the newest version needs a newer compiler, pin the newest that builds on 1.88 and say so in a comment (R14). No other new dependency.
- [X] T002 [P] Create empty module files so the crates build: `crates/kb-core/src/resolve.rs` (declare in `lib.rs`), `crates/kb/src/platform/{env,clipboard,osc52,opener,fake}.rs` (declare in `platform/mod.rs`), `crates/kb/src/{doctor,show}.rs` (declare in `lib.rs`).
- [X] T003 [P] Create config fixtures in `tests/fixtures/config/`: `good.toml` (every setting present, with comments, blank lines, a trailing comment and unusual spacing, for byte-preservation tests), `bad.toml` (one unknown key `clipbaord.method` on line 3 and one bad value `clipboard.method = "xclipp"` on line 6 → exactly two warnings), `syntax-error.toml`, `legacy/config.yml` (`root`, `theme: dark`, `mouse: true`). Add a `config/` section to `tests/fixtures/README.md`.

**Checkpoint**: `cargo build --workspace` passes; nothing behaves differently yet.

---

## Phase 2: Foundational (blocks every story)

**Purpose**: config loading, environment detection, the platform trait, the clipboard chain, OSC 52,
the fake, the opener and the boundary check. No user-visible command changes yet.

### Config loading (kb-core, strictly test-first)

- [X] T004 [P] Tests first (red) in `crates/kb-core/src/config.rs` `mod tests`, for the new API in [data-model.md](data-model.md#setting-and-settings-kb_coreconfig): every key's default; precedence flag > env > file > default with the right `Source` for each; unknown key → one warning with line; bad value → one warning with line and the text "expected one of …" and the default used; unparseable file → one warning, all defaults; each problem yields exactly one warning even if `load` is called twice on the same text; empty env var counts as unset; env booleans accept `1/0/yes/no/true/false`, file booleans only TOML `true/false`; `clipboard.osc52_max_bytes = 0` rejected (positive integer, "must be at least 1"); legacy `config.yml` (fixture) is read for `root`, `theme`, `mouse`→`tui.mouse` with the deprecation warning and only when no `config.toml` exists; a present `config.toml` hides the yml. Values quoted verbatim from data-model: `tui.enter` is `copy|print` default `copy`; `clipboard.method` is `auto|native|osc52|stdout` default `auto`; `clipboard.osc52_max_bytes` default `74000`; `clipboard.trim_trailing_newline` default `true`; `theme` is `auto|dark|light` default `auto`; `tui.mouse` default `false`.
- [X] T005 Implement `crates/kb-core/src/config.rs` (replaces the YAML-only reader): the `SETTINGS` table (key, kind, default, env name, doc line), `Settings`, `Resolved<T>`, `Source`, `ConfigWarning`, `Loaded`, and `load(files: &[PathBuf], get_env: &dyn Fn(&str) -> Option<String>, flags: &Flags) -> Loaded` using `toml_edit` (line from the value's span, R2). Keep `ThemeChoice`. Reuse the existing YAML reader only for the legacy fallback. Never returns `Err` for content problems (FR-016). T004 goes green.
- [X] T006 [P] Rewrite `crates/kb-core/src/root.rs`: `RootInputs` loses `config_file` and gains `config_root: Option<String>` (the already-resolved `root` setting); the file-reading code and its YAML config tests are removed or moved to T004. Update the tests first (red), then the code. `RootError::Config` goes away; `resolve_root` order stays flag, `KB_ROOT`, config, `~/kb` (FR-012).

### Environment, paths and the trait

- [X] T007 [P] Tests first in `crates/kb/src/platform/env.rs`: `Environment::detect(get, os)` table from R8 — `SSH_CONNECTION`/`SSH_TTY`/`SSH_CLIENT` each alone sets `ssh`; empty strings count as unset; `TMUX` sets `tmux`; `STY` sets `screen` only when not in tmux; Linux `WAYLAND_DISPLAY` → Wayland even if `DISPLAY` is also set; `DISPLAY` only → X11; neither → None; macOS → Quartz regardless; `COLORTERM=truecolor` and `24bit` → `true_color`, `256color` → false; `TERM`/`TERM_PROGRAM` copied.
- [X] T008 Implement `Environment`, `Os`, `Display` and `detect` in `crates/kb/src/platform/env.rs`. T007 green.
- [X] T009 [P] Tests first in `crates/kb/src/platform/mod.rs` for the pure `config_paths(os, xdg, home) -> ConfigPaths { read: Vec<PathBuf>, write: Option<PathBuf> }`: Linux with absolute `XDG_CONFIG_HOME`, relative or empty (ignored), no home; macOS returns `~/.config/kb/config.toml` then `~/Library/Application Support/kb/config.toml` then the two `config.yml` paths, and `write` is the first; Linux write is the first read candidate (clarification 2, FR-011); plus `data_dir`/`cache_dir` for both OSes (R1 table). Then implement it.
- [X] T010 Extend the `Platform` trait in `crates/kb/src/platform/mod.rs` to the full list in [contracts/platform.md](contracts/platform.md) with **no default methods**; add the supporting types (`NativeTool`, `ToolCommand`, `ToolError`, `OpenerCommand`, `Opened`, `Editor`). Remove the M3 `config_file` method; The `kb` crate stays red until the impls (T011–T013) and the startup rewire (T014) land; do them in one sitting.
- [X] T011 Tests first, then implement shared helpers in `crates/kb/src/platform/unix.rs`: `find_tool` (PATH lookup, execute bit, R6; a directory or non-executable file is not a match); `run_tool(cmd, text, timeout)` per R5 — stdin piped and written on a helper thread, stdout/stderr null, poll `try_wait`, kill on deadline. Tests use scripts: exit 0 → Ok; exit 1 → `ToolError::Failed(1)`; `sleep 30` → `TimedOut` after about 2 s (use a 200 ms timeout parameter in the test); a script that backgrounds a process holding no pipes and exits 0 → Ok quickly (the `xclip` shape); a tool that never reads stdin with a 1 MB payload still times out instead of blocking the writer; `write_tty`; `tmux_allows_passthrough` parsing (pure function over the command output: `on`, `all` → true, `off` → false, error → true, not in tmux → None).
- [X] T012 [P] Implement `crates/kb/src/platform/linux.rs`: `config_files` and `config_write_path` from `config_paths`, `data_dir`/`cache_dir`, `native_tools` (Wayland tools first when `WAYLAND_DISPLAY`, then X11 `xclip -selection clipboard -in`, `xsel --clipboard --input` when `DISPLAY`, R4/R5), `opener` = `xdg-open`, delegating the rest to `unix.rs`. Add `#[cfg_attr(not(target_os = "linux"), allow(dead_code))]` on pure helpers that only Linux impls call, so macOS clippy stays clean. Replace the old tests that asserted `config.yml`. **Unit-test the exact argument vectors** returned by `native_tools`: `wl-copy` (none), `xclip -selection clipboard -in`, `xsel --clipboard --input` (R5), and that Wayland tools precede X11 tools when both displays are set.
- [X] T013 [P] Implement `crates/kb/src/platform/macos.rs` the same way (`pbcopy`, `open`, two config locations, Library data/cache dirs). Replace the old `config.yml` tests. Unit-test that `native_tools` is exactly `pbcopy` with no arguments.
- [X] T014 Rewire startup in `crates/kb/src/main.rs` so the `kb` crate compiles again after the config and trait changes: call `config::load(platform.config_files(), env, flags)` once; build `RootInputs { config_root: settings.root… }` instead of `config_file`; take theme and mouse from `Settings` (`tui.mouse`) instead of `read_config`; keep warnings collected in a `Vec` for now. Convert the one test that writes a legacy file, `crates/kb/tests/tui_pty.rs:184` (`config.yml` → `config.toml`, `mouse: true` → `[tui] mouse = true`), so the M3 theme/mouse tests do not silently use the legacy fallback. `cli_init.rs` and `cli_lint.rs` only set `XDG_CONFIG_HOME` and need no change. **The `kb` crate does not compile between T005 and this task; run `cargo test -p kb-core` for the tasks in between.**
- [X] T015 Make the test harness hermetic before Enter starts copying by default: in `crates/kb/tests/support/pty.rs` (`Spec::new`) and `crates/kb/tests/support/mod.rs` (the `Command` helper) set `KB_CLIPBOARD_METHOD=stdout` **only for the TUI/pty helpers and the copy-style commands (`kb`, `kb copy`)**, not in the generic helper used by config and doctor tests, `HOME` and `XDG_CONFIG_HOME` to temp dirs, and prepend an empty fake-tools directory to `PATH`, so no test can reach the real `pbcopy`/`wl-copy`. Add `Spec::with_fake_tool(name, script)` and `Spec::allow_clipboard_method(..)` that the copy tests use to override it. Apply the same env to the Command-based tests in `crates/kb/tests/perf.rs` and the 100-run SC-003 loop in `tui_pty.rs`. With `stdout` as the default method, M3 tests that expect the payload on stdout after Enter keep passing; review those that assert on stderr. The `kb config` and `kb doctor` helpers leave the variable unset, so source assertions (default/file/env/flag) are not polluted.

### OSC 52, clipboard chain and the copy function

- [X] T016 [P] Tests first in `crates/kb/src/platform/osc52.rs`: base64 vectors (moved from `tui/terminal.rs`), `sequence(text, wrap, max_bytes)` for `Wrap::None` (`ESC ] 52 ; c ; … BEL`), `Wrap::Tmux` (`ESC P tmux ; …` with every ESC doubled, `ESC \`), `Wrap::Screen` (DCS wrapping, chunks of at most 250 bytes joined by `ESC \ ESC P`, R7), the size rule: exactly `osc52_max_bytes` text bytes is allowed, one more is `TooLarge` (R7), UTF-8 multi-byte text is measured in bytes. Then implement, and make `tui/terminal.rs` call `osc52::sequence` instead of its own copy (delete its base64 and `copy_sequence`, keep its other items); the existing M3 Ctrl-Y tests must stay green.
- [X] T017 [P] Tests first in `crates/kb/src/platform/clipboard.rs` for `chain(env, settings, tools)` — a table-driven test with about 25 rows covering R4: macOS local, macOS over SSH (goes straight to OSC 52), Wayland, Wayland + X11 (Wayland tools first), X11 only, X11 over SSH (OSC 52 first), no display, `native` forced over SSH (tools first), `osc52` forced (OSC 52 then tools then stdout), `stdout` forced (only stdout), custom `clipboard.command` replacing the tool list and sitting first for `auto` and `native`, tools missing from PATH (kept in chain, marked unavailable). Then implement `Method`, `ClipboardSettings`, `chain`.
- [X] T018 [P] Implement `FakePlatform` in `crates/kb/src/platform/fake.rs` (public): built from an `Environment` plus a table `tool name → Ok | Fail(code) | Hang | Missing`, records every call as `Vec<Call>`; its `run_copy_tool` returns the scripted result at once (`Hang` returns `TimedOut` without sleeping). Add a test that calls every `Platform` method on it (SC-004), so a method added without a fake fails to compile and one never exercised fails the test.
- [X] T019 Tests first, then implement `copy(platform, env, settings, text, id) -> CopyOutcome` in `crates/kb/src/platform/clipboard.rs` per [data-model.md](data-model.md#copyoutcome-replaces-the-m3-copiedtoolargefailed-enum): trims trailing `\n`/`\r` when `clipboard.trim_trailing_newline` (and not otherwise); counts lines; tries the chain in order with `FakePlatform`; a missing tool is an `Attempt` with an install hint (`install wl-clipboard`, `install xclip or xsel`); a tool exit-1 and a timeout are `Attempt`s and the chain continues (FR-009); forced `native` that fails falls through to OSC 52 then stdout (clarification 1); `used.confirmed` is true only for native and custom; OSC 52 over the limit → `Attempt` "TooLarge"; **if nothing else delivered and `stdout` was reached only because of a size refusal, `used` is `None` and nothing is to be printed (R7)**, but an explicit `method = "stdout"` or `stdout` reached for other reasons prints; `message(id)` produces exactly `Copied <id> (N lines) via <tool>`, `Sent <id> (N lines) via terminal (OSC 52)`, `Printed <id> (N lines) to stdout; no clipboard method worked` and the size-refusal text of R7 (FR-008); `warnings()` yields one `kb: …` line per attempt.
- [X] T020 [P] Tests first, then implement `crates/kb/src/platform/opener.rs`: `open_url`/`open_path` choose `open` (macOS) or `xdg-open` (Linux) or `opener.command`; with `ssh` and `display == None` they return `Opened::Printed(url)` and spawn nothing (FR-010); `opener.command` containing arguments is split by the shell like the editor; failures return an `io::Error` naming the tool. Tested with `FakePlatform` and scripts on PATH. No command uses it yet (M7); it is `pub`, so no dead-code warning.
- [X] T021 Add the FR-003 check `crates/kb/tests/platform_boundary.rs` per R13: scans `crates/*/src/**/*.rs` outside `crates/kb/src/platform/`, with comments and doc strings stripped, for the forbidden tool names, environment names and `cfg(target_os`; fails with `file:line`. Add a second test that runs the checker on a temp tree containing one violation and expects it to report it. **The real-tree check would fail at first** on `tui/mod.rs` (`TMUX`) and `tui/theme.rs` (`COLORTERM`); T053 makes it green. Mark the real-tree test `#[ignore = "red until the TMUX/COLORTERM migration (T053)"]` from the start so the suite stays green, and remove the ignore in T053; the temp-tree self-test runs always.

**Checkpoint**: `cargo test --workspace` green (the real-tree boundary check is `#[ignore]`d until T053); config
loads with precedence, warnings and legacy fallback; the chain, OSC 52 and `copy()` are covered on
any host; nothing user-facing has changed yet.

---

## Phase 3: User Story 1 — Enter copies, on any machine (P1) — MVP

**Goal**: pick an entry in the TUI, press Enter, the payload is on the clipboard and one line is on
stderr (FR-024, spec US1 scenarios 1–6).

**Independent test**: pty tests with fake tools on `PATH` for each environment.

### Tests (write first, see them fail)

- [X] T022 [P] [US1] In `crates/kb/tests/tui_pty.rs`: with a fake `pbcopy` (macOS) or `wl-copy` plus `WAYLAND_DISPLAY` (Linux) first on `PATH`, search `5xx auth`, Enter → the fake received the exact payload, stderr (file redirect) has `Copied sumo/5xx-by-authorizer (N lines) via <tool>`, stdout is empty, exit 0, `.kb/state.json` use count is 1, the terminal was restored (alternate screen left).
- [X] T023 [P] [US1] In `crates/kb/tests/tui_pty.rs`: `SSH_CONNECTION` set → no tool is called; the raw pty output contains the OSC 52 sequence with the base64 payload; stderr says `Sent … via terminal (OSC 52)`, never "Copied"; with `TMUX` set the sequence is the tmux-wrapped form (US1 scenarios 3 and 4). Also `KB_CLIPBOARD_METHOD=stdout` → payload on stdout without an added newline, stderr says `Printed … to stdout`, exit 0 (scenario 5, edge case "exit 0").
- [X] T024 [P] [US1] In `crates/kb/tests/tui_pty.rs`: `tui.enter = "print"` in a temp `config.toml` and plain Enter prints the payload to stdout as in M3; `--print id` prints the ID even though `tui.enter` is `copy`; `--print` given with `tui.enter = "copy"` wins (scenario 6); `KB_CLIPBOARD_OSC52_MAX_BYTES=10` over SSH → nothing printed, no use recorded, exit 2, stderr names the size, the limit and `kb show --raw <id> | <tool>` (R7, FR-008).
- [X] T025 [P] [US1] Unit tests in `crates/kb/src/tui/app.rs` and `view.rs` for the header warning `⚠ config: N problems (run kb doctor)` (hidden when N = 0; truncates before the entry count on narrow widths), and a new stored screen `config-warning.120x40.dark.txt` through the existing snapshot harness.

### Implementation

- [X] T026 [US1] In `crates/kb/src/main.rs`: use the `Loaded.warnings` collected at startup (T014): for every non-TUI command print each warning once on stderr (`kb: <file>:<line>: …`, contracts/config.md); for the TUI pass the problem count into `Options` and print the warnings after the terminal is restored.
- [X] T027 [US1] Make `--print` optional: `TuiArgs.print: Option<PrintMode>` in `main.rs`. Change `tui::Outcome::Picked` to carry the entry `id` (not the finished output) in `crates/kb/src/tui/mod.rs`; remove its `ui_kb.pick(...)` call. Add `PostPick::{Copy, Print(PrintMode), Show(..)}` in `commands.rs` and choose it in `main`: `--print` → `Print`, else `tui.enter` (R12). Update the M3 pty/CLI tests whose expectations change (Enter no longer prints by default; they now pass `--print payload` or set `tui.enter = "print"`).
- [X] T028 [US1] In `crates/kb/src/commands.rs` add `run_pick(platform, kb, settings, id, PostPick) -> ExitCode`: for `Copy` it calls `kb.pick(id, Payload)`, `clipboard::copy`, prints `warnings()` then `message(id)` on stderr, prints the text to stdout without a newline when `used == Some(Stdout)`, records a use iff `sent()` (FR-008), and returns `0`, or `2` for the size refusal. `Print` and `Show` reuse the M3 print path.
- [X] T029 [US1] Add the header warning to `AppState`/`view.rs` (T025 green) and refresh any stored snapshot whose header changed (the `config-warning` one is new; no other should change, verify with `git diff tests/snapshots`).

**Checkpoint**: Enter copies on macOS (fake and, by hand, real `pbcopy`); T022–T025 green. The MVP is
usable: `kb`, search, Enter, paste.

---

## Phase 4: User Story 2 — Copy and show from the shell (P1)

**Goal**: `kb copy <id|query>` and a richer `kb show`, with the shared resolution rule (FR-018–020,
spec US2).

**Independent test**: CLI integration tests against the fixture KB with fake tools.

### Tests (write first)

- [X] T030 [P] [US2] Tests first in `crates/kb-core/src/resolve.rs` (red): exact ID; ID with `.md`; a relative path inside the root; an absolute path inside the root; a path outside the root → `NotFound`; a search with a clear winner (single result, or first ≥ `CLEAR_WINNER_RATIO` × second) → `Resolved`; several close matches → `Ambiguous` with at most 5 candidates in rank order; no match → `NotFound`; an exact ID is never treated as a search. **Calibration (R10)**: on the `search` fixture KB, `5xx auth` must resolve to `sumo/5xx-by-authorizer` and `latency` must be `Ambiguous`; record the scores that justified the constant in a comment next to it, and change the ratio if the first guess of 1.5 does not separate them. **This calibration is a hard gate:** `CLEAR_WINNER_RATIO` is not final, and US2 scenario 2 is not accepted, until both fixture cases pass with the recorded scores.
- [X] T031 [P] [US2] `crates/kb/tests/cli_copy.rs` (new): `kb copy <id>` with a fake tool copies the exact payload, stderr has the confirmation, use recorded, exit 0; `kb copy 5xx auth` resolves and names the ID; `kb copy latency` with no terminal → 5 lines `id<TAB>title` on stderr, exit 1, nothing copied; unknown → exit 1; `--method osc52` overrides config for the run; `--print` → exit 2 (clarification 3); `--json` shape from [contracts/cli.md](contracts/cli.md) including `attempts`; every `kind` of `link` and `code` entry copies the URL and `path:line` (FR-018); trailing newline trimmed by default and kept with `KB_CLIPBOARD_TRIM_TRAILING_NEWLINE=false`; size refusal → exit 2, no use.
- [X] T032 [P] [US2] Rewrite `crates/kb/tests/cli_show.rs`: default output piped (no color) equals the preview text, and contains the title, metadata line, line-numbered payload, variables and notes; under a pty the output contains SGR color codes; `NO_COLOR` and `--no-color` strip them; `--raw` is the payload only with no added newline; `--json` equals M3's `--print json`; `--file` is one absolute path; `--print` is now a usage error (exit 2); `--raw`/`--json`/`--file` conflict with each other; a query resolves as in `copy`; ambiguous without a terminal → exit 1 with candidates; `show` records no use. The old `--print` cases are deleted, not skipped.
- [X] T033 [P] [US2] In `crates/kb/tests/tui_pty.rs`: `kb copy <ambiguous query>` under a pty opens the TUI with the query filled in; Enter there copies even with `tui.enter = "print"` (clarification 3 / plan "ambiguous"); `kb show <ambiguous query>` under a pty opens the TUI and Enter prints the formatted entry; Esc exits 130 with nothing copied.

### Implementation

- [X] T034 [US2] Implement `crates/kb-core/src/resolve.rs` (`Resolution`, `CLEAR_WINNER_RATIO`, `TOP_CANDIDATES = 5`, `clear_winner`) and `Kb::resolve(&mut self, text) -> Result<Resolution, KbError>` in `search.rs` (R10). T030 green, calibration done.
- [X] T035 [US2] Split `draw_preview` in `crates/kb/src/tui/view.rs` into a function taking `&PreviewModel`, `&Theme`, the width and returning the rows (R9) and a thin wrapper that keeps the TUI behavior; all 91 M3 snapshots must pass unchanged.
- [X] T036 [US2] Implement `crates/kb/src/show.rs`: render a `PreviewModel` into a `ratatui::buffer::Buffer` of the terminal width (100 when not a terminal) and convert it to text — SGR sequences per style change when color is on, plain trimmed rows when off (R9). Unit tests: a known model renders to the stored plain text; with color the stripped output equals the plain output.
- [X] T037 [US2] Implement `run_copy` and the new `run_show` in `crates/kb/src/commands.rs` and the `Copy`/`Show` clap subcommands in `main.rs` (`--method`, `--json`, `--raw`, `--file`, `--no-color`; remove `--print` from `ShowArgs`); shared resolution helper `resolve_or_pick(...)` that on `Ambiguous` with a terminal launches the TUI with the query and the matching `PostPick`, and without a terminal lists candidates on stderr and exits 1. Update the module doc of `commands.rs` ("kb show: what picking id prints") to the new behavior.

**Checkpoint**: `kb copy` and `kb show` work from scripts and from an ambiguous query; T030–T033 green;
the calibration of R10 is recorded.

---

## Phase 5: User Story 3 — Configure kb once per machine (P2)

**Goal**: `kb config init|path|get|set|list`, safe edits, clear warnings (FR-014–017, FR-021, spec US3).

**Independent test**: `crates/kb/tests/cli_config.rs` with temp `HOME`/`XDG_CONFIG_HOME`.

### Tests (write first)

- [X] T038 [P] [US3] Tests first in `crates/kb-core/src/config.rs` for the edit API: `set_value` on `good.toml` changes only the target line (byte-compare everything else, comments and blank lines intact, order unchanged); setting a key whose table is missing appends the table; `init` then `set clipboard.method osc52` puts `method = "osc52"` directly under `[clipboard]` with the commented `# method` line kept (the placement check promised in contracts/cli.md); values are validated against the setting's kind before writing (`tui.enter = "maybe"` → error naming the allowed values); the writer uses a temp file in the same directory and a rename and leaves no temp file on failure; `render_template()` has one commented line per setting with its default and doc, parses as valid TOML when uncommented, and every `SETTINGS` row appears exactly once.
- [X] T039 [P] [US3] `crates/kb/tests/cli_config.rs` (new): `init` writes the template at the write path, creates missing directories, prints the path on stdout, exits 0; a second `init` exits 1 and leaves the file byte-identical (spec US3.2); `path` prints the write path and `path --read` lists candidates marking the one in use; on macOS-style paths with both files present `~/.config` wins and `doctor` (T047) warns; `get` shows the effective value from each source; `set` keeps comments/order (run on `good.toml`); `set` with an invalid value or unknown key → exit 2 and the file untouched; `set` into a read-only directory → exit 2 with a clear message and normal commands (`kb search`) still work (edge case); `set` on a key overridden by `KB_THEME` succeeds and prints the note on stderr; `list` text and `--json` show value, source (`default`, `file`, `env`, `flag`), file and line; no file is created by any other command (spec US3.1); SC-005: with `bad.toml`, `kb search x` still works and prints exactly two warnings on stderr (and none on stdout, `--json` output stays parseable). Run these with `KB_CLIPBOARD_METHOD` and every other `KB_*` setting variable unset except the one under test.
- [X] T040 [P] [US3] Unit tests for the `kb init` hint in `crates/kb/src/main.rs`: it now says `add root = "<path>" to <config.toml>` (TOML) instead of the YAML `root:` line.

### Implementation

- [X] T041 [US3] In `crates/kb-core/src/config.rs`: `set_value(text, key, value) -> Result<String, ConfigEditError>` using `toml_edit::DocumentMut`, `write_atomic(path, text)`, `render_template() -> String` from the `SETTINGS` table. T038 green.
- [X] T042 [US3] In `crates/kb/src/commands.rs` and `main.rs`: the `config` subcommand tree (`init`, `path [--read]`, `get`, `set`, `list`, each with `--json`) per [contracts/cli.md](contracts/cli.md), exit codes per FR-023; `set` prints the override note; update the `init` hint (T040 green). T039 green.

**Checkpoint**: a user can create, inspect and edit the config entirely from `kb config`.

---

## Phase 6: User Story 4 — Diagnose the environment (P2)

**Goal**: `kb doctor` explains why copying does or does not work (FR-022, spec US4, SC-003).

**Independent test**: `FakePlatform` scenarios for each doctor row; real-binary tests for exit codes.

### Tests (write first)

- [X] T043 [P] [US4] Tests first in `crates/kb/src/doctor.rs` with `FakePlatform` and fake `Kb` states, one test per row of [contracts/doctor.md](contracts/doctor.md): root missing → `fail`; root read-only → `warn`; index stale → `warn`, corrupt → `fail`; config clean/warnings/unreadable; second macOS config ignored → `warn`; legacy `config.yml` → `warn`; Wayland without `wl-copy` → `warn` with `install wl-clipboard`; X11 without tools → `install xclip or xsel`; no display and no SSH → OSC 52 note; Terminal.app with OSC 52 → warn; tmux with passthrough off → warn with `set -g allow-passthrough on`; method `stdout` → warn; no true color → warn; editor not on PATH → warn; opener missing → warn; the clipboard line always shows the 74000-byte limit and the `kb show --raw <id> | <tool>` workaround (SC-003's "payload too large"); exit status is 1 iff any `fail`, warnings alone give 0; `--json` matches the shape in doctor.md (parse it back).
- [X] T044 [P] [US4] `crates/kb/tests/cli_doctor.rs` (new): root missing → exit 1; a healthy fixture with a fake tool → exit 0 and a `✓` line per check (`ok`/`warn`/`FAIL` when `NO_COLOR`); `--json` is valid JSON with `"ok": true`; doctor never writes to the KB root (compare a directory listing before and after, `.kb/` excluded) (constitution I).
- [X] T045 [P] [US4] `--copy-test` under a pty in `tests/tui_pty.rs` style (`crates/kb/tests/cli_doctor.rs`): pasting the test string back → pass, exit 0; pasting something else → fail, exit 1, attempts listed; without a terminal → usage error, exit 2; it records no use.

### Implementation

- [X] T046 [US4] Implement `crates/kb/src/doctor.rs`: `Check`, `Status`, `run(...) -> Vec<Check>`, text and JSON renderers, the root-writable probe inside `.kb/` only (R11), index status through `Kb::status`, tmux via `Platform::tmux_allows_passthrough`. T043 green.
- [X] T047 [US4] Add `kb doctor [--copy-test] [--json]` to `main.rs`/`commands.rs`, including the paste prompt (`Paste here and press Enter:`, one line from stdin, compare). T044 and T045 green. Also make doctor mention the ignored second macOS config file checked in T039.

**Checkpoint**: on a machine where copying fails, `kb doctor` names the cause and a fix for the four
SC-003 scenarios (the "payload too large" one by showing the limit and the workaround).

---

## Phase 7: User Story 5 — The TUI uses the platform layer everywhere (P3)

**Goal**: editor and Ctrl-Y come from the platform layer; the TUI reads no OS environment (FR-025,
FR-026, spec US5).

**Independent test**: pty tests with a script editor named in `config.toml` and fake clipboard tools; the boundary test passes.

### Tests (write first)

- [X] T048 [P] [US5] Tests first for `Platform::resolve_editor` (and a pure `pick_editor(kb_editor, config_editor, visual, editor)`): order `KB_EDITOR`, config `editor`, `$VISUAL`, `$EDITOR`, `vi`; empty values count as unset at each step (FR-013, R3). Adapt the M3 editor tests in `platform/unix.rs` to pass the resolved string instead of reading `$VISUAL`/`$EDITOR` inside `editor_command`.
- [X] T049 [P] [US5] In `crates/kb/tests/tui_pty.rs`: Ctrl-E opens the editor named by `editor = "…"` in `config.toml` (a script that records its argument), not `$EDITOR`; with only `$EDITOR` set it still works (M3 behavior); Ctrl-E signal handling tests from M3 stay green.
- [X] T050 [P] [US5] In `crates/kb/tests/tui_pty.rs`: Ctrl-Y with a fake tool calls it (not OSC 52), stays open, the key bar shows `Copied via pbcopy` (or the tool name) for 2 s then reverts; over SSH it emits OSC 52 and the key bar shows `Sent via terminal (OSC 52)`; with the size limit lowered it shows the refusal text and records no use; a successful Ctrl-Y records a use (FR-025).

### Implementation

- [X] T051 [US5] Implement `Platform::resolve_editor`/`run_editor` in `unix.rs`, `linux.rs`, `macos.rs`, `fake.rs`: run the resolved command as `sh -c 'exec $1 "$2"' kb-editor "<editor>" "<path>"` (shell splits the editor string as before, the path stays one argument). Update the `kb edit` command and `tui::edit_entry` to use it. T048, T049 green.
- [X] T052 [US5] In `crates/kb/src/tui/mod.rs`: replace `copy_selected` with `clipboard::copy`; the TUI's `Action::CopyDone` carries the `CopyOutcome` (replace the M3 `CopyOutcome::{Copied, TooLarge, Failed}` in `app.rs`); key bar text from `outcome.message_short()`; record a use iff `sent()`. A `ponytail:` comment marks the blocking copy (worst case 2 s, plan Complexity Tracking). T050 green. The OSC 52 sequence is written with `Platform::write_tty` (research R7), not the TUI guard's writer; add a pty assertion in `tui_pty.rs` that after Ctrl-Y over SSH the screen contents are unchanged except the key bar message (no stray escape text).
- [X] T053 [US5] Remove the OS reads from the TUI: `tui/mod.rs` `TMUX` read and `tui/theme.rs` `COLORTERM` read now come from `Environment` (pass it in `Options`/`Theme::detect`). Remove the `ignore` from the boundary test (T021). It must be green: the FR-003 check passes.

**Checkpoint**: all five stories work; the boundary test is green.

---

## Phase 8: Polish and cross-cutting

- [X] T054 [P] SC-002 timing test in `crates/kb/tests/perf.rs`: `kb copy <id>` with a fake `pbcopy` on a generated 10,000-entry KB, release build, serialized, mtimes backdated; budget 150 ms end to end; also assert `kb config list` completes in 100 ms or less (CLI cold start, constitution VII); `kb doctor` is exempt from the budget (it opens the index and probes tools) but its time is measured and reported. Report the measured numbers.
- [X] T055 [P] Update `README.md`: the root-resolution list and the `config.yml` example (~lines 49–54) to `config.toml` and the macOS rule; the Enter/Ctrl-Y text in "Using the TUI"; new sections for `kb copy`, `kb show` (modes, `--print` removed), `kb config`, `kb doctor`, the SSH/tmux/OSC 52 notes and the Terminal.app caveat; replace the pointer to M3's `contracts/config.md` (~line 266) with this milestone's; status line M4.
- [X] T056 [P] Add a "Superseded by `specs/004-platform-clipboard/contracts/config.md`" note at the top of `specs/003-tui-core/contracts/config.md`; no other M3 spec file changes.
- [X] T057 [P] Re-run the verification set across the workspace (`Cargo.toml`) and fix findings: `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace` (all existing tests pass or were rewritten on purpose per T027, T032); `cargo +1.88 test --workspace`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`.
- [X] T058 [P] Confirm nothing touched the real home (`~/kb/.kb`, `~/.config/kb`, `~/Library/Application Support/kb`): `~/kb/.kb` unchanged, no `~/.config/kb` created by any test (check before and after a full `cargo test`).
- [X] T059 Walk through [quickstart.md](quickstart.md) steps 1–11 by hand on this Mac with the real `pbcopy`/`pbpaste` (use a temp `KB_ROOT`, `HOME` and `XDG_CONFIG_HOME` so nothing real is touched) and the `#[ignore]` round-trip test below; record results in the final report. Also time `kb copy <id>` with the real `pbcopy` on a generated 10,000-entry KB (release build, 5 runs) and report it against the 150 ms budget (SC-002), since T054 uses a fake tool.
- [X] T060 [P] Add the `#[ignore]`d local test `real_pbcopy_round_trip` in `crates/kb/tests/cli_copy.rs` (macOS only, `cfg(target_os = "macos")` is allowed in tests): `kb copy` then `pbpaste` returns the payload. Run it once by hand.
- [ ] T061 **Manual terminal matrix, results recorded in `specs/004-platform-clipboard/quickstart.md` (last section) (SC-001, needs a person; leave unticked until done)**: paste-verify copying in Terminal.app, iTerm2, GNOME Terminal on Wayland (`wl-copy`), Linux X11 (`xclip`, then `xsel`, and that the content survives kb exiting), SSH Mac→Linux with iTerm2 and with kitty (OSC 52), SSH inside tmux with `allow-passthrough on` and `off`, and GNU screen. This is the M4 equivalent of M3's T070; the GNU screen wrapping is the least certain part.
- [ ] T062 **CI gate (constitution VIII; needs your push to work)**: push `004-platform-clipboard` (the earlier SSH failure must be fixed first) and confirm `.github/workflows/ci.yml` is green on both macOS and Linux — fmt, clippy `-D warnings`, all tests, including the pty tests and the fake-tool tests that depend on `sh`, `perl`, `pgrep`, `kill`, `stty`. Record the run URL in the final report. The milestone is not done until this is green; any Linux-only failure is fixed here (the Linux platform rules have so far only run as pure-function tests on macOS).

---

## Dependencies and execution order

- **Phase 1** first. **Phase 2** blocks everything; inside it: T004→T005, T006 after T005; T007→T008; T009→T010; T010 before T011–T013, T018, T020; T014 (startup rewire) after T005, T006 and T010–T013, and T015 (hermetic harness) after T014 and before any pty or CLI test that presses Enter; T016, T017 are independent of the trait; T019 needs T017, T018 and T016; T021 any time after T008 (red until T053).
- **US1 (Phase 3)** needs all of Phase 2. **US2** needs US1's `PostPick` and the config load (T026–T028). **US3** needs T005 only (it can start after Phase 2, parallel to US1/US2, except that it shares `commands.rs`/`main.rs`). **US4** needs T018 and T019, and the existing M2 `Kb::status`. **US5** needs T019 (and T052 builds on the M3 Ctrl-Y code); T053 closes FR-003.
- **Polish** last; T061 (a person at several terminals) and T062 (a working push) cannot be done by me alone, are unticked until done, and the milestone is not complete without T062.
- Same-file serialization: `main.rs` (T026, T027, T037, T042, T047), `commands.rs` (T028, T037, T042, T047), `tui_pty.rs` (T022–T024, T033, T045, T049, T050: add them in order, one test each, no reformatting).

## Parallel opportunities

- Phase 2: T004 ∥ T007 ∥ T009 ∥ T016 ∥ T017 (different files), then T006 ∥ T008 ∥ T012 ∥ T013 ∥ T018 ∥ T020.
- US1 tests T022–T025 together; US2 tests T030–T033 together; US3 tests T038–T040 together; US4 tests T043–T045 together; US5 tests T048–T050 together.
- US3 and US4 implementations can proceed in parallel with US2 once Phase 2 is done.

## Implementation strategy

- **MVP**: Phase 1, Phase 2, US1 (T001–T029). That already gives "Enter copies" on every environment the chain supports.
- Then US2 (shell parity, constitution IV), US3 (config), US4 (doctor), US5 (cleanup and FR-003), Polish.
- Deliberately not built: a background thread for Ctrl-Y copies (2 s worst case, documented), real terminal automation for Terminal.app/iTerm2/Linux (T061), a way to detect that a terminal ignored OSC 52 (impossible; `doctor` explains).
- Keep the report honest at the end: Linux CI not yet run, GNU screen wrapping unverified, the 1.5 ratio calibrated on one fixture KB, and which tests were written after their code.

## Implementation notes (what differs from the text above)

- **Order**: T048/T049/T051 (editor resolution) were done before US4 because `kb doctor` needs
  `Platform::resolve_editor`. `pick_editor` takes three arguments (the `editor` setting already
  includes `KB_EDITOR`), not four.
- **T006/T014**: `RootInputs.config_file` was kept until T014 so the workspace compiled at every
  step; T014 removed it together with the old `read_config` and `Platform::config_file`.
- **Test-first** (what was actually seen failing before its code): the config loader (T004,
  19 of 19 red), the clipboard chain and `copy` (T017/T019, 17 of 20 red), the Enter-copies pty
  tests (T022–T024, 6 of 8 red; the header tests did not compile until the code existed), and the
  Ctrl-Y pty tests (T050, 4 of 4 red). The config edit tests (T038) were red only by accident: the
  1.88 run picked up the `unimplemented!()` stubs (13 failing) before the code was written.
  **Not run red first**: `cli_copy.rs`, the `cli_show.rs` rewrite and the ambiguous-query pty tests
  (T033), `cli_config.rs`, `cli_doctor.rs` and the doctor unit tests, the editor tests (T048/T049,
  `pick_editor`), and the pure modules `env.rs`, `osc52.rs`, `resolve.rs`, `show.rs`, `opener.rs`,
  `fake.rs` and the `unix.rs` helpers, whose tests were written with them. The constitution's
  red-green rule for `kb-core` was met for `config.rs` but not for `resolve.rs`.
- **T015**: the harness defaults are `KB_TUI_ENTER=print` and `KB_CLIPBOARD_METHOD=osc52` for pty
  tests (not `stdout`), plus guard scripts for every clipboard/opener program first on `PATH`; a
  test that reaches a guard without asking fails in `wait_exit`.
- **T019/T052**: `Place::Ui` drops the `stdout` method; `Place::Cli` keeps it.
- **T021/T053**: the boundary check ignores `#[cfg(test)]` modules; the two real violations (the
  `COLORTERM` read and the advice text naming it) moved into `platform/env.rs`.
- **T030/T034**: calibration recorded in `resolve.rs` and the plan re-check.
- **T058**: the check found `~/kb/.kb/index.db` and a `state.json` with one recorded use of
  `cw/example-lambda-errors` (an example entry), dated 07:58–07:59 on 2026-10-09, in the real
  `~/kb`, which holds only the `kb init` examples and had only `.gitignore` in `.kb/` before. They
  were made by a test run, not by hand (nothing else in this session ran `kb` against the real
  home), so both were deleted and `.kb/` holds only `.gitignore` again. **The cause was not
  found.** It did not recur under the hermetic harness (T015) in any test binary, the lib tests,
  the release perf run or the final run; whether it happened before T015 is not established.
- **T059**: steps 1 to 8 of the quickstart were run by hand with the real `pbcopy` (the clipboard
  was saved and restored); steps 9 to 11 need a terminal and are covered by the pty tests instead.
- **Two older tests were made race-free** while here: `concurrency.rs` writes files atomically
  (a reader could see half a file), and the `$VISUAL` test waits for content, not for the file.
