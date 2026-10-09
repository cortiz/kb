# kb

A personal, serverless knowledge base for reusable operational snippets: SQL, Sumo Logic and
CloudWatch queries, shell commands, prompts, links and code locations. Each snippet is a plain
Markdown file you can edit by hand, and `kb` is a single terminal binary that checks and searches
them.

> Status: milestone M3 (entry format, `kb lint`, `kb search`, the terminal UI). Variables at use
> time, native clipboard and actions arrive in later milestones.

## Building

```sh
cargo build --release      # the binary is target/release/kb
```

## Getting started

```sh
kb init                 # create ~/kb (or the configured root) with one example per type
kb init ~/notes/kb      # or somewhere else
kb init --no-examples   # just the layout
kb lint                 # check every entry
```

`kb init` creates the KB root, a `.kb/` directory for kb's own data, and `.kb/.gitignore`
(containing `*`) so git never picks that data up. It writes nothing else to the root. On a KB
that already exists it only adds what is missing and never changes or overwrites a file, and it
adds example entries only to a KB that has none yet. Copy an example to start a new entry, then
run `kb lint` to check it.

```text
~/kb
├── .kb/
│   └── .gitignore
├── sql/example-active-connections.md
├── sumo/example-5xx-by-authorizer.md
└── …
```

Folders are a convention, not a rule: an entry's type comes from its `type:` field.

## The KB root

Entries live under one directory, the **KB root**. kb looks for it in this order:

1. `--root <path>`
2. the `KB_ROOT` environment variable
3. `root:` in the config file: `~/.config/kb/config.yml` on Linux (or under `$XDG_CONFIG_HOME`),
   `~/Library/Application Support/kb/config.yml` on macOS
4. `~/kb`

```yaml
# config.yml
root: ~/notes/kb
```

## Entry format

An entry is one UTF-8 Markdown file ending in `.md`, anywhere under the KB root. Its **ID** is its
path without `.md`: `sumo/5xx-by-authorizer.md` is `sumo/5xx-by-authorizer`. Files and folders
whose names start with `.` are ignored, and so is everything that isn't `.md`.

````markdown
---
kb_format: 1
type: sumo
title: 5xx by authorizer
tags: [lambda, auth, prod]
range: "-{{minutes}}m"
vars:
  env: { kind: enum, options: [prod, stage, dev], default: prod }
  minutes: { kind: number, default: 15 }
---

```
_sourceCategory={{env}}/lambda/authorizer
| where status >= 500
| timeslice 1m
| count by _timeslice, errorCode
```

A spike in TOKEN_EXPIRED usually means the role cache TTL drifted.
````

The file starts with YAML frontmatter between `---` lines, followed by a Markdown body.

### Frontmatter

| Key | Required | Value |
|-----|----------|-------|
| `kb_format` | yes | `1` |
| `type` | yes | `sql`, `sumo`, `cw`, `cmd`, `prompt`, `link` or `code` |
| `title` | yes | Text, at most 120 characters |
| `tags` | no | A list; read in lowercase, without a leading `#` |
| `vars` | no | Variable definitions (below) |
| `created`, `updated` | no | `2026-10-08` or `2026-10-08T14:30:00Z` |

Type-specific keys:

| Type | Keys | Payload |
|------|------|---------|
| `sql` | `dialect`, `database` | yes |
| `sumo` | `range` | yes |
| `cw` | `log_groups` (list), `range`, `region` | yes |
| `cmd` | `shell` (`sh` default, `bash`, `zsh`, `fish`), `cwd` | yes |
| `prompt` | — | yes |
| `link` | `url` (required, `http` or `https`) | no |
| `code` | `code:` with `repo` (`owner/name` or a git URL), `path`, `line`, `ref`, optional `symbol` and `end_line` | no |

Other keys are kept as they are and reported as info. kb accepts plain YAML: no anchors,
aliases, explicit tags or duplicate keys.

### Body

For `sql`, `sumo`, `cw`, `cmd` and `prompt`, the **payload** is the first fenced code block in the
body. Use a longer fence (four backticks, or tildes) to put code blocks inside a prompt. Everything
else in the body is **notes**. For `link` and `code`, the whole body is notes, and it may be empty.

### Variables

