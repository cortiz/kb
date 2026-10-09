# Research: Platform Layer, Clipboard and Basic CLI

Each entry: decision, reason, alternatives rejected. "Checked" means I read the code or ran it in
this repository; "from practice" means documented behavior of the tool that is not testable here
(no Linux, X11, Wayland, screen or SSH on this machine).

## R1. The `Platform` trait: one trait, no default methods, one fake

**Decision.** Extend the existing `Platform` trait (M3: `home_dir`, `config_file`, `has_terminal`,
`open_tty`, `run_editor`) to the full FR-002 list, with **no default methods**. Implementations:
`MacOs`, `Linux` (each a thin delegation to shared `unix.rs` helpers plus the few real differences)
and `FakePlatform`. Because nothing has a default, adding a method to the trait fails to compile
until the fake implements it; that is how SC-004's "the test double covers 100% of the interface"
is enforced rather than hoped for. One more test calls every method on the fake so the claim is
also executed.

The real differences, and the only places `cfg(target_os)` is needed:

| Concern | macOS | Linux |
|---|---|---|
| Config read order | `~/.config/kb/config.toml`, then `~/Library/Application Support/kb/config.toml`, then legacy `config.yml` in the same two places | `$XDG_CONFIG_HOME/kb/config.toml` (absolute only), else `~/.config/kb/config.toml`, then legacy `config.yml` |
| Config write path | `~/.config/kb/config.toml` | the first Linux candidate |
| Data / cache dir | `~/Library/Application Support/kb`, `~/Library/Caches/kb` | `$XDG_DATA_HOME/kb` else `~/.local/share/kb`; `$XDG_CACHE_HOME/kb` else `~/.cache/kb` |
| Native clipboard tools | `pbcopy` | Wayland: `wl-copy`; X11: `xclip`, `xsel` |
| Opener | `open` | `xdg-open` |
| Display server | always "macOS" | `WAYLAND_DISPLAY` → Wayland, else `DISPLAY` → X11, else none |

**Why.** Constitution V asks for exactly this shape. Keeping the rules as pure functions that take
an environment value (R8) lets a Mac run the Linux rules in unit tests; only the `impl` blocks are
`cfg`-gated, and those are a few lines each.

**Alternatives.** Two separate traits (clipboard vs paths): rejected, every caller would need both
and the fake would split in two. Dynamic `Box<dyn Platform>`: rejected, compile-time selection is
free and callers already take `&impl Platform` (checked: `commands.rs`, `tui/mod.rs`).

## R2. Config file library: `toml_edit`

**Decision.** Add `toml_edit` to `kb-core` and use it for read, validate and write.

**Why.** Two requirements need it. FR-017: `kb config set` keeps comments, order and formatting;
`toml_edit::DocumentMut` round-trips the file byte for byte except for the edited value. FR-016:
warnings carry the line; parsed items expose their source span, and a line number is the count of
`\n` before the span start. One crate does both.

**Known limits, accepted.**
- A file that is not valid TOML at all gives one parse error with its own line/column and all
  settings fall back to defaults (there is nothing to recover from). That is still "warn once, keep
  going" (FR-016).
- `config set` on a key whose table is missing creates the table at the end of the file.
- `config set` rewrites the file through a temp file in the same directory and a rename, the same
  pattern as `usage.rs` (checked: `edit::write_entry` is entry-specific, so config gets its own
  small writer; no shared helper exists).

**Alternatives.** `toml` + `serde` for reading and line-based text surgery for writing: rejected,
breaks on quoted keys, inline tables and multi-line strings. A hand-written TOML subset parser:
rejected, TOML is not small.

## R3. Config location, format migration and precedence

**Decision.**

- **Format.** TOML, file name `config.toml`.
- **Location.** As in the R1 table and clarification 2 (macOS: `~/.config` wins; the Library file
  is read only if the first does not exist; `init` and `set` write to `~/.config`; `doctor` warns
  about an ignored second file).
