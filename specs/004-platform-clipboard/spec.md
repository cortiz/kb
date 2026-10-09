# Feature Specification: Platform Layer, Clipboard and Basic CLI

**Feature Branch**: `004-platform-clipboard`
**Created**: 2026-10-09
**Status**: Draft
**Milestone**: M4 (was M3; swapped with the TUI core)
**Depends on**: M1 (`001-entry-format`), M2 (`002-index-search`), M3 (`003-tui-core`)
**Input**: Isolate everything OS-specific behind one platform layer, add real clipboard support on macOS and Linux (with a terminal fallback for SSH), make Enter in the TUI copy by default, and add the `kb show`, `kb copy`, `kb config` and `kb doctor` commands.

## Overview

M3 made kb usable, but picking an entry only printed it to stdout. M4 closes that gap: Enter copies to the clipboard, wherever kb is running. It also introduces the **platform layer**, the one place where macOS and Linux differ (clipboard, opening URLs and files, config and data paths, launching the editor). Constitution principle V requires this isolation before any later milestone needs to open a URL (M7) or an editor link (M8).

The config file is introduced here too, because clipboard and editor behavior are the first settings that genuinely vary by machine.

## Clarifications

### Session 2026-10-09

- Q: If `clipboard.method` is forced to `native` (or a custom `clipboard.command` is set) and it fails or is missing, fall back or stop? → A: Always fall back down the chain (native, then OSC 52, then stdout), even when forced; the confirmation names the method actually used and the stderr warning says what was skipped and why.
- Q: On macOS, if both `~/.config/kb/config.toml` and `~/Library/Application Support/kb/config.toml` exist, which is used and where do `kb config init`/`set` write? → A: `~/.config/kb/config.toml` wins; the Library file is read only when the first doesn't exist; `init` and `set` always write to `~/.config`; `kb doctor` warns about an ignored second file.
- Q: Does `--print <mode>` / `tui.enter = "print"` also change `kb copy` and `kb show`? → A: No. Both affect only the TUI's Enter key; `kb copy` always copies, `kb show` always shows, and `--print` on either is a usage error (exit 2).
- Q: Should a use be recorded when the copy was sent by OSC 52, which can't be confirmed? → A: Yes. A use is recorded whenever kb sends the text (native tool exit 0, OSC 52, or stdout fallback); nothing is recorded only if every method failed or the payload was refused for size.
- Q: Where does the one-time config warning appear when the TUI runs? → A: In the TUI header as `⚠ config: N problems (run kb doctor)`, styled like M3's "with errors" warning; the full text is printed on stderr after the TUI exits.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Enter copies, on any machine (Priority: P1)

Carlos picks an entry in the TUI and presses Enter. The payload is on his clipboard, whether he's on his Mac, on a Linux laptop under Wayland or X11, or SSH'd into a server inside tmux.

**Why this priority**: copying is the action behind almost every use of kb. Piping to `pbcopy` by hand was only a stopgap.

**Independent Test**: on each supported environment in CI or a test matrix, pick an entry and read the clipboard back with the platform's own tool.

**Acceptance Scenarios**:

1. **Given** kb runs locally on macOS, **When** the user presses Enter on an entry, **Then** the payload is placed on the system clipboard, the TUI exits, and a one-line confirmation such as `Copied sumo/5xx-by-authorizer (6 lines)` is printed to stderr.
2. **Given** kb runs on Linux under Wayland with `wl-copy` installed, or under X11 with `xclip` or `xsel` installed, **Then** the same happens using that tool.
3. **Given** kb runs over SSH (no local display), **Then** kb copies using OSC 52 so the text lands on the clipboard of the machine the user is sitting at, and the confirmation says `Sent <id> (6 lines) via terminal (OSC 52)` ("sent", not "copied", because kb cannot confirm the terminal accepted it; FR-008).
4. **Given** kb runs inside tmux, **Then** OSC 52 output is wrapped the way tmux requires, so it reaches the outer terminal.
5. **Given** no clipboard method is available at all, **Then** kb prints the payload to stdout instead, and explains on stderr why it couldn't copy and how to fix it (for example "install wl-clipboard").
6. **Given** the user wants the old behavior, **When** they set `tui.enter = "print"` in config or pass `--print`, **Then** Enter prints to stdout as in M3.

