# Research: TUI Core — Search and Preview

Phase 0 for `003-tui-core`. Technical Context had no open NEEDS CLARIFICATION items. A scratch spike
(ratatui 0.30.2, syntect 5 with `regex-fancy`, signal-hook 0.3, vt100 0.15, portable-pty 0.9; release
build, macOS arm64) measured what is marked **[spike]**. The whole set also compiled with
`cargo +1.88 test --no-run`.

## R1. Architecture: state, update, view, effects (FR-021, FR-019)

- **Decision**: `AppState` is changed only by `update(&mut AppState, Action) -> Vec<Effect>`. Actions
  are things that happened (a key, a worker reply, a tick, a resize). Effects are things to do
  (send a request to the worker, copy, pick, open the editor, quit). `draw(&AppState, &Theme,
  &mut Buffer)` is a pure function. Only the event loop in `tui/mod.rs` touches the terminal,
  threads and clock.
- **Rationale**: key handling, selection rules, chip editing and the "transient message for 2 s"
  logic become plain unit tests; screens become snapshot tests on a ratatui `Buffer` with no
  terminal (FR-021). Time enters as a value (`Action::Tick(Instant)`), never read inside `update`.
- **Alternatives considered**: ratatui "component" widgets that own state and I/O (hard to snapshot,
  hides effects); an Elm-style framework crate (a dependency for a 60-line pattern).

## R2. Terminal I/O: draw on `/dev/tty`, restore on every path (FR-001, FR-002, FR-004a, SC-003)

- **Decision**:
  - *Is there a terminal?* `stdin().is_terminal()` or `File::open("/dev/tty")` succeeds. A redirected
    stdout does not matter. If neither holds, print the help text to stderr and exit 2.
  - *Where to draw*: open `/dev/tty` read-write and give that handle to ratatui's crossterm backend.
    crossterm reads key events from stdin when it is a terminal and from `/dev/tty` otherwise, so
    `echo x | kb` and `kb | pbcopy` both work.
  - *Restore*: a `TerminalGuard` whose `Drop` leaves the alternate screen, shows the cursor,
    disables mouse capture if enabled, and disables raw mode. The same restore function is called
    from a panic hook (installed before raw mode, chained to the previous hook) and from the exit
    path for signals.
  - *Signals*: `signal_hook::flag::register` sets an atomic flag for SIGTERM, SIGHUP and SIGINT; the
    loop checks it every tick (at most 50 ms), then restores and exits. This needs no `unsafe`.
    Exit codes: SIGINT and Ctrl-C 130 (spec), SIGTERM 143, SIGHUP 129 (128 + signal, the shell
    convention; FR-018 lists only the user-driven cases).
  - *Keyboard*: no kitty-protocol negotiation. It needs a round trip to the terminal at startup, which
    is risky inside tmux and over slow links, and the startup budget is 100 ms. Keys that some
    terminals don't send (Ctrl-Home, Ctrl-End, Shift-PgUp, Shift-PgDn) simply don't arrive there; each
    has a working alternative (PgUp, PgDn, Shift-↑, Shift-↓, Alt-P) and the help overlay says so.
- **Rationale**: this is the `fzf` model, which is what the spec asks for. Drawing on stdout would
  put escape codes into the pipe.
- **Learned while implementing**: ratatui 0.30's `Terminal::clear()` asks the terminal for the cursor position and waits up to two seconds for the answer, so after the editor returns the UI calls `Terminal::resize` (a full redraw with no query) instead. The restore sequences are written exactly once, whoever gets there first (panic hook or guard), because leaving the alternate screen twice moves the cursor back to its saved position and the shell's next output overwrites what was printed in between, such as the panic message. SIGTERM, SIGHUP and SIGINT arriving while the editor runs end the editor and the program (the handlers replace the default action, so the wait must poll them).
- **Alternatives considered**: drawing on stderr (breaks when stderr is redirected, and mixes with
  error messages); a `--tty` flag (every user would need it); `ratatui::init()` (draws on stdout).