```yaml
vars:
  env:     { kind: enum, options: [prod, stage, dev], default: prod }
  minutes: { kind: number, default: 15, description: Look-back window }
  user:    { kind: text }
  token:   { kind: secret, env: API_TOKEN }
```

- `kind` is `text`, `number`, `enum` or `secret`. `enum` needs `options`.
- Reference a variable as `{{name}}` (lowercase letters, digits and `_`) in the payload or in
  `range`, `url`, `cwd` and `log_groups`.
- Write `\{{` for a literal `{{`.
- A reference that isn't declared is treated as `text` with no default.

### Secrets

Entries must never hold credentials. Use a `secret` variable that reads `env` (or is prompted for),
and never give it a `default`. `kb lint` rejects AWS keys, bearer tokens, JWTs, private keys and
high-entropy values assigned to names like `password` or `token`. If a line only looks like a
secret, end it with `kb:allow-secret` (in whatever comment syntax the payload uses); every
suppression is listed in the lint output.

## `kb lint`

```sh
kb lint                     # check the whole KB
kb lint sumo/ cmd/x.md      # check some directories or files
kb lint --strict            # warnings fail too
kb lint --quiet             # summary line only
```

Paths are tried relative to the current directory first, then relative to the KB root. Output
lists each finding with its file, line, column, rule code and a suggested fix, then a summary:

```text
sumo/bad.md:3:7: error[KB001-unknown-type]: unknown type "sumologic"; expected one of sql, sumo, cw, cmd, prompt, link, code
  fix: use "sumo"

284 entries, 1 error, 0 warnings
```

Exit codes: `0` no errors (and no warnings with `--strict`), `1` findings failed the run, `2` usage
or I/O error, such as a missing KB root. Every rule code is listed in
[`specs/001-entry-format/contracts/lint-rules.md`](specs/001-entry-format/contracts/lint-rules.md).

## Searching

```sh
kb search 5xx auth                    # every word must match, in any field
kb search latency type:cw tag:prod    # filters narrow the results
kb search '"role cache"'              # exact phrase (quote it for the shell too)
kb search auth --ids | fzf            # only IDs, one per line
kb search auth --json                 # ranges and snippets, for scripts and the TUI
kb search                             # everything, most used first
```

| Syntax | Meaning |
|--------|---------|
| `word` | Matches as a word prefix (`auth` finds `authorizer`), ignoring case and accents. Words of 5+ characters also match one typo (`authroizer`), ranked below exact matches. |
| `"two words"` | Exact phrase. |
| `type:cw` | Entry type; repeat for "any of". Same as `--type cw`. |
| `tag:prod` / `-tag:stage` | Has / lacks a tag; several `tag:` must all be present. Same as `--tag`, `--not-tag`. |
| `var:region` | Uses the variable `region`. |
| `repo:acme/api` | A `code` entry in that repo. |
| `in:title`, `in:payload`, `in:notes` | Restrict the free text to one field. |

Results are ordered by relevance (title, then tags, ID, variables and type-specific fields, payload,
notes), with entries you use often and recently ranked a little higher; an exact title match always
comes first. `--limit N` caps the results (default 20, `0` for all). Exit codes: `0` results, `1`
none (`kb: no matches` on stderr), `2` error. Entries with lint errors are left out, and each search
says how many (`run kb lint`).

The index lives in `.kb/index.db` and is brought up to date on every search: edit, rename or delete
a file and the next search sees it, with no command to run. It is disposable: delete `.kb/` any time
and it is rebuilt (you only lose usage counts in `.kb/state.json`). `kb index status` shows what is
indexed, and `kb reindex` rebuilds it from scratch. Both are git-ignored through `.kb/.gitignore`,
which kb creates whenever it creates `.kb/`.

## Using the TUI

```sh
kb                      # open the UI; type to search, Enter to pick
kb 5xx auth             # open it with the search line already filled in
kb | pbcopy             # the pick goes to stdout, so it pipes (macOS; use xclip or wl-copy on Linux)
x=$(kb --print id)      # --print: payload (default), id, path or json
kb show sumo/5xx-by-authorizer     # what Enter prints for that entry, without the UI
kb edit sumo/5xx-by-authorizer     # what Ctrl-E does, without the UI
```

