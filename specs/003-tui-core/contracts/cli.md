# Contract: `kb`, `kb tui`, `kb show`, `kb edit`

Global flags, root resolution and exit-code base are in
[M1's CLI contract](../../001-entry-format/contracts/cli.md#global). New commands print errors the
same way (`kb: <message>` on stderr, `{"error": {...}}` on stdout with `--json`).

## `kb [--print <mode>] [query...]` and `kb tui [--print <mode>] [query...]`

Opens the TUI. The two forms are identical; `kb tui` exists so a query that starts with a subcommand
name (`kb tui lint`) can be typed.

- `query...`: the words become the initial text of the search line, joined with spaces. Filter
  syntax (`type:sql`, `-tag:x`) is accepted; `-tag:value` works anywhere on the line (M2's rewrite).
  A first word that is a subcommand name runs that subcommand, as always.
- `--print <mode>`: what Enter writes to stdout. Modes: `payload` (default), `id`, `path`, `json`.
  Defined in [Pick output](#pick-output).
- **When the TUI opens** (FR-001): when stdin is a terminal, or `/dev/tty` can be opened. A
  redirected stdout does not matter. When neither is available, kb prints the help text to stderr and
  exits 2 without starting anything.
- **Where it draws**: on `/dev/tty`, never on stdout. While the TUI is open nothing is written to
  stdout.
- **stdout after exit**: the pick output only, byte for byte as `kb show --print <mode>` would print
  it: no added newline for `payload`, `id`, `path`; `json` is one document and a newline.
  Cancelling prints nothing.

### Exit codes

| Code | When |
|------|------|
| 0 | an entry was picked |
| 130 | cancelled: Esc on an empty query, or Ctrl-C |
| 129 / 143 | SIGHUP / SIGTERM received (terminal restored first) |
| 2 | usage or configuration error, I/O error, no terminal available, a panic (the terminal is restored first and the panic message printed on the normal screen) |

`--json` has no meaning for the interactive command and is not accepted (`--print json` is the
machine-readable pick). Errors that happen before the UI starts follow the usual error format.

### Configuration

`theme` and `mouse` in the config file: [config.md](config.md). Unknown keys are ignored.

## Pick output

| Mode | `sql`, `sumo`, `cw`, `cmd`, `prompt` | `link` | `code` |
|------|---------------------------------------|--------|--------|
| `payload` | the payload text, `{{variables}}` unresolved (resolution is M5) | the URL | `path:line` as written in the entry (`code.path:code.line`) |
| `id` | the entry ID | same | same |
| `path` | absolute path of the entry file | same | same |
| `json` | [entry.schema.json](entry.schema.json) document | same | same |

The payload is the text M1 reports (escaped `\{{` already turned into `{{`), with the file's line
endings turned into `\n`, and with no trailing newline added beyond what the code block contains.

## `kb show <id> [--print <mode>]`

Prints exactly what picking `<id>` in the TUI would print, without the UI and without recording a
use. Default mode `payload`.

- Exit 0 on success; 2 when the ID does not exist, is not a valid ID, or the file can't be read
  (`kb: no entry with ID "x"`).
- `--json` is accepted as a synonym for `--print json`.

## `kb edit <id>`

Opens the entry's file in `$VISUAL`, else `$EDITOR`, else `vi`, with the terminal attached, and
exits with 0 when the editor exits 0 and with 1 when the editor exits with an error. If the file has
lint errors afterwards, prints the first one to stderr (`path:line: error[KBxxx]: message`) and
still exits 0. Exit 2 when the ID does not exist or the
editor cannot be started. This is the scriptable form of Ctrl-E; it records no use.

## Not a terminal

Running `kb` with no subcommand and no way to reach a terminal prints the help to stderr and exits 2.
`kb search`, `kb show`, `kb lint` and the other commands never need a terminal.

## Documentation

`README.md` gains a "Using the TUI" section (keys, `--print`, `kb | pbcopy`, config) in the same change.
