---
description: "Task list for M3: TUI Core — Search and Preview"
---

# Tasks: TUI Core — Search and Preview

**Input**: Design documents from `specs/003-tui-core/`
**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Required. Constitution VIII makes red-green-refactor mandatory for `kb-core`; the TUI
follows the same order. Every test task is written and seen failing (`cargo test` red) before its
implementation task starts. Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file
they test.

**Organization**: Tasks are grouped by user story so each story can be implemented and tested on its
own. US1 is a complete, shippable TUI (search, pick, scriptable `kb show`); US2 adds the real
preview; US3 the chips; US4 editing; US5 help. Cross-cutting edge cases (narrow terminals, resizing,
mouse, themes, signals) come after the stories because they touch the shared view code.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: The user story the task belongs to (US1 to US5)

## Path Conventions

Core code in `crates/kb-core/src/`, UI code in `crates/kb/src/tui/`, other binary code in
`crates/kb/src/`, binary tests in `crates/kb/tests/`, fixtures in `tests/fixtures/`, stored screens
in `tests/snapshots/tui/`. FR/SC numbers refer to [spec.md](spec.md).

## Ground rules for every task

- **Never run `kb` or a test against the default root.** Always pass `--root`/`KB_ROOT` pointing at a
  temp copy of a fixture; a bare `kb` would index the real `~/kb`.
- **Timing tests** must be serialized with a mutex and must backdate file mtimes by an hour
  (`File::set_modified`) so the "racy file" rule from M2 does not force re-reads.
- **Pseudo-terminal tests** set `HOME`, `KB_ROOT`, `TERM=xterm-256color`, `EDITOR` and `NO_COLOR`
  explicitly instead of inheriting them.
- Every change keeps `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`
  green; M1 and M2 tests must keep passing.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Dependencies, module skeletons, the extra fixture KB.

- [X] T001 (also added `crossterm` 0.29 with `use-dev-tty`, the same version ratatui uses, so the UI can read keys from `/dev/tty` when stdin is a pipe) Add to `[workspace.dependencies]` in `Cargo.toml`: `ratatui = { version = "0.30", default-features = false, features = ["crossterm"] }`, `syntect = { version = "5", default-features = false, features = ["parsing", "default-syntaxes", "regex-fancy", "yaml-load"] }`, `signal-hook = "0.3"`, `unicode-width = "0.2"`, `vt100 = "0.15"`, `portable-pty = "0.9"`; add the first four to `[dependencies]` and the last two to `[dev-dependencies]` of `crates/kb/Cargo.toml`. No other new dependency (research R14). Run `cargo +1.88 test --workspace --no-run` to confirm the 1.88 minimum still builds.
- [X] T002 [P] Declare the new modules as empty files so the crates build: in `crates/kb-core/src/lib.rs` add `config`, `entry_json`, `payload`; in `crates/kb/src/main.rs` add `mod commands; mod tui;` with `crates/kb/src/tui/mod.rs` declaring `app`, `highlight`, `input`, `keys`, `layout`, `markdown`, `preview`, `terminal`, `theme`, `view`, `worker` (each an empty file).
- [X] T003 [P] Create the extra fixture KB `tests/fixtures/tui/` (lint-clean under `kb lint --strict`): `sumo/long-query.md` (a 60-line payload and Markdown notes using bold, italic, inline code, a link, a bullet list and a numbered list), `cmd/deploy-with-secret.md` (variables `env` enum with default, `region` text with default and description, `token` of kind `secret`; payload uses `{{env}}`, `{{region}}` and one escaped `\{{literal}}`), `prompt/cjk-and-emoji.md` whose title contains CJK and emoji and is longer than 80 columns, `link/long-url.md` (a 150-character URL), `code/with-symbol.md` (repo, path, line, `end_line`, ref, symbol), `sql/multi-dialect.md` (dialect and database set, payload with comments, strings, numbers), `cw/with-fields.md` (log groups, region, range). Add a `tui/` section to `tests/fixtures/README.md` and a `search_fixture_kb_is_lint_clean`-style test `tui_fixture_kb_is_lint_clean` in `tests/lint_fixtures.rs`.

**Checkpoint**: `cargo build --workspace` passes with empty modules; the fixture KB lints clean.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The `kb-core` additions ([contracts/core-api.md](contracts/core-api.md)), the key map,
line editor, layout, theme and platform pieces that every story uses.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

### Tests first (must fail before the matching implementation)