- **Existing `config.yml`.** M1/M3 read `config.yml` (keys `root`, `theme`, `mouse`) at
  `~/.config/kb/` (Linux) or `~/Library/Application Support/kb/` (macOS). Checked in
  `platform/{linux,macos}.rs` and `kb-core/src/config.rs`. M4 reads `config.toml` first. If no
  `config.toml` exists and a `config.yml` does, kb reads those three keys from it (`mouse` maps to
  `tui.mouse`) and warns once: "config.yml is deprecated; run `kb config init` and move your
  settings to config.toml". It is never written. Reason: silently ignoring `root:` would open
  `~/kb` instead of the user's KB.
- **Precedence** (FR-015): flag, then env var, then file, then default. Env var name is
  `KB_<SECTION>_<KEY>` upper-cased, with top-level keys as `KB_<KEY>`: `KB_ROOT`, `KB_EDITOR`,
  `KB_THEME`, `KB_TUI_ENTER`, `KB_TUI_MOUSE`, `KB_CLIPBOARD_METHOD`, `KB_CLIPBOARD_COMMAND`,
  `KB_CLIPBOARD_OSC52_MAX_BYTES`, `KB_CLIPBOARD_TRIM_TRAILING_NEWLINE`, `KB_OPENER_COMMAND`.
  `KB_ROOT` already exists from M1, so the pattern is consistent with it.
- **Editor chain** (FR-013 with FR-015): flag (none), `KB_EDITOR`, config `editor`, `$VISUAL`,
  `$EDITOR`, `vi`. An empty value counts as unset at every step.
- **Flags that map to settings**: `--root` (root) and `copy --method` (clipboard.method). No others.
- **Values.** Strings are exact, booleans accept `true`/`false` (TOML) and, from env vars only,
  `1/0/yes/no/true/false`. `osc52_max_bytes` is a positive integer; `0` is rejected (it would
  disable OSC 52, which `method = "native"` already expresses).

**Why.** One rule per concern, and the old file does not become a trap.

**Alternatives.** Drop `config.yml` support: rejected above. Keep YAML and ignore the spec: the
spec (written knowing nothing of the M1 file) says TOML, and `toml_edit` gives FR-017; YAML has no
comment-preserving editor in the dependency tree.

## R4. The clipboard chain

**Decision.** `chain(env, settings, native_tools_available) -> Vec<Method>`, pure.

```
tools    = [Custom]                       if clipboard.command is set
         = platform.native_tools(env)     otherwise      (R1 table; Wayland tools, then X11 tools)
auto     = if env.ssh or env.display == None   → [Osc52, Stdout]
           else                                → tools… , Osc52, Stdout
native   = tools…, Osc52, Stdout
osc52    = Osc52, tools…, Stdout
stdout   = Stdout
```

- A tool that is not on `PATH` stays in the chain but is recorded as "skipped: not found" with an
  install hint, so the message can say what to install (FR-005.5, scenario 5).
- Forced methods fall back exactly like `auto` (clarification 1). `stdout` is the end of every
  chain and the only element of its own, so a copy never loses the text.
- macOS over SSH: `pbcopy` would fill the *remote* Mac's clipboard, which is not what the user
  wants, so `auto` goes straight to OSC 52 (the `ssh` flag decides, not the OS).
- Linux with both `WAYLAND_DISPLAY` and `DISPLAY` (XWayland): Wayland tools first, X11 tools after
  them, so a machine with `xclip` but no `wl-copy` still works.
- SSH with X forwarding: `ssh` wins over `DISPLAY` (edge case in the spec); `clipboard.method =
  "native"` overrides.

**Why.** It is the FR-005 order plus clarification 1 as data, so every case in the spec is a
table-driven unit test (about 25 rows), no process spawned.

**Alternatives.** Trying every method and reporting the first success without an order: loses
FR-005's priorities (a Wayland session would race xclip against wl-copy).