## R3. Threads, requests and "latest wins" (FR-003, FR-012, SC-001, SC-002)

- **Decision**: the UI thread owns the screen and a second, index-free `Kb` for pick, copy, edit
  checks and `record_use`. A single worker thread creates its own `Kb` and owns the index. They talk
  over `std::sync::mpsc`:
  - UI → worker: `Refresh`, `Search { generation, query, limit }`, `Preview { generation, id }`.
  - worker → UI: `Refreshed { entries, skipped, notices, duration }`, `Results { generation, ... }`,
    `Preview { generation, model }`, `Failed { message }`.
  - The worker blocks on `recv`, then drains the channel and keeps only the newest request of each
    kind. A reply whose generation is older than the UI's current one is dropped without drawing.
  - The first frame is drawn right after the terminal is set up, with an empty list, the KB root in
    the header and "refreshing…". The worker's first `Refresh` then first `Search` fill it in.
  - Refresh schedule (spec clarification): on open, on Ctrl-L, after the editor returns, and on a
    1-second timer. If a refresh takes longer than 250 ms the timer backs off to four times its
    duration, so a huge KB doesn't spend its life re-walking.
  - Keystrokes: the loop reads every pending event, applies all of them, then sends one `Search`.
    Typing fast therefore costs one search per frame at most.
- **Rationale**: M2 measured the warm walk at ~40 ms for 10,000 entries, which would sit on the UI
  thread and blow the 16 ms frame budget once per second. A worker keeps typing smooth and makes the
  first frame independent of the index. Coalescing is the simplest correct "discard the older search".
- **Alternatives considered**: all on one thread with a time budget (cannot split a 40 ms walk); an
  async runtime (a large dependency for two threads); cancelling an in-flight SQLite query
  (`sqlite3_interrupt` needs `unsafe`; queries are a few ms, so waiting is fine).

## R4. Syntax highlighting (FR-007, US2, SC-004)

- **Decision**: use syntect only as a *parser*. For each payload line, `ParseState::parse_line` gives
  scope operations; a `ScopeStack` tells which scope covers each span; one function maps scopes to a
  small `Class` enum (`Keyword`, `String`, `Number`, `Comment`, `Operator`, `Function`, `Field`,
  `Variable`, `Plain`, ...). The theme maps `Class` to a ratatui `Style`.
  - SQL: syntect's "SQL". Shell: "Bourne Again Shell (bash)" for `bash`, `sh` and `zsh`; the same
    grammar for `fish` (no fish grammar ships; keywords differ little for display). Prompts and
    anything else: plain text.
  - Sumo Logic and CloudWatch Insights: two embedded `.sublime-syntax` files loaded with
    `SyntaxDefinition::load_from_str` ([spike] 0.9 ms to load).
  - `{{variable}}` marking is not done by the grammars. The core's `payload_pieces` splits the
    payload into text and variable pieces using M1's `scan_refs` (so `\{{` stays a literal, exactly
    as `kb lint` sees it); highlighting runs on the text with variables replaced by an
    unlikely-to-collide placeholder of equal width, then the variable spans are re-inserted with
    class `Variable`.
  - Cost [spike]: loading the default pack 1.1 ms; the first parse of a language 22 to 36 ms
    (regex compilation); steady state about 1.5 ms for 15 lines. So highlighting runs on the worker,
    results are cached per `(id, mtime)`, and the worker parses a few throwaway lines of each of the
    four languages once, right after the first search, so a user rarely pays the cold cost.
  - `NO_COLOR`: skip syntect altogether; variables are shown in bold and reverse video.
- **Rationale**: constitution Technical Constraints name syntect. Using only its parser avoids
  shipping or converting color themes, makes light/dark/`NO_COLOR` one code path, and gives the exact
  fixed set of classes the snapshot tests need. [spike] binary size grew to 3.4 MB for the spike,
  acceptable for a single static binary.