- [X] T004 [P] Write tests in `crates/kb-core/src/config.rs` for `read_config`: a missing file or `None` gives `Config { theme: Auto, mouse: false }`; `theme: dark|light|auto` and `mouse: true|false` parse; unknown keys are ignored; `theme: blue` is a `ConfigError` whose message names `theme` and the file and lists `auto, dark or light`; `mouse: maybe` is an error naming `mouse`; the file that also has `root:` still parses; a YAML syntax error is a `ConfigError`.
- [X] T005 [P] Write tests in `crates/kb-core/src/payload.rs` for `payload_pieces` using fixture text: `echo {{env}} \{{literal}} {{ not a var }}` yields `Text("echo ")`, `Var("env")`, `Text(" {{literal}} {{ not a var }}")` (escaped and invalid braces stay literal, exactly as M1's `scan_refs` classifies them); joining the pieces back with `{{name}}` for variables equals the payload text for every fixture entry in `tests/fixtures/valid`, `tests/fixtures/search` and `tests/fixtures/tui`; an entry with no payload (link, code) yields an empty list.
- [X] T006 [P] Write tests in `crates/kb-core/src/entry_json.rs` for `entry_json`, `PrintMode::parse` and `pick_output`: `PrintMode::parse("payload"|"id"|"path"|"json")` and `None` for anything else; `pick_output` for a `sql` entry is the payload text with no trailing newline added (a payload ending in a newline keeps exactly that newline); for `link` it is the URL; for `code` it is `code.path:code.line`; `id` is the ID; `path` is the absolute path passed in; `json` is `entry_json` serialized plus one `\n`; every key of [contracts/entry.schema.json](contracts/entry.schema.json) is present with the documented type for each fixture type (assert required keys and types by hand, no schema crate); `secret` variables never carry a `default`; an entry with a missing payload block gives `PickError::NoPayload`.
- [X] T007 [P] Write tests in `crates/kb-core/src/search/tests.rs` (append) for the additions: `refresh` returns `indexed`/`skipped` counts and `query` after `refresh` equals `search` for 10 queries from `queries.tsv`; `query` does not walk (count reads through the test reader: zero after the first refresh); `type_counts` for `latency` equals the number of candidates per type and is identical with and without `type:cw` while `total` and `results` change; the sum of `type_counts` for the empty query equals the number of indexed entries; `SearchResult.use_count` is 0 then 2 after two `record_use` calls; `updated` is `Some("2026-…")` for a fixture entry that has an `updated:` value and `None` for one that doesn't (add `updated: 2026-09-30` to `sumo/5xx-by-authorizer` in `tests/fixtures/search`, and keep every M2 expectation green); `modified` is the file's mtime in unix seconds; `lint_entry` returns no findings for a valid entry, an error finding after the test writes `type: nope`, and `KbError::NotFound` for a missing ID; a version-1 index file (set `user_version` to 1) is rebuilt with `Notice::Rebuilt`; `kb index status` reports `index_version` 2 (update the assertion `"Version:    1"` in `crates/kb/tests/cli_index.rs` to `2`).

### Implementation

- [X] T008 [P] Implement `crates/kb-core/src/config.rs` per [contracts/config.md](contracts/config.md): `Config`, `ThemeChoice`, `ConfigError` (with `Display` naming file and key), `read_config(Option<&Path>)` using `crate::yaml::parse` like `root.rs` does. Depends on T004.
- [X] T009 [P] Implement `crates/kb-core/src/payload.rs`: `PayloadPiece { Text(String), Var(String) }` and `payload_pieces(&Entry)`, scanning the raw payload text (`entry.source().text()[payload.raw_span]`) with `crate::vars::scan_refs`, unescaping `\{{` in the text pieces with `crate::body::unescape`, and keeping `NotAVariable` braces as text. Depends on T005.
- [X] T010 Implement `crates/kb-core/src/entry_json.rs`: `PrintMode` (+ `parse`), `PickError`, `entry_json` (keys per `entry.schema.json`: `id`, `path`, `file`, `type`, `title`, `tags`, `created`, `updated`, `vars` with `name`, `kind`, `declared`, `default` omitted for secrets, `options`, `description`, `env`, `payload` (null for link and code), `notes`, `fields` as an object holding the type-specific values), `pick_output`, and `Kb::pick(&self, id, mode)` in `crates/kb-core/src/search.rs`. Depends on T006.
- [X] T011 Bump the index to version 2: in `crates/kb-core/src/index/schema.rs` set `INDEX_VERSION = 2` and add `updated TEXT` to `entries`; in `index/doc.rs` carry `entry.updated()` in `Doc`; in `index/sync.rs` `insert_doc` write it. Update the M2 unit tests that assert version 1 (`index/mod.rs` tests use the constant; fix any literal `1`). Depends on T007.
- [X] T012 Implement the `search.rs` additions per [contracts/core-api.md](contracts/core-api.md): keep the synced `Index` and its facts in the `Kb` handle (new private field), add `Kb::refresh() -> Refresh` (walk + sync + state updates + `Notice`s, the work `search` does today before querying) and `Kb::query()` (queries the held index, calls `refresh` first if none; still drops results whose files are gone; one rebuild-and-retry on corruption), make `search` = `refresh` + `query`; move the `types` filter out of `filters()` into a Rust pass after candidates are gathered, filling `SearchOutcome.type_counts: [usize; 7]` (index = `EntryType::ALL`), `indexed`, `skipped`; add `use_count`, `updated`, `modified` to `SearchResult` (join `files.mtime_ns / 1e9`, `entries.updated`, and the loaded `State`); add `Kb::lint_entry(id)` using `crate::lint::check_file`. All 196 M2 core tests and the CLI tests stay green. Depends on T007, T011.

### Tests for the UI foundations (must fail first)

- [X] T013 [P] Write tests in `crates/kb/src/tui/keys.rs`: the table contains every row of [contracts/keys.md](contracts/keys.md) (assert each key/mode pair resolves to the documented action, including `Ctrl-J`, `Ctrl-K`, `Ctrl-N`, `Ctrl-P`, `F1`, `Alt-P`, `Shift-PgUp`); `has_duplicates(&[Binding])` returns true for a hand-made table with the same key twice in one mode and false for the real table (the real check is also a `const _: () = assert!(!has_duplicates(BINDINGS));` so a duplicate fails the build, FR-011); the same key may appear in different modes; `key_bar(state_kind, width)` returns exactly the rows of the key-bar table in keys.md for each state ("Results shown", "text empty", "No results", "Empty KB", "FullPreview", "Help overlay", "Too small") and drops the lowest-priority items until the text fits a given width (test at widths 120, 80 and 40); `help_lines()` lists every binding once, grouped, and includes the sentence about terminals that swallow Shift-PgUp/PgDn and Ctrl-Home/Ctrl-End.
- [X] T014 [P] Write tests in `crates/kb/src/tui/input.rs` for `LineEditor` and chips: insert/backspace/delete/left/right/home/end keep `cursor` (a character index) on a character boundary, tested with `"héllo"`, a CJK string, an emoji and a combining mark; Ctrl-W deletes back over spaces then the previous word (`"foo bar  "` → `"foo "`), Ctrl-U clears, Ctrl-A is home; typing a space after `tag:prod`, `-tag:stage`, `type:sql`, `var:region`, `repo:acme/api` or `in:title` turns the token into the matching `Chip` and removes it from the text, while `type:nonsense `, `tag: ` and a token in the middle of a quoted phrase stay text; `type:sql type:cw ` produces two `Chip::Type`; Backspace on empty text removes the last chip, on non-empty text deletes a character; `to_query_string(chips, text)` for `[Type(Cw), Tag("prod")]` + `latency` equals `type:cw tag:prod latency` and `Query::parse` of it equals `Query::from_parts("latency", ["cw"], ["prod"], [], None)` (M2's equality); Esc-clear empties both.
- [X] T015 [P] Write tests in `crates/kb/src/tui/layout.rs`: `layout(Rect)` gives header, search, chips and key bar one row each and the body `height − 4`; width 120 → list 40% (≥ 34 columns) and preview the rest; width 99 → list only with `preview: None`; width 60 and height 15 are valid, width 59 or height 14 give `TooSmall`; `truncate("…", width)` uses display width: a CJK string of width 10 truncated to 7 ends with `…` and never splits a wide character (replaces a half-fitting one with a space), an emoji at the cut is dropped whole, text that fits is unchanged; `truncate_middle` shortens a long root path from the middle keeping the last component.
- [X] T016 [P] Write tests in `crates/kb/src/tui/theme.rs`: `Theme::resolve(choice, env)` where `env` is a struct of `no_color`, `colorfgbg`, `colorterm`: `NO_COLOR=1` (any non-empty) gives `NoColor` regardless of `theme`; `NO_COLOR=` (empty) does not; `Auto` with `COLORFGBG="0;15"` gives `Light`, `"15;0"` gives `Dark`, unset gives `Dark`; `Dark`/`Light` ignore the variable; the seven type colors equal the table in [contracts/screen.md](contracts/screen.md#type-colors-and-classes) in every theme except `NoColor`, which has none; `class_style(Class::Keyword)` differs between dark and light; `NoColor` styles use only bold/underline/reverse/italic modifiers and no fg/bg; `colorterm=truecolor` maps palette entries to RGB, otherwise to indexes.
- [X] T017 [P] Write tests in `crates/kb/src/platform/mod.rs` (unix-only `#[cfg(unix)]`): `has_terminal()` is false when both stdin is `/dev/null` and `/dev/tty` can't be opened (run the check in a child process started with `setsid` and null stdio) and true under a pseudo-terminal; `run_editor(path, tty)` runs `$VISUAL` first, then `$EDITOR`, then `vi` (shadow `PATH` with a temp dir holding a fake `vi` script that writes a marker), supports an editor with arguments (`EDITOR='sed -i.bak s/a/b/'`), passes a path containing spaces intact, and returns the editor's exit status.

### Implementation

- [X] T018 [P] Implement `crates/kb/src/tui/keys.rs`: `Key { code: Code, mods: u8 }` with a `const fn` equality, the `Mode` and `Action` enums, `const BINDINGS: &[Binding]` with every row of keys.md (group, label, optional bar priority), `const fn has_duplicates`, the `const _` assertion, `resolve(key_event, mode) -> Option<Action>` converting crossterm events, `key_bar(kind, width)` and `help_lines()`. Depends on T013.
- [X] T019 [P] Implement `crates/kb/src/tui/input.rs`: `LineEditor`, `Chip`, chip parsing on space, `to_query_string`, `clear_all`. Depends on T014.
- [X] T020 [P] Implement `crates/kb/src/tui/layout.rs`: `Layout`, `layout(Rect)`, `truncate`, `truncate_middle` using `unicode-width`. Depends on T015.
- [X] T021 [P] Implement `crates/kb/src/tui/theme.rs`: `Theme`, `Class`, `ThemeEnv`, `Theme::resolve`, the fixed type palette and class palettes from screen.md. Depends on T016.
- [X] T022 Implement the platform pieces: add `fn has_terminal(&self) -> bool`, `fn open_tty(&self) -> io::Result<File>` and `fn run_editor(&self, path: &Path, tty: &File) -> io::Result<ExitStatus>` to the `Platform` trait in `crates/kb/src/platform/mod.rs` with the implementations in `linux.rs` and `macos.rs` (both delegate to a private shared unix helper module `platform/unix.rs` so there is one implementation per OS file but no copy-paste): `has_terminal` = `stdin().is_terminal() || File::open("/dev/tty").is_ok()`; `open_tty` opens `/dev/tty` read-write; `run_editor` runs `sh -c 'exec ${VISUAL:-${EDITOR:-vi}} "$1"' kb-editor <path>` with stdin, stdout and stderr all set to duplicates of the tty handle. Depends on T017.

**Checkpoint**: `cargo test --workspace` is green (all M1, M2 and the new foundation tests); `kb index status` reports version 2.

---

## Phase 3: User Story 1 - Search as you type and pick an entry (Priority: P1) 🎯 MVP

**Goal**: `kb` opens a full-screen UI, updates the list on every keystroke from M2 search, and
Enter prints the pick to stdout. `kb show` is the scriptable twin.

**Independent Test**: the pseudo-terminal tests below against a copy of `tests/fixtures/search`.

### Tests for User Story 1 ⚠️ (write first, see them fail)

- [X] T023 [P] [US1] Write tests in `crates/kb/src/tui/app.rs` for `update` (pure, `now` passed in): typing a character sets text, increments `generation`, resets `selected` to 0 and returns exactly one `Effect::Search { query, limit: 60 }` whose `query` equals `to_query_string`; two keys applied before the effects are drained still produce the latest text in the last effect only when the loop coalesces (test `update` once per key and assert each effect carries its own generation); `Action::Results(g, page)` with `g` older than `state.generation` is ignored (state unchanged, no effect); with the current generation it replaces `results`, `total`, `type_counts`, `indexed`, `skipped`, clears `status.refreshing`, keeps `selected` by entry ID when the previous ID is still present, and moves to the nearest remaining index when it is not (US edge case "a deleted selected entry moves the selection to the nearest remaining result"); the first result of a new query is selected and a `LoadPreview` effect is returned for it, none when the same ID stays selected; Down/Up/Ctrl-N/Ctrl-P/Ctrl-J/Ctrl-K/PgDn/PgUp/Ctrl-End/Ctrl-Home move `selected` within bounds (`selected < results.len()` invariant) and PgDn moves by body-height rows; moving within 10 of the end of `results` when `total > results.len()` returns `Search` with `limit = results.len() * 2`, and Ctrl-End returns `Search` with `limit = 0` then selects the last result when the page arrives; Enter with a selection returns `[RecordUse(id), Pick(print_mode)]`; Enter with no results returns no effect and sets the message `Nothing to pick` for 2 s (`message` expires on `Tick` after 2 s); Esc with text or chips clears both and returns a `Search`; Esc with both empty returns `Quit(130)`; Ctrl-C returns `Quit(130)` in every mode; `Resize` keeps text, chips, selected ID and clamps `list_offset` and `preview_scroll`; `Action::Refreshed` clears `refreshing` and records counts; `Tick` after 1 s since `last_refresh` returns `Refresh` (and after 4× a refresh that took 300 ms, not before); the empty-KB flag is set when `indexed + skipped == 0`; `list_offset` keeps the selection visible with 2-line rows.
- [X] T024 [P] [US1] Write tests in `crates/kb/src/tui/worker.rs`: spawn the worker on a copy of `tests/fixtures/search`; send `Refresh` then `Search { generation: 1, query: "5xx auth", limit: 60 }` and receive `Refreshed` then `Results` with generation 1 whose first result is `sumo/5xx-by-authorizer`, `type_counts` non-zero for sumo, `indexed == 32`; send three `Search` requests (generations 2, 3, 4) while the worker is blocked (hold it with a gate in the test reader or send them before it starts) and assert only generation 4 is answered; a `Search` with a query that matches nothing returns `Results` with an empty page, not `Failed`; `Shutdown` ends the thread and the join returns.
- [X] T025 [P] [US1] Write tests in `crates/kb/src/tui/terminal.rs`: `osc52("hello")` equals `\x1b]52;c;aGVsbG8=\x07`; base64 matches the RFC 4648 test vectors (`""`, `"f"`, `"fo"`, `"foo"`, `"foob"`, `"fooba"`, `"foobar"`); inside tmux (`tmux = true`) the sequence is wrapped as `\x1bPtmux;` + the sequence with every `\x1b` doubled + `\x1b\\`; `copy_sequence(payload, tmux)` returns `Err(TooLarge)` above 74,000 bytes; `restore(&mut Vec<u8>, mouse)` writes leave-alternate-screen, show-cursor (and disable-mouse-capture only when `mouse`) and calling it twice is harmless; `exit_code_for_signal(SIGINT)` = 130, `SIGTERM` = 143, `SIGHUP` = 129.
- [X] T026 [P] [US1] (restoration is checked by running `stty -a` in the same pty after kb exits and reading its `icanon`/`echo` flags, not by reading termios from the master, which would need the `nix` crate) Write the pseudo-terminal harness `crates/kb/tests/support/pty.rs` (helpers: `Session::spawn(args, env, kb_root)` using `portable_pty` at a given size, a reader thread feeding a `vt100::Parser`, `wait_for(text, timeout)`, `screen_text()`, `send(bytes)`, `send_key(Key)` for Enter, Esc, Ctrl-C, Tab, arrows, Ctrl-letters, `resize(cols, rows)`, `wait_exit() -> (code, output_file)`, `termios()` reading ICANON/ECHO from the master) and the tests in `crates/kb/tests/tui_pty.rs` for the US1 scenarios against a copy of `tests/fixtures/search` (`HOME` set to a temp dir, `KB_ROOT` to the copy, `TERM=xterm-256color`): (1) `kb` shows the header text `entries`, a `›` search line, chips row `all`, the 20 first list entries, a preview whose title matches the first entry, and the key bar `Enter pick`; (2) typing `5xx auth` leaves `sumo/5xx-by-authorizer` selected first with its preview, and the counter shows `N / 32`; (3) Enter with stdout redirected to a file (`sh -c 'kb … > out'` so stdin is the tty and stdout a file) exits 0 and `out` contains exactly the Sumo payload with no trailing newline, and `.kb/state.json` has `count` 1 for that entry; (4) `--print id`, `--print path`, `--print json` write the documented outputs; (5) `kb 5xx auth` opens with the text pre-filled; (6) Esc with text clears it and keeps the UI open, Esc again exits 130 with an empty `out`; (7) `kb | cat`-style piping: stdout a pipe, nothing but the pick arrives, none of it escape codes; (8) after exit the vt100 screen shows the alternate screen is off and the cursor visible.
- [X] T027 [P] [US1] Write tests in `crates/kb/tests/cli_show.rs` (and the `kb` CLI parsing for the TUI) with `assert_cmd` on a copy of `tests/fixtures/search`: `kb show sumo/5xx-by-authorizer` prints the payload with no trailing newline beyond the payload's own and exit 0; `--print id|path|json` match [contracts/cli.md](contracts/cli.md#pick-output) (path is absolute and exists, json has the schema's required keys and `payload` equal to the default output); `--json` equals `--print json`; `kb show link/payments-runbook` prints the URL; `kb show code/request-handler` prints `src/handler.rs:10`; unknown ID → exit 2 with `kb: no entry with ID "x"` on stderr and `{"error": …}` on stdout with `--json`; `kb show ../x` → exit 2; `kb show` never creates `.kb/state.json`; `kb` with no terminal (spawned with `setsid` and null stdio) prints the usage text to stderr and exits 2 within one second; `kb tui --help` and `kb --help` list `--print`.

### Implementation for User Story 1

- [X] T028 [US1] Implement `crates/kb/src/tui/app.rs`: `AppState` (every field of [data-model.md](data-model.md#appstate)), `Action`, `Effect`, `update(&mut AppState, Action, Instant) -> Vec<Effect>` covering the behaviors in T023 (generation counter, selection by ID, paging rules, message expiry, refresh timer with back-off, quit codes), and `AppState::new(options, size, initial_query)`. Preview-related and chip-related actions are stubs that later phases fill. Depends on T018, T019, T023.
- [X] T029 [P] [US1] (partly: the rule "a fatal index error twice in a row ends the thread" was not built; a failed refresh shows a notice and the next timer tick retries) Implement `crates/kb/src/tui/worker.rs`: the request and reply enums of the data model, `spawn(root) -> (Sender<Request>, Receiver<Reply>, JoinHandle)`; the thread builds its own `Kb::open(root)`, loops on `recv`, drains the queue keeping the newest `Search`, newest `Preview` and a single `Refresh`, answers `Refresh` with `Kb::refresh`, `Search` with `Kb::query(&Query::parse(q) with limit, SystemTime::now())` and a `Page` built from `SearchOutcome`, turns errors into `Failed`; a fatal index error twice in a row ends the thread with a final `Failed`. Preview requests are answered in Phase 4. Depends on T012, T024.
- [X] T030 [P] [US1] Implement `crates/kb/src/tui/terminal.rs`: base64 and `copy_sequence` (OSC 52, tmux wrapper, size limit), `TerminalGuard` (enter: raw mode, alternate screen, hide cursor, mouse capture only when enabled; `Drop` and `restore()` undo exactly that), the panic hook (restores, then calls the previous hook; in debug builds `KB_TUI_PANIC_AFTER_FIRST_FRAME` makes the loop panic after the first draw, used by the restore test), the signal flags with `signal_hook::flag::register` for SIGINT, SIGTERM and SIGHUP, and `exit_code_for_signal`. Draws go to the `File` from `Platform::open_tty`. Depends on T022, T025.
- [X] T031 [US1] Implement the US1 part of `crates/kb/src/tui/view.rs`: `draw(frame_buffer, &AppState, &Theme)` for the header (root with `~`, `N entries`, `⚠ M with errors`, `refreshing…`), search line (`› `, text, reverse-video cursor cell, right-aligned `shown / total`), chips row (`all N` plus the seven type chips with counts, dimmed when 0), the bordered list with two-line rows (type badge padded to 6 columns in the type color, title with match highlighting from `matches` of fields `Title`, tags line with `#tag`s mapped from the `Tags` ranges, right-aligned `×N` use count omitted when 0, selection background on both lines, `list_offset` windowing) and the key bar from `keys::key_bar`; a placeholder box for the preview (title and path only); empty-KB and no-results texts per [contracts/screen.md](contracts/screen.md#empty-and-error-states) (the preview-pane tips for no results are implemented in Phase 4). Only visible rows are built. Depends on T020, T021, T028.
- [X] T032 [US1] Implement `crates/kb/src/tui/mod.rs` `run(options) -> ExitCode`: set up the guard and signal flags, draw the first frame immediately (header with the root and `refreshing…`, empty list), spawn the worker and send `Refresh` then `Search` for the initial query, then loop: wait up to 50 ms for terminal events with `crossterm::event::poll` (events come from stdin, or `/dev/tty` when stdin is not a terminal), read every pending event, apply them with `update`, check the signal flags, execute the returned effects (`Search`/`LoadPreview`/`Refresh` → worker, `RecordUse` → the UI-thread `Kb`, `Pick` → remember mode and stop, `Quit(code)` → stop), drain worker replies into actions, redraw only when something changed, and on exit restore the terminal first and only then write the pick output (`Kb::pick`) to stdout without adding anything; record-use errors are a warning on stderr; internal errors restore the terminal, print `kb: <message>` and exit 2. Depends on T029, T030, T031.
- [X] T033 [US1] Wire the command line in `crates/kb/src/main.rs`: make the subcommand optional (`#[command(args_conflicts_with_subcommands = true)]`) with top-level `query: Vec<String>` and `--print <mode>` (value parser from `PrintMode::parse`, default `payload`), add `Command::Tui(TuiArgs)` with the same arguments, apply the existing `-tag:` rewrite, call `read_config(platform.config_file())` for `theme`/`mouse` (a `ConfigError` is exit 2 before touching the terminal), check `platform.has_terminal()` first and otherwise print the help to stderr and exit 2, build `Options` and call `tui::run`. Depends on T022, T032.
- [X] T034 [P] [US1] Implement `kb show <id> [--print mode] [--json]` in `crates/kb/src/commands.rs` (`run_show`), wired in `main.rs`: `Kb::open(root)?.pick(id, mode)`, print the string with `print!` (no newline added), errors as `CliError::new("io", …)` → exit 2, `--json` as a synonym for `--print json`. Depends on T010, T027.
- [X] T035 [US1] Make T023 to T027 pass end to end; fix whatever they expose in layout, worker coalescing or terminal restore. Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`. Depends on T028 to T034.

**Checkpoint**: US1 is shippable: `kb` is a working search-as-you-type picker and `kb | pbcopy` works.

---

## Phase 4: User Story 2 - Read an entry fully before using it (Priority: P1)

**Goal**: the preview pane shows the full entry: highlighted payload with line numbers and marked
variables, variables table, fields, notes as Markdown, link and code details, scrolling.

**Independent Test**: the snapshot tests below for each fixture type.

### Tests for User Story 2 ⚠️

- [X] T036 [P] [US2] Write tests in `crates/kb/src/tui/highlight.rs`: `highlight(lang, pieces)` returns lines of `Span { text, class }` where concatenating the span texts of a line reproduces the line (with `{{name}}` for variable pieces); SQL `select id, 'x' from t where n = 42 -- c` marks `select`/`from`/`where` as `Keyword`, `'x'` as `String`, `42` as `Number`, `-- c` as `Comment`; bash `kubectl -n "$NS" logs # c` marks the string and the comment; Sumo `_sourceCategory=prod/api | where status >= 500 | count by host` marks `where`/`count`/`by` as `Keyword`, `_sourceCategory` as `Field`, `500` as `Number`, `|` as `Operator`; CloudWatch Insights `fields @timestamp, @message | filter @message like /ERROR/ | limit 20` marks `fields`/`filter`/`limit`/`like` as `Keyword`, `@timestamp` as `Field`, `/ERROR/` as `String`, `20` as `Number`; a `Var` piece becomes a span of class `Variable` with text `{{name}}` even inside a string or a comment, and `\{{x}}` (a Text piece) is never `Variable`; unknown language and `prompt` give all-`Plain`; multi-line input keeps parser state across lines (a SQL `/* … */` spanning two lines is `Comment` on both); with `NoColor` the highlighter is not invoked and variables are still `Variable`; cold start: parsing 15 lines of each language once on a fresh process completes (no time assertion) and `warm_up()` is idempotent.
- [X] T037 [P] [US2] Write tests in `crates/kb/src/tui/markdown.rs`: `render_notes("**b** *i* `c` [t](http://u)\n\n- one\n- two\n\n1. a\n2. b\n\n> q\n\n```\ncode\n```")` returns blocks `Paragraph` with spans of classes `Bold`, `Italic`, `Code`, `Link` (text `t`, the URL not in the text), a bullet `List { ordered: false }` of two items, an ordered list, a `Quote`, a `Code` block; headings become `Heading`; tables, images and HTML are kept as their plain text; `wrap(blocks, width)` produces lines no wider than `width` display columns (tested with width 20 and CJK text), list items get a hanging indent (`• ` / `1. `), and empty notes give no lines.
- [X] T038 [P] [US2] Write tests in `crates/kb/src/tui/preview.rs` for `build_preview(entry, mtime, use_count)` on the fixture entries: a `sumo` entry has `payload` lines, `meta.entry_type == Sumo`, tags, `edited` equal to its `updated:` date when present and otherwise the file's date as `YYYY-MM-DD`; `cmd/deploy-with-secret` has three `VarRow`s with kind, options, default and description, and the `secret` row has `secret (prompted)` and no default; type fields are listed (`dialect`, `database` for sql; `log_groups`, `region`, `range` for cw; `shell`, `cwd` for cmd); `link/long-url` has `url` set and no payload; `code/with-symbol` has `code` with repo, path, line, end line, ref and symbol; the model is theme-independent (no colors); `PreviewCache` returns the cached model for the same `(id, mtime)` and rebuilds when `mtime` changes; it keeps at most 64 entries and drops the oldest.
- [X] T039 [P] [US2] Write the snapshot harness `crates/kb/tests/tui_snapshots.rs` plus `crates/kb/tests/support/snapshot.rs`: `render_case(state, width, height, theme) -> String` draws into a ratatui `Buffer` and serializes it as trimmed text lines, then a `--- styles ---` section with runs `line:col..col fg bg mods` for cells whose style differs from default; `assert_snapshot(name, actual)` compares with `tests/snapshots/tui/<name>.txt`, writes it when `KB_UPDATE_SNAPSHOTS=1`, and fails with a unified diff otherwise; states are built from a real worker reply over the fixture KBs (`tests/fixtures/search` and `tests/fixtures/tui`) so the data is real. Add the US2 cases: one per entry type (sql, sumo, cw, cmd, prompt, link, code) at 120×40 dark; `sumo/long-query` scrolled to the middle; `cmd/deploy-with-secret` (variables marked and listed); notes rendering; `link/long-url` wrapping; `code/with-symbol`. Review each stored snapshot by eye before committing it. (Reviewed by eye so far: about a dozen of the 91, namely the sumo, cmd, code, link, cjk, chips, help, narrow and too-small screens at one theme each; the rest are produced by the same code and have only been checked by the width and content assertions in the test.)
- [X] T040 [P] [US2] Add `app.rs` tests for preview behavior: selecting another result sets `preview = None`, `preview_scroll = 0` and returns `LoadPreview` with a new generation; `PreviewReady` with an old generation is ignored; Shift-Down/Up scroll by one line and Shift-PgDn/PgUp by the pane height, clamped to `[0, preview_lines − pane_height]`, without changing `selected`; resizing clamps the scroll.

### Implementation for User Story 2

- [X] T041 [P] [US2] Create the two grammars `crates/kb/src/tui/syntaxes/sumo.sublime-syntax` (scope `source.sumo`: keywords `where|parse|json|count|sum|avg|min|max|by|sort|top|limit|timeslice|fields|if|as|nodrop|lookup|join|extract|format|formatDate|toLowerCase|concat`, case-insensitive; `_sourceCategory|_source|_sourceHost|_index|_collector` as field names; strings in double quotes with `\"` escapes; numbers; `|` as operator; `//` comments) and `cwi.sublime-syntax` (scope `source.cwi`: `fields|filter|stats|sort|limit|parse|display|dedup|like|in|as|by|and|or|not|desc|asc`, built-in functions `count|sum|avg|min|max|pct|bin|ispresent|strlen|concat|ts`, `@timestamp|@message|@logStream|@log|@ingestionTime` and `@`-prefixed names as fields, strings in single or double quotes, `/regex/` literals as strings, numbers, `|` operator, `#` comments), each starting with `%YAML 1.2\n---`. Depends on T036.
- [X] T042 [P] [US2] Implement `crates/kb/src/tui/highlight.rs`: `Lang` from the entry type and `shell` field (sql → "SQL"; cmd with `bash|sh|zsh|fish` → "Bourne Again Shell (bash)"; sumo and cw → the embedded grammars loaded with `SyntaxDefinition::load_from_str` into a `SyntaxSetBuilder`; others plain), a process-wide `OnceLock<SyntaxSets>` (default pack plus custom), the scope → `Class` mapping (`keyword*` → Keyword, `string*` → String, `constant.numeric*` → Number, `comment*` → Comment, `keyword.operator*`/`punctuation.separator*` → Operator, `support.function*`/`entity.name.function*` → Function, `variable.other*`/`variable.parameter*` in the custom grammars → Field, `storage.type*`/`support.type*` → Type, else Plain), variable handling per [research R4](research.md#r4-syntax-highlighting-fr-007-us2-sc-004) (replace each `Var` piece with a same-width placeholder of private-use characters before parsing, restore afterwards as a `Variable` span even inside strings and comments), `highlight(lang, pieces, color: bool)` and `warm_up()`. Depends on T036, T041.
- [X] T043 [P] [US2] Implement `crates/kb/src/tui/markdown.rs`: `render_notes(&str) -> Vec<Block>` with `pulldown_cmark::Parser` (default features are off in the workspace; no extra options) mapping events to the blocks of the data model, and `wrap(&[Block], width) -> Vec<Line>` using `unicode-width`. Depends on T037.
- [X] T044 [US2] Implement `crates/kb/src/tui/preview.rs`: `PreviewModel`, `Meta`, `VarRow`, `CodeLine`, `build_preview` (calls `payload_pieces`, `highlight`, `render_notes`; `edited` from `updated` or the mtime formatted as `YYYY-MM-DD` with the civil-date arithmetic already in `output.rs::utc`, moved to a shared function), `PreviewCache`. Depends on T038, T042, T043.
- [X] T045 [US2] Answer `Preview` requests in `crates/kb/src/tui/worker.rs`: `Kb::entry(id)` + the result's `modified`/`use_count` (the worker keeps the last page's metadata by ID, or reads `Kb::usage`) → `build_preview` through the cache, reply `Preview { generation, model }`; after the first `Results` the worker calls `highlight::warm_up()` once in the idle time. `Failed` for an entry that can't be read (the UI keeps the previous preview and shows the notice). Depends on T044.
- [X] T046 [US2] Implement the preview pane in `crates/kb/src/tui/view.rs` per [contracts/screen.md](contracts/screen.md#preview-fr-007): title, `link` URL line (wrapped, never truncated), metadata line (`type · tags · used N times · edited DATE`), muted path, code fields list, `payload` rule and lines with right-aligned muted line numbers and class styles from the theme (lines longer than the pane cut with `›`), `variables` rows, `fields`, `notes` rule and wrapped blocks, `↑ more`/`↓ more` indicators, the previous preview kept with a `…` title while the next loads, and the no-results tips. Add the scroll actions to `app.rs` (T040). Depends on T040, T044, T045.
- [X] T047 [US2] Create and review the US2 snapshots with `KB_UPDATE_SNAPSHOTS=1`, then make T036 to T040 pass without the update flag. Depends on T046.

**Checkpoint**: US1 and US2 together: a complete read-and-pick TUI.

---

## Phase 5: User Story 3 - Narrow by type and tag without typing filters (Priority: P2)

**Goal**: chips, Tab cycling, per-type counts.

**Independent Test**: app tests plus pty tests that type filters and press Tab.

### Tests for User Story 3 ⚠️

- [X] T048 [P] [US3] Extend `crates/kb/src/tui/app.rs` tests: Tab on `all` activates the first type with `type:sql` chip and returns a `Search` whose query string contains `type:sql`; Shift-Tab from `all` goes to `code`; Tab cycles `all → sql → sumo → cw → cmd → prompt → link → code → all` (order of `EntryType::ALL`); Tab replaces typed type chips (`type:sql type:cw ` then Tab → one chip, the first type after the typed ones per spec's "first type after them" rule is implemented as: next after the last typed type); typing `type:sumo ` makes only `sumo` active; typing `tag:prod ` adds `Chip::Tag("prod")` and returns a `Search`; Backspace on empty text removes the last chip and searches again; `-tag:stage ` becomes `NotTag`; the query string always equals what `kb search` would be given (`Query::parse` equality with `from_parts`); `type_counts` from a `Page` are stored and are unaffected by the active type filter; a chip row with zero-count types marks them dim.
- [X] T049 [P] [US3] Add snapshot cases to `crates/kb/tests/tui_snapshots.rs`: the chips row with counts for query `latency` (some types dimmed), with the `cw` type active, with `tag:prod` and `-tag:stage` chips, and with more chips than fit (right ones dropped, types kept). Add pty tests to `tui_pty.rs`: Tab twice shows `type:` text-free chips and the list only has that type; typing `tag:prod ` shows a `tag:prod ×` chip and the list shrinks; Backspace removes the chip and the list grows; `"role cache"` and `type:cw latency` give the same lists as `kb search`.

### Implementation for User Story 3

- [X] T050 [US3] Implement the chip actions in `crates/kb/src/tui/app.rs` (Tab/Shift-Tab group replacement, chip creation on space via `input.rs`, Backspace removal, Esc clearing both) and the chips row in `view.rs` (active chips highlighted, count 0 dimmed, `×` on removable chips, dropping from the right when narrow). Depends on T048, T019, T031.
- [X] T051 [US3] Make T048 and T049 pass and review the new snapshots. Depends on T050.

**Checkpoint**: US1 to US3 work together.

---

## Phase 6: User Story 4 - Fix an entry on the spot (Priority: P2)

**Goal**: Ctrl-E opens the editor and returns to the same query and selection; errors are reported.
`kb edit` is the scriptable form.

**Independent Test**: pty test with `EDITOR` set to a script; `cli_show.rs` for `kb edit`.

### Tests for User Story 4 ⚠️

- [X] T052 [P] [US4] Extend `app.rs` tests: Ctrl-E with a selection returns `EditEntry(id)`; with none sets `Nothing to pick`; `EditorReturned(None)` sets `refreshing`, returns `Refresh` and `Search` (same text and chips) and keeps `selected` by ID; `EditorReturned(Some(error))` sets a warning message `path:line KBxxx text` that stays until the next key; the next key clears it.
- [X] T053 [P] [US4] Add pty tests to `tui_pty.rs` with `EDITOR` pointing at a script in a temp dir: Ctrl-E hands the right absolute path to the script (script writes `$1` to a marker file), the screen leaves the alternate screen while it runs (check the vt100 state), and on return the same query and selection are shown; the script appends a word to the title and the new title appears in the list after return; the script writes `type: nope` into the file and the TUI shows a notice naming `KB001` and the entry disappears from the list; `VISUAL` wins over `EDITOR`; an editor that fails to start shows `Refresh`-style failure notice and the TUI stays open. Add to `cli_show.rs`: `kb edit <id>` runs `$EDITOR` with the file and exits 0; `kb edit` prints the first lint error to stderr when the editor leaves one and still exits 0; unknown ID → exit 2; with `--json` errors use the error document.

### Implementation for User Story 4

- [X] T054 [US4] Implement `Effect::EditEntry` in `crates/kb/src/tui/mod.rs`: `TerminalGuard::suspend()` (leave alternate screen, disable raw mode and mouse capture), `Platform::run_editor(path, &tty)` for the entry's absolute path (from the UI-thread `Kb`, never composed by hand), `TerminalGuard::resume()`, then `Kb::lint_entry(id)` → `Action::EditorReturned(first error)`, and the follow-up `Refresh`/`Search`. A spawn failure becomes a failure message, not an exit. Depends on T022, T052, T032.
- [X] T055 [P] [US4] Implement `kb edit <id>` in `crates/kb/src/commands.rs` (`run_edit`, wired in `main.rs`): resolve the file through `Kb`, open the tty (or inherit stdio when there is none), run the editor, then print the first lint error as `path:line: error[KBxxx]: message` to stderr; exit codes per [contracts/cli.md](contracts/cli.md#kb-edit-id). Depends on T053.
- [X] T056 [US4] Make T052 and T053 pass. Depends on T054, T055.

**Checkpoint**: US1 to US4 work together.

---

## Phase 7: User Story 5 - Learn the keys (Priority: P3)

**Goal**: help overlay and a key bar that is always accurate.

### Tests for User Story 5 ⚠️

- [X] T057 [P] [US5] Extend `app.rs` tests: `?` on empty text opens the overlay (`overlay = Help`) and `?` with text typed inserts the character; F1 opens it in any state; any key (including Esc, Enter, a letter) closes it and is consumed (no search, no pick); Ctrl-C still quits from the overlay; `key_bar` states follow keys.md (assert the exact bar text at width 120 for results, no results, empty KB, full preview, overlay).
- [X] T058 [P] [US5] Add snapshot cases: help overlay at 80×24 and 120×40 (dark, light, nocolor) and the key bar in each state; add a pty test: `?` shows the overlay containing every key from the table (assert each label appears), any key closes it, and F1 works while text is typed. Depends on T039.

### Implementation for User Story 5

- [X] T059 [US5] Implement the overlay in `view.rs` from `keys::help_lines()` (centered bordered box, grouped, clipped to the terminal size with the sentence about swallowed keys) and the overlay actions in `app.rs`. Depends on T018, T057, T058.
- [X] T060 [US5] Make T057 and T058 pass; confirm the key bar never shows `Enter pick` or `^E edit` when there is no selection. Depends on T059.

**Checkpoint**: all five stories work.

---

## Phase 8: Cross-Cutting Edge Cases, Robustness and Polish

**Purpose**: the spec's edge cases, SC-003 to SC-006, performance, documentation and the final gate.

- [X] T061 [P] Write and then implement the narrow, full-preview and too-small behavior ([contracts/screen.md](contracts/screen.md#regions-top-to-bottom)): `app.rs` tests for Alt-P toggling `layout_mode`, Esc leaving full preview, scroll keys in full preview, `Mode::TooSmall` below 60×15 (only Ctrl-C and resize act) and resizing back restoring the previous mode and selection; snapshot cases at 80×24 (list only), 80×24 full preview, 59×15 and 60×14 (`Terminal too small`), and 60×15 (smallest valid); implement in `layout.rs`, `view.rs` and `app.rs`.
- [X] T062 [P] Write and then implement the optional mouse support (`mouse = true`): `app.rs` tests that wheel over the list moves the selection by 3, wheel over the preview scrolls by 3, left click selects the clicked row, nothing else is handled and `mouse = false` never enables capture (`terminal.rs` enter/restore tests assert the escape sequences); add the pty test that with the default config a wheel event sequence is ignored.
- [X] T063 [P] Write and then implement the `NO_COLOR` and theme behavior end to end: snapshot cases for `nocolor` (selection reverse video, matches bold + underline, variables bold + reverse, badges without color) and `light`; a pty test with `NO_COLOR=1` asserting the vt100 screen has no cells with a foreground or background color; `theme: light` read from a config file via `HOME`/`XDG_CONFIG_HOME` is honored; an invalid `theme:` exits 2 before the terminal is touched.
- [X] T064 [P] Complete the snapshot matrix for SC-004 in `crates/kb/tests/tui_snapshots.rs`: loop over every fixture entry type (sql, sumo, cw, cmd, prompt, link, code, plus `tui/cjk-and-emoji`) × sizes 80×24, 120×40, 200×60 × themes dark, light, nocolor with the entry selected; assert the total case count equals 8 × 3 × 3 so a dropped case fails; check wide-character rows never exceed the pane width (assert every rendered line's display width ≤ the buffer width) and long titles/tags end with `…`.
- [X] T065 Write the restore-and-exit loop for SC-003 in `crates/kb/tests/tui_pty.rs`: 100 runs cycling Enter, Esc, Ctrl-C, SIGTERM (sent to the process group) and a forced panic (`KB_TUI_PANIC_AFTER_FIRST_FRAME=1`, debug build), each followed by assertions that the process exited with the documented code (0, 130, 130, 143, 2), the vt100 alternate-screen flag is off, the cursor is visible, and the pty termios shows canonical mode and echo on. Also: SIGHUP gives 129; killing during the editor (Ctrl-E) restores the terminal; a resize during a search keeps the state. Depends on T032.
- [X] T066 [P] Add the timing tests to `crates/kb/tests/perf.rs` (ignored by default, release mode, serialized, mtimes backdated): SC-001 first frame ≤ 100 ms on 10,000 generated entries (spawn `kb` in a pty, time until the header text appears on the vt100 screen, median of 10); SC-002 keystroke-to-list ≤ 50 ms (time from sending a character to the vt100 screen showing the new counter, median of 20, with the 1-second refresh timer disabled by the environment variable `KB_TUI_NO_REFRESH_TIMER`, which the binary honors in every build (it is a test knob, undocumented for users), plus one run with it on); FR-019 one `draw` of a state holding 10,000 results at 200×60 ≤ 16 ms (median of 50, in-process, release); SC-005 start-to-pick of a named entry by typing its title and pressing Enter ≤ 5 s including process start. Print the measured numbers.
- [X] T067 [P] Write the "Using the TUI" section of `README.md` in the same change: starting it (`kb`, `kb 5xx auth`, `kb tui`), the key table (generated from `keys::help_lines` text, kept in sync by a test that fails if README lacks a bound key), `--print` modes and `kb | pbcopy` / `$(kb)`, `kb show` and `kb edit`, config `theme` and `mouse`, `NO_COLOR`, the fact that Enter prints rather than copies until M4, and the terminals that swallow some keys; update the status line to "milestone M3".
- [X] T068 [P] Update `.github/workflows/ci.yml`: the existing matrix runs the pty tests on Ubuntu and macOS (no change needed beyond making sure `portable-pty` tests don't require a display); the perf job also runs the new ignored TUI timing tests; confirm the 1.88 MSRV job still builds with the new dependencies.
- [X] T069 Run `cargo tree -e normal --depth 1 -p kb` and confirm the new direct dependencies are exactly `ratatui`, `syntect`, `signal-hook` and `unicode-width`, and that no crate in the tree does network I/O; record the result and the measured timing numbers from T066 under "Post-implementation re-check" in `specs/003-tui-core/plan.md`.
- [ ] T070 (NOT DONE: the manual terminal matrix needs a person at each terminal; the non-interactive commands of quickstart sections 1 to 6 are covered by `cli_show.rs`, `tui_pty.rs` and the snapshot tests) Run every command of [quickstart.md](quickstart.md) sections 1 to 6 against a release build in a real terminal, fix any drift between the document and the behavior, and tick the manual terminal matrix (SC-006) on each listed terminal; record in `quickstart.md` which terminals were checked and any key a terminal swallowed. Never use the default root.
- [X] T071 Final gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` on stable and on `cargo +1.88`, `cargo test --workspace --release -- --ignored`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`. Re-check each constitution principle against the finished code and write the result (including anything not done test-first) in the plan's re-check section.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: T001 first; T002 and T003 in parallel after it.
- **Foundational (Phase 2)**: needs Phase 1. Core tests T004 to T007 in parallel, then T008, T009, T010, T011 (T008 to T010 in parallel; T011 before T012), T012. UI foundation tests T013 to T017 in parallel with the core tests (different crates); their implementations T018 to T021 in parallel, T022 after T017.
- **US1 (Phase 3)**: needs Phase 2. Tests T023 to T027 in parallel; implementations T028, then T029 and T030 in parallel, T031, T032, T033; T034 independent of the TUI (needs only T010).
- **US2 (Phase 4)**: needs US1 (it extends `view.rs`, `worker.rs`, `app.rs`). T036, T037, T038, T039, T040 in parallel; T041 to T043 in parallel; then T044, T045, T046, T047.
- **US3, US4, US5 (Phases 5 to 7)**: each needs US1; they extend `app.rs`/`view.rs`, so run them one after another unless two people coordinate on those files. US4's `kb edit` (T055) is independent of the TUI.
- **Polish (Phase 8)**: T061 to T064 need the stories they exercise; T065 needs US1; T066 needs US1 and US2; T070 and T071 are last.

### Within Each Story

- Tests are written and seen failing before implementation (Constitution VIII).
- Core before UI; `app.rs` (pure) before `view.rs` and `mod.rs` (impure).
- Tasks editing `app.rs`, `view.rs` or `worker.rs` are never marked [P] against each other.

### Parallel Opportunities

- Phase 2: T004 to T007 and T013 to T017 are eleven independent test files; T008 to T010 and T018 to T021 are independent implementations.
- US1 tests: T023, T024, T025, T026, T027 together.
- US2: T036 to T040, then T041 to T043.
- Polish: T061 to T064, T066 to T068.

### Parallel Example: Phase 2 tests

```text
Task: "config tests in crates/kb-core/src/config.rs"        (T004)
Task: "payload_pieces tests in crates/kb-core/src/payload.rs" (T005)
Task: "key map tests in crates/kb/src/tui/keys.rs"           (T013)
Task: "line editor and chip tests in crates/kb/src/tui/input.rs" (T014)
Task: "layout and truncation tests in crates/kb/src/tui/layout.rs" (T015)
```

---

## Implementation Strategy

### MVP First (User Story 1)

1. Phase 1 and Phase 2.
2. Phase 3 (US1). **Stop and validate**: quickstart section 1 in a real terminal; `kb | pbcopy`;
   `kb show`.
3. This alone is a usable tool: search-as-you-type with a minimal preview (title and path).

### Incremental Delivery

1. Setup + Foundational → US1 (MVP).
2. US2 → the real preview (the other P1; ship here for the P1 set).
3. US3 → chips and type cycling. US4 → edit in place. US5 → help.
4. Phase 8: edge cases, restore proof (SC-003), timings, docs, manual terminal matrix.

### Notes

- The 1.88 minimum was checked for the dependency set in a scratch crate; T001 repeats it on the real workspace.
- `Ctrl-Home`, `Ctrl-End`, `Shift-PgUp` and `Shift-PgDn` may not reach kb in some terminals; that is documented behavior, not a bug to chase (research R2).
- Commit after each task or logical group with Conventional Commits (`feat(core): …`, `feat(tui): …`, `test: …`) when you are asked to commit.