## R5. Running native tools: stdin pipe, null output, 2 s deadline

**Decision.** For each tool: spawn with `stdin` piped and `stdout`/`stderr` **null**, write the
text on a helper thread (so a tool that stops reading cannot block the writer), then poll
`try_wait` every 5 ms until exit or the 2 s deadline (FR-009); on timeout, kill the child and
record "timed out after 2 s". Success is exit status 0.

| Tool | Arguments | Notes |
|---|---|---|
| `pbcopy` | none | reads stdin, exits when done |
| `wl-copy` | none | forks into the background to serve the selection; parent exits at once. Output must be null or the daemon keeps our pipes open (from practice) |
| `xclip` | `-selection clipboard -in` | forks and keeps serving until another app takes the selection. The ownership edge case (spec) is satisfied by *letting it fork*; we must not wait for the daemon, only for the parent (from practice) |
| `xsel` | `--clipboard --input` | same pattern as `xclip` (from practice) |
| custom | `sh -c <clipboard.command>` | stdin gets the text; same runner and deadline |

**Why.** One runner for all five; the null-output rule is what makes `xclip` return at all. The
runner is testable here with scripts on `PATH` that exit 0, exit 1, hang (`sleep 30`) and fork a
background process that holds stdout, covering the three failure shapes without X11.

**Checked.** `xclip` ownership and the real `wl-copy`/`xsel` behavior could not be run here
(macOS). They are in the manual matrix (quickstart, last section).

**Alternatives.** `Command::output()`: hangs forever on a forking tool with inherited stdout (the
well-known `xclip` trap). A crate (`arboard`, `copypasta`): rejected, links X11/Wayland libraries
and breaks the "single static binary" constraint and the no-bundled-clipboard assumption.

## R6. PATH lookup without a crate

**Decision.** `find_tool(name)`: split `$PATH`, return the first entry where `dir/name` is a file
with an execute bit (`metadata().permissions().mode() & 0o111`). About 12 lines in `unix.rs`.

**Why.** Needed to say "not found" before spawning and for `doctor`. The `which` crate adds a
dependency for 12 lines.

## R7. OSC 52

**Decision.** Move the code from `tui/terminal.rs` to `platform/osc52.rs` and extend it.

- **Sequence.** `ESC ] 52 ; c ; <base64> BEL` (checked: this is what M3 emits; it is
  unit-tested in `tui/terminal.rs`, not in a pty test, so M4 adds an end-to-end Ctrl-Y test).