---

### User Story 2 — Copy and show from the shell (Priority: P1)

Carlos runs `kb copy sumo/5xx-by-authorizer` or `kb show 5xx` without opening the TUI.

**Why this priority**: constitution principle IV requires every TUI action to be scriptable. Copy and show are the first two.

**Acceptance Scenarios**:

1. **Given** an exact entry ID, **When** the user runs `kb copy <id>`, **Then** the payload is copied using the same method as the TUI, a use is recorded, and kb exits 0.
2. **Given** a search query instead of an ID (`kb copy 5xx auth`), **When** exactly one entry is clearly the best match (it ranks first and outscores the second by the margin defined in the plan), **Then** that entry is copied and its ID is shown in the confirmation.
3. **Given** a query with several close matches, **When** stdin and stdout are terminals, **Then** kb opens the TUI with that query so the user can choose. **When** not in a terminal, **Then** kb lists the top matches on stderr and exits 1 without copying.
4. **Given** `kb show <id or query>`, **Then** kb prints the entry formatted as in the TUI preview (title, metadata, highlighted payload, variables, notes) and exits 0. Colors are used only when stdout is a terminal.
5. **Given** `kb show --raw <id>`, **Then** kb prints only the payload, unformatted. **Given** `--json`, **Then** it prints the entry as JSON (same shape as `--print json` in M3).
6. **Given** `kb show --file <id>`, **Then** kb prints the absolute path to the entry's file.

---

### User Story 3 — Configure kb once per machine (Priority: P2)

Carlos keeps his KB in `~/work/kb` on one machine and `~/kb` on another, prefers a light theme on his laptop, and wants `nvim` as the editor.

**Why this priority**: the first settings that really differ by machine arrive in this milestone.

**Acceptance Scenarios**:

1. **Given** no config file exists, **Then** kb works with defaults and creates no file until the user asks for one.
2. **Given** `kb config init`, **Then** kb writes a commented config file that lists every setting with its default value (each setting commented out, so later changes to a default reach the user and the file shows only what was changed), at the platform's standard location, and prints the path. It MUST NOT overwrite an existing file.
3. **Given** `kb config path`, `kb config get <key>` and `kb config set <key> <value>`, **Then** they show the path, read one value, and write one value while keeping comments and order in the rest of the file.
4. **Given** `kb config list`, **Then** kb prints every setting with its current value and where it came from: default, config file, environment variable or flag.
5. **Given** the config file has an unknown key or an invalid value, **Then** kb starts anyway, warns once with the file, line and expected values, and uses the default for that setting.

---

### User Story 4 — Diagnose the environment (Priority: P2)

On a new Linux box, copying doesn't work. Carlos runs `kb doctor` to find out why.

**Why this priority**: clipboard support depends on tools and environment variables outside kb's control. The user needs a single command to find what is missing.

**Acceptance Scenarios**:

1. **Given** `kb doctor`, **Then** kb prints a checklist with a pass, warn or fail mark and a fix hint for each item:
   - OS, architecture and kb version;
   - KB root found and writable; `.kb/` present; index health (from M2);
   - config file location and whether it parsed cleanly;
   - display server detected (macOS, Wayland, X11, none), SSH session detected, tmux detected;
   - which clipboard method kb will use, and which others are available;
   - whether the terminal advertises true color;
   - editor that Ctrl-E will launch, and whether it was found on `PATH`;
   - URL opener that will be used.
