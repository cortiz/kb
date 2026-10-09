# Data Model: Platform Layer, Clipboard and Basic CLI

Nothing here is stored except the config file. All other entities are in-memory values.

## Environment (`kb::platform::env`)

| Field | Type | Rule |
|---|---|---|
| `os` | `Os::{MacOs, Linux}` | compile time |
| `arch` | string | `std::env::consts::ARCH` |
| `display` | `Display::{Quartz, Wayland, X11, None}` | research R8 |
| `ssh`, `tmux`, `screen` | bool | R8; `screen` is false inside tmux |
| `true_color` | bool | `COLORTERM` has `truecolor` or `24bit` |
| `term`, `term_program` | `Option<String>` | for hints only |

Built by `Environment::detect(get, os)`, a pure function; the real platform passes `std::env::var`.

## Setting and Settings (`kb_core::config`)

One const table `SETTINGS: &[Setting]`; each row has `key` (`"clipboard.method"`), `kind`, `default`,
`env` (`"KB_CLIPBOARD_METHOD"`), `doc` (the comment written by `config init`).

| Key | Kind | Default | Notes |
|---|---|---|---|
| `root` | path | `~/kb` | `~` expanded; a flag (`--root`) overrides |
| `editor` | string | unset | command with arguments allowed |
| `theme` | `auto\|dark\|light` | `auto` | from M3 |
| `tui.enter` | `copy\|print` | `copy` | |
| `tui.mouse` | bool | `false` | was top-level `mouse` in M3 |
| `clipboard.method` | `auto\|native\|osc52\|stdout` | `auto` | `copy --method` overrides |
| `clipboard.command` | string | unset | receives text on stdin |
| `clipboard.osc52_max_bytes` | integer ≥ 1 | `74000` | text bytes |
| `clipboard.trim_trailing_newline` | bool | `true` | |
| `opener.command` | string | unset | gets the URL or path as its argument |

`Settings` is the typed struct (`root: Resolved<PathBuf>`, …), where
`Resolved<T> { value: T, source: Source, line: Option<usize> }` and
`Source::{Default, File(PathBuf), Env(&'static str), Flag(&'static str)}`.

`Loaded { settings: Settings, warnings: Vec<ConfigWarning>, file: Option<PathBuf>, legacy: bool }`.
`ConfigWarning { path, line: Option<usize>, key: Option<String>, message }`; `message` always says
what was expected ("expected one of auto, native, osc52, stdout").

Rules: unknown key → warning, ignored; wrong type/value → warning, default used for that key;
whole file unparseable → one warning, all defaults; each problem produces exactly one warning
(SC-005), also when the same file is read twice in one run.

## ClipboardMethod (`kb::platform::clipboard`)

```
Method::Native(NativeTool{name, args})   // pbcopy, wl-copy, xclip, xsel
Method::Custom(String)                   // clipboard.command
Method::Osc52
Method::Stdout
```

`chain(env, &ClipboardSettings, tools: &[NativeTool]) -> Vec<Method>` (R4).

## CopyOutcome (replaces the M3 `Copied/TooLarge/Failed` enum)

```
CopyOutcome {
    used: Option<Method>,            // what delivered the text; None = refused for size
    bytes: usize,                    // text bytes after trimming
    lines: usize,
    confirmed: bool,                 // true for Native/Custom (exit 0), false for Osc52 and Stdout
    attempts: Vec<Attempt>,          // every method tried before `used`, in order
}
Attempt { method: Method, reason: String, hint: Option<String> }   // why it did not deliver
```

`outcome.message(id) -> String` builds the one-line confirmation (R12); `outcome.warnings() ->
Vec<String>` builds the `kb: …` lines; `outcome.sent()` is `used.is_some()` and decides whether a
use is recorded. `Stdout` is the last resort and does not fail, except when the only reason it was
reached is a size refusal (R7): then `used` is `None`.

## Resolution (`kb_core::resolve`)

```
Resolution::Resolved(EntryId)
Resolution::Ambiguous(Vec<Candidate{id, title, score}>)     // up to 5
Resolution::NotFound
```

## Check (`kb::doctor`)

```
Check { id: &'static str, status: Status::{Pass, Warn, Fail}, summary: String, hint: Option<String> }
```

## Platform (`kb::platform::Platform`)

The trait is in [contracts/platform.md](contracts/platform.md); the data it exchanges is the types
above plus `Opened::{Launched, Printed}` for the opener.

## State changes

None persistent except: `config init` creates the file; `config set` rewrites it; `copy`, Enter
and Ctrl-Y record a use through the existing `Kb::record_use` (R12 rule).

## Small types the tasks refer to

- `Flags { root: Option<PathBuf>, clipboard_method: Option<ClipboardMethodChoice> }` — the values
  given on the command line that map to settings (`--root`, `kb copy --method`); passed to
  `config::load` so they win over env and file and are reported as `Source::Flag`.
- `CopyOutcome::message_short() -> String` — the key-bar form for Ctrl-Y: `Copied via <tool>`,
  `Sent via terminal (OSC 52)`, `Printed to stdout`, or the size-refusal text trimmed to one line.
  `message(id)` is the full stderr line with the entry ID and line count.
- `PostPick::{Copy, Print(PrintMode), Show(ShowOptions)}` — what `main` does with the ID the TUI
  returns (`ShowOptions` = raw/json/file/no-color as given to `kb show`).