- **Alternatives considered**: syntect's own themes (RGB values that clash with the terminal palette
  and don't have a light/dark story we control); hand-written tokenizers for all five languages (less
  code at first, but contradicts the constitution and leaves SQL and shell worse than a real grammar);
  tree-sitter (many C grammars).

## R5. Notes as Markdown (US2 scenario 3)

- **Decision**: render notes with `pulldown-cmark` (already a dependency) into blocks of styled spans:
  paragraphs, bullet and numbered lists, block quotes, fenced code (plain, dim), headings (bold).
  Inline: **bold**, *italic*, `code`, links (text underlined, the URL not printed). Anything else
  (tables, images, HTML) is shown as its text. Wrapping to the pane width happens at draw time with
  display widths from `unicode-width`.
- **Rationale**: the spec asks for "basic Markdown rendering"; the parser is already in the tree.
- **Alternatives considered**: showing raw Markdown (fails the scenario); `termimad` or `tui-markdown`
  (new dependencies for a few dozen lines).

## R6. Query model: text plus chips (US3)

- **Decision**: the query box holds *free text only*. Filters live as chips: type (single group),
  `tag:`, `-tag:`, `var:`, `repo:`, `in:`. When the user types a filter token and then a space, the
  token leaves the text and becomes a chip; Backspace on an empty text removes the last chip. The
  type filter is one group (clarification): Tab and Shift-Tab replace all type chips with the next or
  previous single type, cycling through "all" (no chip); typing `type:sql ` makes the `sql` chip active.
  The query sent to the worker is the chips rendered as M2 syntax followed by the text, so
  `Query::parse` sees exactly what `kb search` would. Esc clears text and chips.
- **Rationale**: matches the spec's wording (chips, Backspace removes them) and keeps the search
  semantics in one place, M2's parser.
- **Alternatives considered**: leaving everything as text and only *rendering* chips (cannot satisfy
  "Backspace removes the last chip" on an empty line).

## R7. Results, counts and paging (FR-004, US3 scenario 1)

- **Decision**: M2's `search` already returns a ranked page and `total`. M3 needs per-type counts that
  ignore the active type filter, so `kb-core` applies the `types` filter *after* collecting candidates
  instead of in SQL, counts types over the candidates, then filters and pages (details in
  [contracts/core-api.md](contracts/core-api.md)). The 1,000-candidate cap now applies before the type
  filter, so a type-filtered search sees the best 1,000 overall rather than the best 1,000 of that
  type; at 10,000 entries and a 20-row visible list this does not change what the user sees.
  The TUI asks for 60 results, asks again with a bigger limit when the selection comes within 10 of
  the end (so scrolling stays smooth), and asks for `limit = total` on Ctrl-End.
- **Rationale**: counts must come from the same candidates as the list or the chips would disagree
  with it. Re-querying is cheap (milliseconds) and needs no cursor protocol.
- **Alternatives considered**: a second GROUP BY query for counts (disagrees with typo-tier results);
  loading every result up front (10,000 row loads and file checks on open).

## R8. Rendering, layout and budgets (FR-004 to FR-008, FR-019, edge cases)

- **Decision**: regions from a pure `layout(area) -> Layout` function. Fixed rows: header 1, search 1,
  chips 1, key bar 1; the body takes the rest. Wide mode (≥ 100 columns): list 40% (at least 34
  columns), preview the rest. Narrow (60 to 99 columns): list only, Alt-P swaps in a full-screen
  preview. Below 60×15: only "Terminal too small". List rows are two lines (badge, title; tags and
  use count). Only visible rows are built each frame, so the cost does not depend on KB size.
  Truncation uses display width and never cuts a wide character in half; overflow shows `…`.
- **Rationale**: keeps `draw` O(screen). The 16 ms test builds a 10,000-result state and draws it.
- **Alternatives considered**: ratatui `List` with all items (O(n) per frame).