The list updates on every keystroke from the same search as `kb search` (so `type:cw`, `tag:prod `,
`-tag:stage `, `"exact phrase"` all work; a filter followed by a space becomes a chip, and
Backspace on an empty line removes the last one). Tab and Shift-Tab cycle the type filter. The right
side previews the selected entry: syntax-highlighted payload with line numbers, `{variables}`
marked and listed, type-specific fields, notes rendered from Markdown.

Enter prints the entry and exits (it also counts as a use, which ranks the entry higher next time).
Until the native clipboard arrives, Enter does not copy; pipe it, or press Ctrl-Y to ask the terminal
to copy the payload (OSC 52: works in most modern terminals, over SSH, and in tmux with
`set -g set-clipboard on`). Esc clears the search line, and on an empty one exits with code 130
without printing anything. The UI is drawn on the terminal, never on stdout, so redirecting stdout
is safe; with no terminal at all, `kb` prints the help and exits 2.

| Key | Action |
|-----|--------|
| `Enter` | pick: print the entry and exit |
| `Ctrl-Y` | copy the payload (terminal clipboard) |
| `Ctrl-E` | edit the entry in $VISUAL / $EDITOR |
| `↓  Ctrl-N  Ctrl-J` | select the next result |
| `↑  Ctrl-P  Ctrl-K` | select the previous result |
| `PgDn` | down one page |
| `PgUp` | up one page |
| `Ctrl-End` | last result |
| `Ctrl-Home` | first result |
| `Tab` | next type filter |
| `Shift-Tab` | previous type filter |
| `Shift-↓` | scroll the preview one line down |
| `Shift-↑` | scroll the preview one line up |
| `Shift-PgDn` | scroll the preview one page down |
| `Shift-PgUp` | scroll the preview one page up |
| `Alt-P` | full-screen preview (narrow terminals) |
| `←` | cursor left |
| `→` | cursor right |
| `Home  Ctrl-A` | start of the line |
| `End` | end of the line |
| `Backspace` | delete before the cursor; on an empty line remove the last filter chip |
| `Delete` | delete at the cursor |
| `Ctrl-W` | delete the previous word |
| `Ctrl-U` | clear the line |
| `Ctrl-L` | refresh from disk |
| `?` | this help (when the text line is empty) |
| `F1` | this help, always |
| `Esc` | clear the query; exit when it is empty |
| `Ctrl-C` | exit immediately |
| `` |  |
| `` |  |
| `` |  |
| `` |  |
| `` |  |
| typing | edit the query |

Terminals that keep Shift-PgUp/PgDn or Ctrl-Home/End for themselves never send them to kb; use
PgUp/PgDn, Shift-↑/↓ and Alt-P instead. Below 100 columns only the list shows and Alt-P zooms the
preview; below 60×15 kb shows "Terminal too small" until you resize.

Settings in the config file (`theme: auto|dark|light`, `mouse: true|false`) and `NO_COLOR` are
described in [`specs/003-tui-core/contracts/config.md`](specs/003-tui-core/contracts/config.md).
`theme: auto` reads `COLORFGBG` and otherwise assumes a dark background.

## JSON output

Every command takes `--json` and prints a single JSON document, so kb composes with `jq`, `fzf` and
scripts:

```sh
kb lint --json | jq -r '.findings[] | select(.severity == "error") | "\(.path):\(.line) \(.code)"'
kb lint --json | jq '.summary'
kb init ~/kb --json | jq '.created'
kb search auth --json | jq -r '.results[] | "\(.id)\t\(.title)"'
```

The formats are defined in
[`lint-report.schema.json`](specs/001-entry-format/contracts/lint-report.schema.json) and
[`init-report.schema.json`](specs/001-entry-format/contracts/init-report.schema.json), and for
search by
[`search-report.schema.json`](specs/002-index-search/contracts/search-report.schema.json) and
[`index-status.schema.json`](specs/002-index-search/contracts/index-status.schema.json). On a usage or
I/O error (exit code `2`), `--json` prints `{"error": {"kind": "...", "message": "..."}}`.

kb never changes an entry while reading it: parsing then writing an unmodified file reproduces it
byte for byte, including comments, quoting and line endings.
