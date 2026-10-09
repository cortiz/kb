# Contract: commands

Exit codes for all new commands (FR-023): `0` success; `1` not found, ambiguous, or a failed
doctor check; `2` usage or I/O error, including a copy refused for size (no other method could carry it). Every command accepts `--json` (constitution IV); errors in
`--json` mode use the M1 `{"error": {"kind", "message"}}` shape on stderr.

Stdout carries only the command's result; confirmations, warnings and hints go to stderr.

## `kb copy <id|query...> [--method auto|native|osc52|stdout] [--json]`

1. Resolve the argument (research R10). `NotFound` → exit 1, `no entry matches "<text>"`.
   `Ambiguous` → terminal: open the TUI with the query; no terminal: list up to 5 `id<TAB>title`
   lines on stderr, exit 1, nothing copied.
2. Get the pick output (M3 `PrintMode::Payload`: payload / URL / `path:line`), trim trailing
   newlines if `clipboard.trim_trailing_newline`.
3. `clipboard::copy` with the settings (`--method` overrides `clipboard.method` for this run).
4. Record a use if the text was sent by any method.
5. Print the confirmation (R12) on stderr. If the method that delivered is `stdout`, the text is
   printed to stdout (no added newline) and the warning lines say why.

Exit `0` after delivery by any method, including the stdout fallback. Exit `2` when refused for
size (research R7): nothing printed, nothing recorded, stderr names the size, the limit and
`kb show --raw <id> | <tool>`.

`--print` is a usage error (clarification 3).

If resolution is ambiguous and the TUI opens, Enter there **copies** (not `tui.enter`): the TUI
returns the picked ID and `kb copy` then performs steps 2 to 5 on it.

`--json` prints on stdout:

```json
{"id": "sumo/5xx-by-authorizer", "method": "osc52", "confirmed": false,
 "bytes": 212, "lines": 6,
 "attempts": [{"method": "wl-copy", "reason": "not found", "hint": "install wl-clipboard"}]}
```

With `--json` and the stdout fallback, the text is in a `"text"` field instead of being printed raw.

## `kb show <id|query...> [--raw | --json | --file] [--no-color]`

Resolution as `copy`. Default output is the preview as text (R9). `--raw` is the payload only,
`--json` the entry JSON (M3 `--print json` shape), `--file` the absolute path, one line.
**`--print` is removed from `show`** (usage error, exit 2; clarification 3). `--raw`, `--json` and
`--file` replace M3's `payload`, `json` and `path`; `id` is a field of the JSON. If resolution is
ambiguous and the TUI opens, Enter prints what `show` would for the picked entry (same flags).
The modes are
mutually exclusive (clap `conflicts_with`). Color only when stdout is a terminal, `NO_COLOR` is
unset and `--no-color` is absent. `show` does not record a use.

Ambiguous / not found: as `copy`.

## `kb config init | path | get <key> | set <key> <value> | list`

| Subcommand | Behavior | Exit |
|---|---|---|
| `init` | write the commented default file at `config_write_path`; never overwrite; print the path on stdout (`--json`: `{"path", "created": true}`) | `0`; existing file → `1` with `config file already exists: <path>`; unwritable directory → `2` |
| `path` | print the path kb would write (`config_write_path`); with `--read` print every candidate, marking `(in use)` and `(exists, not used)`; `--json`: `{"path", "candidates": [{path, exists, in_use}]}` | `0` |
| `get <key>` | print the effective value (all sources), nothing else | `0`; unknown key → `2` |
| `set <key> <value>` | validate against the setting's kind, edit the file preserving comments/order; create the file with just that key if absent (no full template) | `0`; invalid value or unknown key → `2`; unwritable → `2` |
| `list` | every setting: `key = value  (source)` aligned; `--json`: array of `{key, value, source, file, line}` | `0` |

When the new file would hide another one that was in use (macOS: the `~/Library` file), `set` first copies that file's content and says so on stderr. `set` with an env var or flag overriding the key succeeds and prints a note on stderr (`KB_THEME
is set and overrides this value`).

`config init` template: one commented line per setting, grouped by section, with the default and
the doc string. Values are commented out (the spec's US3.2 now says so), so a future change to a default reaches
the user and the file shows only what they changed; `kb config list` shows effective values.

```toml
# kb configuration. Remove the leading # to change a setting.

# Where your entries live.
# root = "~/kb"

[clipboard]
# auto | native | osc52 | stdout
# method = "auto"
```

A task tests `init` then `set clipboard.method osc52`: the new key must land sensibly under
`[clipboard]` with the comments kept (`toml_edit` stores comments as decor, so placement is
checked, not assumed).

## `kb doctor [--copy-test] [--json]`

Text format (stdout):

```
kb 0.1.0 on macOS (arm64)
 ✓ root           /Users/me/kb (writable, 31 entries)
 ✓ index          current, 31 entries
 ✓ config         /Users/me/.config/kb/config.toml
 ⚠ clipboard      osc52 — Terminal.app ignores OSC 52
     fix: use iTerm2, kitty or Alacritty, or set clipboard.method = "native"
 …
```

`✓` pass, `⚠` warn, `✗` fail on a terminal; plain `ok`, `warn` and `FAIL` when stdout is piped or `NO_COLOR` is set.
Exit `1` if any check failed, else `0`. Check ids, statuses and hints: [doctor.md](doctor.md).

## TUI-facing changes (no new command)

- `kb [tui] [query…] [--print MODE]`: `--print` is optional. Omitted → `tui.enter` decides.
- The TUI returns the picked entry's ID (`Outcome::Picked { id }`); `main` then runs the
  post-pick action: `Copy` (default, and always when opened by `kb copy`), `Print(mode)` (`--print`
  or `tui.enter = "print"`), or `Show(flags)` (when opened by `kb show`). The TUI's `Options` do
  not change; `Outcome` does (M3 returned the finished output string).
- Ctrl-Y: copy and stay open. Ctrl-E: editor from the platform layer.
- Header shows `⚠ config: N problems (run kb doctor)` when load warnings exist; the full text is
  printed on stderr after the TUI exits.