2. **Given** `kb doctor --copy-test`, **Then** kb copies a short test string with the chosen method and asks the user to paste it back to confirm it arrived.
3. **Given** `--json`, **Then** the same checks are output as JSON.
4. **Given** any check fails, **Then** kb exits 1. Warnings alone exit 0.

---

### User Story 5 — The TUI uses the platform layer everywhere (Priority: P3)

**Acceptance Scenarios**:

1. **Given** the user presses Ctrl-E in the TUI, **Then** the editor comes from the platform layer's resolution order (FR-013), not from code inside the TUI.
2. **Given** Ctrl-Y in the TUI, **Then** it copies using the same method as Enter and stays open, replacing M3's OSC-52-only behavior.

---

### Edge Cases

- **Large payloads**: OSC 52 is limited by many terminals (often around 100 KB). Above the configured limit (default 74 KB of text), kb refuses to send it by OSC 52 and suggests `kb show --raw <id> | <tool>` instead. If a native tool or a custom `clipboard.command` can carry the copy, it succeeds that way. Otherwise kb prints nothing, records no use and exits 2 (FR-023); the payload is not dumped to stdout, because the alternative above is the intended path.
- **Linux with both Wayland and X11 available** (XWayland): Wayland tools are preferred when `WAYLAND_DISPLAY` is set.
- **`xclip` ownership**: on X11 the clipboard content disappears when the owning process exits. kb MUST hand off ownership correctly (for example by letting `xclip` fork and keep serving) so the content survives kb exiting.
- **SSH session that also has X forwarding**: OSC 52 is preferred, because X forwarding often copies to the wrong place or hangs. Config can override this.
- **Clipboard tool hangs** (a known issue with some X11 setups): kb gives up after 2 seconds, reports the failure and falls back to the next method.
- **Text encoding**: payloads are UTF-8 and copied as UTF-8 text. Trailing newline handling is controlled by `clipboard.trim_trailing_newline` (default `true`).
- **Copy inside the TUI while in the alternate screen**: OSC 52 sequences MUST be written so the terminal processes them without corrupting the display.
- **Read-only or missing config directory**: `kb config set` fails with a clear message. Normal commands are unaffected.
- **Exit codes from M3 stay the same** for the TUI: 0 for a pick, 130 for cancel, 2 for errors. A failed copy that fell back to stdout still exits 0, with the warning on stderr.

## Requirements *(mandatory)*

### Platform layer

- **FR-001**: All OS-specific behavior MUST live behind a platform interface with separate implementations for macOS and Linux, chosen at compile time where possible and at runtime where the environment decides (Wayland vs X11, SSH vs local).
- **FR-002**: The interface MUST cover: copy text to clipboard; open a URL; open a file or directory with the default app; resolve config, data and cache directories; resolve the editor command; and detect the environment (OS, display server, SSH, tmux, terminal capabilities).
- **FR-003**: No code outside the platform layer may call OS-specific commands or check OS-specific environment variables. A check in CI MUST enforce this (for example by forbidding direct process calls to `pbcopy`, `xclip`, `open` or `xdg-open` outside that module).
- **FR-004**: Every platform implementation MUST have a test double, so the TUI and CLI can be tested without a real clipboard or display.

### Clipboard

- **FR-005**: kb MUST choose a clipboard method in this order unless config overrides it:
  1. **macOS, local**: `pbcopy`;
  2. **Linux, Wayland** (`WAYLAND_DISPLAY` set): `wl-copy`;
  3. **Linux, X11** (`DISPLAY` set): `xclip`, then `xsel`;
  4. **SSH session or no display**: OSC 52;
  5. **Nothing available**: print to stdout with an explanation on stderr (except a payload refused by OSC 52 for size, which is not printed; see FR-023).