- **tmux.** `ESC P tmux ; <seq with every ESC doubled> ESC \` (checked: already in M3).
- **GNU screen** (`STY` set, not in tmux). Wrap in DCS, and split the OSC body into chunks of at
  most 250 bytes, joining chunks with `ESC \ ESC P`, so screen's DCS length limit is respected
  (from the `vim-oscyank` / `tmux-yank` practice). **Not verified against a real screen.**
- **Target.** Written to `/dev/tty` through `Platform::write_tty`, never to stdout (FR-007). In the
  TUI (Ctrl-Y) it also goes through `Platform::write_tty`, which opens `/dev/tty` separately and
  writes the whole sequence in one `write_all` + flush; ratatui flushes after every frame, so the
  two writers never interleave mid-sequence. (M3 used the guard's own writer; that path is
  removed with `copy_sequence`.) Enter copies after the terminal is restored, so it needs
  nothing special. A pty test (T052) checks the screen is intact after Ctrl-Y.
- **Limit.** `clipboard.osc52_max_bytes` (default 74,000) applies to the **text** byte length
  (spec: "74 KB of text"); base64 makes that about 98.7 KB encoded, under the common 100 KB cap.
  M3's constant applied to the same number; the check moves to the new setting. Over the limit,
  the OSC 52 attempt is recorded as `TooLarge`. **Rule (decided here): if nothing else delivered the
  text and the only reason `stdout` was reached is that size refusal, kb does not print the
  payload.** Dumping 100 KB onto a terminal helps nobody and the spec says to suggest the pipe
  instead. The outcome is `used = None` ("refused"): nothing is printed or recorded, the message is
  `kb: <id> is N bytes, over clipboard.osc52_max_bytes (74000); nothing copied. Try: kb show --raw
  <id> | <clipboard tool>`, and the exit code is 2. An explicit `clipboard.method = "stdout"`, or
  `stdout` reached because no tool/tty worked, still prints. This is what makes FR-008 testable.
- **tmux passthrough.** tmux 3.3+ drops these sequences unless `allow-passthrough` is `on` or
  `all`. `tmux_allows_passthrough()` runs `tmux show-options -gv allow-passthrough` with a 1 s
  deadline: `on`/`all` → true, `off` → false, command error (older tmux, option unknown, where
  passthrough always worked) → true, not in tmux → `None`. Only `doctor` uses it (and the copy
  message, when it knows it is `false`).
- **Terminal.app.** It ignores OSC 52. `TERM_PROGRAM=Apple_Terminal` is recorded in the
  environment so `doctor` can say so (spec assumption). tmux inherits `TERM_PROGRAM`, which is
  imperfect but right in the common case.

## R8. Environment detection

**Decision.** `Environment::detect(get: &dyn Fn(&str) -> Option<String>, os: Os) -> Environment`,
pure.

| Field | Rule |
|---|---|
| `os`, `arch` | compile-time (`cfg!`, `std::env::consts::ARCH`) |
| `ssh` | any of `SSH_CONNECTION`, `SSH_TTY`, `SSH_CLIENT` is non-empty |
| `tmux` | `TMUX` non-empty |
| `screen` | `STY` non-empty and not `tmux` |
| `display` | macOS: `Quartz`; Linux: `WAYLAND_DISPLAY` → `Wayland`; else `DISPLAY` → `X11`; else `None` |
| `true_color` | `COLORTERM` contains `truecolor` or `24bit` |
| `term`, `term_program` | `TERM`, `TERM_PROGRAM` as strings, for `doctor` hints |

Empty strings count as unset. `NO_COLOR` is not OS-specific and stays where M3 reads it.

**Why.** Everything FR-003 forbids outside the platform module is read here once. The TUI's
existing `TMUX` read and the theme's `COLORTERM` read (checked: `tui/mod.rs` `copy_selected`,
`tui/theme.rs`) move to `Environment`.

## R9. `kb show` formatted output

**Decision.** Reuse the TUI's preview rendering. Split `draw_preview` in `tui/view.rs` so its core
takes a `&PreviewModel`, a `Theme` and a width (it currently takes `&AppState`), render into a
ratatui `Buffer` of `width × total_rows`, then convert the buffer to text:

- With color (stdout is a terminal, `NO_COLOR` unset, no `--no-color`): emit SGR sequences per
  style change, reset at end of each line (~40 lines). Width is the terminal's width, else 100.
- Without color (pipe, `NO_COLOR`, `--no-color`): the same rows with styles dropped, trailing
  spaces trimmed.

`--raw` prints only the payload (`pick_output(Payload)`, same no-added-newline rule as M3's
`--print`). `--json` is `PrintMode::Json`. `--file` is the absolute path. **M3's `kb show --print
MODE` is removed** (clarification 3: `--print` on `show` is a usage error). Its modes map to
`--raw` (payload), `--json`, `--file` (path); `id` is a field of `--json`. This breaks M3 callers of
`kb show --print`; `cli_show.rs` and the README are updated in the same change.

**Why.** The spec says "formatted as in the TUI preview"; one renderer guarantees it, and the 91
M3 snapshots cover the refactor.

**Risk.** The refactor touches M3 view code. The snapshots are the safety net. If the split gets
awkward, the fallback is a plain-text formatter in `show.rs` that duplicates layout rules; I would
rather not, and will say so if it comes to that.

**Alternatives.** A new plain printer: drifts from the TUI. Shelling out to `bat`: external
dependency, not on every machine.

## R10. Resolving `<id|query>` and the "clearly best" margin

**Decision.** `Kb::resolve(text) -> Resolution` in `kb-core/src/resolve.rs`:

1. **Exact ID.** `text` (trimmed, single token without spaces) is an entry ID.
2. **ID with `.md` or a path.** Strip a trailing `.md`; if `text` is an existing file path, make it
   absolute and relative to the root; if it is under the root and parses as an entry, use that ID.
3. **Search.** Run the M2 search on the words. `Resolved` if there is one result, or the first
   result's score is at least `CLEAR_WINNER_RATIO` (1.5) times the second's. Otherwise
   `Ambiguous(top 5)`. No results is `NotFound`.

`CLEAR_WINNER_RATIO` and the top-N are constants in `resolve.rs`.

**Why a ratio.** M2 scores are positive and scale with the query (relevance × frecency boost ×
exact bonus; checked in `search.rs`, lines ~537–549), so an absolute gap would behave differently
for short and long queries; a ratio does not. 1.5 is a first value, not a measured one. The task
list must include calibrating it on the fixture KB: `5xx auth` must resolve to
`sumo/5xx-by-authorizer`; `latency` (several close cw/sumo matches) must be ambiguous. If the
numbers do not separate them, the constant moves, and the test names the fixtures that fixed it.

**Ambiguous behavior (spec US2.3).** Terminal on stdin and stdout: open the TUI with the query
(same exit-code rules as the TUI). No terminal: list up to 5 matches on stderr as `id  title`,
exit 1, copy nothing. `kb show` follows the same rule.

**Alternatives.** A fixed margin: rejected above. Always opening the TUI when more than one match:
makes `kb copy 5xx auth` useless in scripts.

## R11. `kb doctor`

**Decision.** `doctor::run(platform, kb_root, settings, load_warnings) -> Vec<Check>`. A `Check` has
an `id`, a `Status` (`Pass`/`Warn`/`Fail`), a one-line `summary` and an optional `hint`. Rendering
text and JSON is separate from running. The checks and what each one decides are in
[contracts/doctor.md](contracts/doctor.md). Highlights:

- **Failures** (exit 1): KB root missing or unreadable; index unreadable (not "stale", which M2
  auto-heals); config file present but not parseable.
- **Warnings** (exit 0): second macOS config ignored; legacy `config.yml`; Terminal.app with OSC 52
  as the method; tmux `allow-passthrough` off; chosen clipboard method is `stdout` because nothing
  else is available; editor not found on `PATH`; no true color.
- **Writable root** is checked without writing to the root (constitution I): if `.kb/` exists,
  create and delete a probe file inside it; otherwise report the root directory's permission bits.
- **`--copy-test`** copies `kb doctor test <8 random hex>` with the full chain, then prints
  "Paste here and press Enter:" and reads one line from stdin; match → pass, else fail with the
  attempts. A tty is required; without one it is a usage error (exit 2). It does not record a use.
- **SC-003 scenarios.** Missing tool → fail-or-warn with the install hint per distro-agnostic
  package name; no display → "using OSC 52" plus the SSH/terminal note; OSC 52 off in tmux → the
  `set -g allow-passthrough on` line; payload too large → `doctor` prints the configured limit and
  the `kb show --raw <id> | <tool>` workaround next to the method (it cannot know a payload), and
  the copy message itself names the offending size. The spec's "identifies the cause" is met for
  the first three by `doctor` alone and for the fourth by `doctor` plus the copy error.

## R12. Enter, `--print`, and what goes to stderr

**Decision.**
- `TuiArgs.print` becomes `Option<PrintMode>`. `Some(mode)` → Enter prints that mode (M3
  behavior). `None` → follow `tui.enter`: `copy` copies the payload; `print` prints the payload.
- Enter with copy: the TUI restores the terminal and returns `Picked`; `main` then calls
  `clipboard::copy`, writes the confirmation to stderr, records a use (rule: sent by any method),
  and exits 0. If the chain ends at `stdout`, the payload is printed to stdout and the warning
  explains why. Exit code stays 0 (spec edge case).
- Messages (stderr), one line each:
  - native: `Copied sumo/5xx-by-authorizer (6 lines) via pbcopy`
  - OSC 52: `Sent sumo/5xx-by-authorizer (6 lines) via terminal (OSC 52)`
  - stdout fallback: `Printed sumo/5xx-by-authorizer (6 lines) to stdout; no clipboard method worked`
  - preceding `kb: …` lines for each skipped or failed attempt with the fix hint (once each).
- `--print` on `kb copy` or `kb show` is a usage error, exit 2 (clarification 3). This removes
  M3's `kb show --print MODE`.
- Ctrl-Y: same `clipboard::copy`, stays open, key bar shows `Copied via pbcopy` /
  `Sent via terminal (OSC 52)` for 2 s; a use is recorded under the same rule.
- Record-use rule (clarification 4): `outcome.sent()` is `used.is_some()`: native or custom exit 0,
  OSC 52 written, or stdout printed. It is false only for the size refusal in R7. A use is
  recorded iff `sent()`. Exit code 2 when refused (an operational failure, not a pick), 0
  otherwise, including the stdout fallback.

**Why.** Reuses the M3 pick path; the only new thing is an action after restore.

## R13. Enforcing FR-003 (OS-specific calls only in `platform`)

**Decision.** `crates/kb/tests/platform_boundary.rs` (runs in `cargo test`; the repo-root `tests/*.rs` files are
wired into `kb-core` through `[[test]] path`, which cannot see `kb`'s sources, so this one lives
in `kb`'s own test directory): walks
`crates/*/src/**/*.rs` outside `crates/kb/src/platform/`, and fails if any file contains one of:

- tool names as string literals: `"pbcopy"`, `"pbpaste"`, `"wl-copy"`, `"xclip"`, `"xsel"`,
  `"xdg-open"`, `"osascript"`;
- `Command::new("open")`;
- environment names as literals: `WAYLAND_DISPLAY`, `"DISPLAY"`, `SSH_CONNECTION`, `SSH_TTY`,
  `SSH_CLIENT`, `"TMUX"`, `"STY"`, `COLORTERM`, `XDG_`, `TERM_PROGRAM`;
- `cfg(target_os` (OS conditionals belong in `platform` too).

Comments and doc strings are stripped before matching. Test directories are not scanned (fake
tools in tests must be named). Checked today: no `cfg(target_os)` exists outside `platform/`, and the only
forbidden env reads outside it are `TMUX` (`tui/mod.rs:235`) and `COLORTERM` (`tui/theme.rs:50`),
both moved to `Environment` by this milestone. A second test proves the checker works by running it on a temp tree with a
violation.

**Why.** Cheap, readable, and fails with the file and line.

## R14. Dependency and CI notes

- New runtime dependency: `toml_edit` (kb-core). Nothing else. The chosen version must build on
  MSRV 1.88; verified with `cargo +1.88 build` when it is added, and the version pinned in
  `Cargo.toml` if the latest needs a newer compiler.
- CI (`.github/workflows/ci.yml`) already runs fmt, clippy and tests on macOS and Linux. The new
  tests depend on `sh`, `sleep`, `kill`, `chmod` (already assumed by M3's pty tests). They create
  fake `pbcopy`/`wl-copy`/`xclip`/`xsel` scripts in a temp dir prepended to `PATH`; no real
  clipboard is needed in CI. One `#[ignore]` test round-trips through the real `pbcopy`/`pbpaste`
  on a Mac, run by hand.
- **Linux is still unverified**: no Linux machine here, CI has not run since M1. The Linux rules
  are covered by pure-function tests on the Mac; the first push is their first real run.