## R9. Colors and themes (FR-008, FR-009, NO_COLOR)

- **Decision**: type colors are fixed ANSI-256 indexes chosen to read on both light and dark
  backgrounds (e.g. sql blue, sumo orange, cw yellow-green, cmd green, prompt magenta, link cyan,
  code red-brown); the table is in [contracts/screen.md](contracts/screen.md). Dark and light themes
  differ in the neutral colors (text, muted, selection background, borders). `theme = auto`: use
  `COLORFGBG` when the terminal sets it (rxvt-style, also set by iTerm2 and some others), else dark.
  True color is used for the selection background and syntax classes only when `COLORTERM` is
  `truecolor` or `24bit`; otherwise the nearest 256-color index. `NO_COLOR` (non-empty) means no color
  at all: selection is reverse video, matches are bold, variables bold plus reverse, type badges
  keep their text.
- **Rationale**: querying the background with OSC 11 needs a reply read from the terminal, which can
  take longer than the 100 ms first-frame budget and misbehaves in tmux and over SSH; `COLORFGBG` is
  free. `theme = light|dark` covers the rest.
- **Alternatives considered**: OSC 11 on every start (latency, unreliable replies); no `auto`.

## R10. Pick, copy and edit (FR-014 to FR-018, US4)

- **Decision**:
  - *Pick (Enter)*: record the use with `Kb::record_use` (a failure is a warning on stderr and does
    not change the exit code), restore the terminal, then write the pick output to stdout exactly as
    produced by `kb show --print <mode>`: no added newline (clarification), `json` followed by one
    newline. Exit 0.
  - *Copy (Ctrl-Y)*: payload text → OSC 52 (`ESC ] 52 ; c ; <base64> BEL`) written to the tty; inside
    tmux (`TMUX` set) wrapped in the tmux passthrough (`ESC P tmux ; ... ESC \`). Base64 is a dozen
    lines, no dependency. Payloads above 74,000 bytes show "Too large for the terminal clipboard"
    instead (terminals commonly cap at 100,000 encoded bytes). The key bar says "Copied (terminal
    clipboard)" for 2 seconds. A use is recorded.
  - *Edit (Ctrl-E)*: leave the alternate screen and raw mode, run the editor with stdin, stdout and
    stderr all on `/dev/tty` (stdout may be a pipe), through `sh -c 'exec $VISUAL "$@"'` so editors
    with arguments (`code -w`) work, in this order: `$VISUAL`, `$EDITOR`, `vi`. On return: set raw
    mode and the alternate screen again, force a refresh, keep query and selection, then check the
    file with `Kb::lint_entry`; if it now has an error, show `path:line KBxxx message` of the first
    error in the key bar notice until the next key. (The entry drops out of the list because M2
    leaves entries with errors out of the index.)
  - Exit codes: pick 0, Esc and Ctrl-C 130, any internal error 2 with `kb: <message>` on stderr after
    restoring the terminal.
- **Rationale**: matches the spec and the clarification; routing pick through the same function as
  `kb show` guarantees the TUI and the scriptable command agree.
- **Alternatives considered**: calling `pbcopy` or `xclip` (M4); printing a newline on pick
  (rejected by the user: it would run commands when pasted).

## R11. Key map (FR-010, FR-011)

- **Decision**: one `const BINDINGS: &[Binding]` table: key, mode set, action, group, label, and an
  optional key-bar priority. A `const fn` checks that no (key, mode) pair appears twice and the
  crate fails to compile otherwise (FR-011 "the build MUST fail"). The help overlay and the key bar
  are generated from the same table, so they cannot drift. Line-editing keys (Ctrl-A, Ctrl-W, Ctrl-U,
  Home, End, Left, Right, Backspace, Delete) are part of the table in the `Search` mode.
  Ctrl-C, Ctrl-J, Ctrl-K, Ctrl-N and Ctrl-P are plain control characters, which raw mode delivers as
  key events; Ctrl-J would be Enter if raw mode were off, and the guard turns it on before the first
  read.
- **Rationale**: single source of truth; compile-time duplicate detection is the literal reading of
  the spec.
- **Alternatives considered**: a unit test only (the spec says the build must fail); a macro DSL.

## R12. Mouse (FR-013)

- **Decision**: included, off by default. `mouse = true` in config enables crossterm mouse capture:
  wheel over the list moves the selection, wheel over the preview scrolls it, click selects a row.
  Nothing else. Off by default so terminal text selection keeps working.
- **Alternatives considered**: leaving mouse out (the spec allows it; it is cheap here because the
  layout regions are already computed).

## R13. Config and the M2 hooks (spec Assumptions)

- **Decision**: `kb-core::config::read_config` parses `theme` and `mouse` from the same YAML file as
  `root`, reusing M1's YAML reader; unknown keys are ignored; a bad value is a configuration error,
  exit 2. M2 hooks, all read-only: see [contracts/core-api.md](contracts/core-api.md).
  Index version 2 stores each entry's `updated:` value so results can carry it without opening
  files; file modification time already is in the `files` table.
- **Rationale**: keeps policy in the core (IV) and keeps the TUI free of file access (FR-020).

## R14. Dependencies

| Crate | Why | Rejected alternative |
|-------|-----|----------------------|
| `ratatui` 0.30 (with its crossterm backend) | the TUI, named in the constitution | writing a terminal renderer |
| `syntect` 5 (`parsing`, `default-syntaxes`, `regex-fancy`, `yaml-load`) | highlighting, named in the constitution; `regex-fancy` is pure Rust (no C toolchain, static binary) | `onig` (C library), hand-written tokenizers |
| `signal-hook` 0.3 | SIGTERM/SIGHUP/SIGINT flags with no `unsafe`; already in the tree via crossterm | raw `libc::signal` (needs `unsafe`, forbidden) |
| `crossterm` 0.29, feature `use-dev-tty` | the same crossterm ratatui already uses, with the feature that makes it read keys from `/dev/tty` when stdin is not a terminal (`echo x \| kb`); without it the UI fails to start with a piped stdin | `dup2` of `/dev/tty` onto stdin (needs `unsafe`, forbidden) |
| `unicode-width` 0.2 | display width for truncation and wrapping; already in the tree via ratatui | assuming one column per char (breaks CJK and emoji, a spec edge case) |
| dev: `vt100` 0.15 | parses the program's terminal output into a screen we can assert on | string matching on raw escape codes |
| dev: `portable-pty` 0.9 | runs the real binary under a pseudo-terminal, sends keys and signals, reads termios | shelling out to `script` or `expect` (not portable) |

`cargo tree -e normal --depth 1 -p kb` after implementation must show exactly these new direct
dependencies and no networking crate.

## R15. Testing the terminal contract (SC-003, SC-006)

- **Decision**: three layers. (1) Unit and snapshot tests need no terminal. (2) Pseudo-terminal tests
  start the real `kb`, wait for text on the `vt100` screen, send keys, and assert on the screen, the
  exit code and stdout. SC-003 is a loop of 100 runs cycling Enter, Esc, Ctrl-C, SIGTERM and a panic
  (triggered by an environment variable `KB_TUI_PANIC_AFTER_FIRST_FRAME` that exists only in test
  builds, `cfg(debug_assertions)`); after each, the test checks that the alternate screen is off, the
  cursor is visible, and the pty's termios has canonical mode and echo back on. (3) SC-006 (several
  terminals, tmux) cannot be automated; [quickstart.md](quickstart.md) has a checklist to run by hand
  on the listed terminals before the milestone is merged.
- **Alternatives considered**: running only layer 1 (does not prove restoration); a headless terminal
  emulator in CI for every terminal brand (does not exist).