- **FR-006**: `clipboard.method` in config MUST accept `auto` (default), `native`, `osc52` or `stdout`, and `clipboard.command` MUST allow a custom copy command that receives the text on stdin. The configured method is tried first; if it fails or is unavailable, kb falls back through the remaining methods (native, then OSC 52, then stdout) exactly as in `auto` (the size-refusal exception in FR-005.5 applies), and warns on stderr which method was skipped and why.
- **FR-007**: OSC 52 output MUST be wrapped for tmux and for GNU screen when those are detected, and MUST be written to the controlling terminal (`/dev/tty`), not to stdout, so it works even when stdout is redirected.
- **FR-008**: Every copy MUST report which method was used, so the confirmation message is accurate. A method that can't confirm success (OSC 52) MUST be reported as "sent", never as "copied". A use MUST be recorded whenever kb sent the text by any method (including OSC 52 and the stdout fallback); it MUST NOT be recorded when the payload was refused for size and nothing else could carry it.
- **FR-009**: A copy attempt MUST time out after 2 seconds and fall back to the next available method.

### Opening and paths

- **FR-010**: Opening a URL MUST use `open` on macOS and `xdg-open` on Linux, with `opener.command` as a config override. Over SSH with no display, kb MUST print the URL instead of trying to open it. (Commands that open URLs arrive in M7; this milestone provides and tests the capability.)
- **FR-011**: Paths MUST follow platform conventions:
  - **macOS**: config in `~/.config/kb/config.toml` (preferred by terminal users) with `~/Library/Application Support/kb/config.toml` read only when the first does not exist (never merged); `kb config init` and `kb config set` always write to `~/.config/kb/config.toml`, and `kb doctor` warns when a second file is being ignored;
  - **Linux**: `$XDG_CONFIG_HOME/kb/config.toml`, falling back to `~/.config/kb/config.toml`.
  The KB's own state stays in `<kb root>/.kb/` as defined in M1 and M2.
- **FR-012**: The KB root MUST continue to be resolved as in M1's root-resolution requirement: `--root`, `KB_ROOT`, config `root`, then `~/kb`.
- **FR-013**: The editor MUST be resolved in this order: config `editor`, `$VISUAL`, `$EDITOR`, then `vi`.

### Config

- **FR-014**: Config MUST be a single TOML file. Settings in this milestone:
  - `root` (path),
  - `editor` (command),
  - `theme` (`auto`, `dark`, `light`; from M3),
  - `tui.enter` (`copy` default, or `print`),
  - `tui.mouse` (boolean; from M3),
  - `clipboard.method`, `clipboard.command`, `clipboard.osc52_max_bytes`, `clipboard.trim_trailing_newline`,
  - `opener.command`.
- **FR-015**: Precedence MUST be: command-line flag, then environment variable (`KB_<SECTION>_<KEY>`, for example `KB_CLIPBOARD_METHOD`), then config file, then built-in default.
- **FR-016**: Unknown keys and invalid values MUST produce a warning with file and line, and the default MUST be used. kb MUST NOT refuse to start because of config problems. Non-TUI commands print each warning once on stderr. The TUI shows a header warning `⚠ config: N problems (run kb doctor)` and prints the full text on stderr after it exits.
- **FR-017**: `kb config set` MUST preserve comments, key order and formatting in the rest of the file.

### Commands

- **FR-018**: `kb copy <id|query...>` MUST copy the pick output defined in M3 (payload, URL or `path:line` depending on type), record a use, and print a confirmation on stderr. `--method` MUST override the clipboard method for one run.
- **FR-019**: `kb show <id|query...>` with `--raw`, `--json`, `--file` and `--no-color`.
- **FR-020**: Both commands MUST resolve their argument by trying an exact ID first, then an ID with the `.md` extension or a path, then a search. Ambiguous searches MUST behave as in User Story 2, scenario 3.
- **FR-021**: `kb config init | path | get | set | list`.
- **FR-022**: `kb doctor` with `--copy-test` and `--json`.
- **FR-023**: Exit codes for the new commands: `0` success; `1` not found, ambiguous, or a failed doctor check; `2` usage or I/O errors, and a copy refused because the payload exceeds the OSC 52 limit when no other method can carry it (nothing is printed or recorded; stderr states the size, the limit and the `kb show --raw <id> | <tool>` alternative). A copy that falls back to printing the payload still exits `0`.

### TUI changes

- **FR-024**: Enter in the TUI MUST copy using the platform clipboard by default, then exit and print the confirmation on stderr. `tui.enter = "print"` or `kb --print <mode>` MUST restore M3's print behavior. These two settings affect only the TUI's Enter key: `kb copy` and `kb show` ignore `tui.enter`, and passing `--print` to them is a usage error (exit 2).
- **FR-025**: Ctrl-Y MUST copy with the platform clipboard and stay open, showing the method used in the key bar for 2 seconds.
- **FR-026**: Ctrl-E MUST launch the editor resolved by the platform layer.

### Key Entities

- **Platform**: the interface for clipboard, opener, paths, editor and environment detection, with macOS, Linux and test implementations.
- **Environment**: OS, architecture, display server, SSH flag, tmux and screen flags, terminal capabilities.
- **ClipboardMethod**: native tool, OSC 52, custom command or stdout, with its availability and limits.
- **CopyOutcome**: method used, bytes sent, whether success can be confirmed, and any fallback that happened.
- **Config**: all settings with their values and the source of each value.

## Success Criteria *(mandatory)*

- **SC-001**: Copying works with no setup in each of these environments: macOS local (Terminal.app and iTerm2), Linux Wayland (GNOME), Linux X11, SSH from macOS into Linux, and SSH inside tmux, verified by pasting the result.
- **SC-002**: `kb copy <id>` completes in 150 ms or less end to end on a KB of 10,000 entries with a local clipboard.
- **SC-003**: On a machine where copying fails, `kb doctor` identifies the cause and gives a working fix in each of these scenarios: missing tool, no display, OSC 52 disabled in tmux. For "payload too large", `kb doctor` shows the OSC 52 size limit and the `kb show --raw <id> | <tool>` workaround, and the failed copy itself names the payload size, the limit and that workaround (doctor cannot know a payload before one is copied).
- **SC-004**: The CI check for OS-specific calls outside the platform layer passes, and the platform test double covers 100% of the interface.
- **SC-005**: A config file with an unknown key and a bad value still lets kb start and work, with exactly one warning per problem.

## Out of Scope

- Opening links, running commands and building console URLs as user actions (M7). M4 only provides the opener capability.
- Filling in variables before copying (M5).
- Reading from the clipboard (used later by the add form in M6).
- Windows support.
- Custom key bindings.

## Assumptions

- Users on Linux can install `wl-clipboard`, `xclip` or `xsel` when told to; kb doesn't bundle a clipboard implementation.
- OSC 52 support is common enough in modern terminals (iTerm2, kitty, Alacritty, WezTerm, Windows Terminal, recent GNOME Terminal) to be the default for SSH. Terminal.app does not support it, which `kb doctor` explains.
- The config file location `~/.config/kb/` on macOS matches what terminal-tool users expect, even though it isn't the macOS default.
- M3 already shipped a minimal version of some of this: `kb show <id>` (with `--print`-style output modes), a small config reader (`theme`, mouse), a thin `Platform` trait (terminal check, TTY open, editor launch) and an OSC 52 copy for Ctrl-Y. M4 extends and replaces these; where M4 differs from M3's behavior (Enter copies instead of printing, `kb show` gains `--raw`, `--file` and query resolution), M4 wins and the M3 tests are updated.
- "Clearly the best match" for `kb copy <query>` is a score margin over the second result; the exact margin is set in the plan (a score ratio) and is final only after it has been calibrated on the fixture KB.
- Before M4 the config file was `config.yml` (keys `root`, `theme`, `mouse`). M4 reads `config.toml` first; if only a `config.yml` exists it is still read (read-only) for those three keys, with a deprecation warning, so an existing `root` is not silently ignored. `mouse` becomes `tui.mouse`.
