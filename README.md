# kb

A personal, serverless knowledge base for reusable operational snippets: SQL, Sumo Logic and
CloudWatch queries, shell commands, prompts, links and code locations. Each snippet is a plain
Markdown file you can edit by hand, and `kb` is a single terminal binary that checks and searches
them.

![kb demo: search, fill in variables, copy, vim mode, add from the clipboard, lint](demo.gif)

> Status: milestone R1 (011-runbook-format: ordered sequence of snippet steps, variable lifting, reference integrity, checklists).

## Install

### Quick install (Linux & macOS)

Install the latest release directly into `~/.local/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/cortiz/kb/main/install.sh | sh
```

Or with `wget`:

```sh
wget -qO- https://raw.githubusercontent.com/cortiz/kb/main/install.sh | sh
```

The installer:
- Detects your operating system and architecture (`linux-x86_64`, `linux-arm64`, `macos-apple-silicon`, `macos-intel`)
- Downloads the matching release archive and verifies its SHA-256 checksum
- Installs the `kb` binary into `~/.local/bin`
- Prompts to configure your `$PATH` in `~/.bashrc`, `~/.zshrc`, or your shell profile if `~/.local/bin` is not yet in your PATH
- Installs the `kb-update` helper into `~/.local/bin/kb-update`

Customizing installation:

```sh
# Install to a custom directory without modifying shell config
curl -fsSL https://raw.githubusercontent.com/cortiz/kb/main/install.sh | sh -s -- --bin-dir /usr/local/bin --no-modify-path

# Install a specific release version non-interactively
curl -fsSL https://raw.githubusercontent.com/cortiz/kb/main/install.sh | sh -s -- --version v0.2.0 -y
```

### Updating kb

To check for and install updates at any time, run:

```sh
kb-update
```

Options:
- `kb-update --check`: check if an update is available without downloading
- `kb-update --force`: force reinstall the latest version
- `kb-update --version <tag>`: install a specific release version

Or update directly via curl:

```sh
curl -fsSL https://raw.githubusercontent.com/cortiz/kb/main/scripts/update.sh | sh
```

### Manual download

Download the archive for your machine from the
[releases page](https://github.com/cortiz/kb/releases) together with `SHA256SUMS`, then:

```sh
shasum -a 256 -c --ignore-missing SHA256SUMS      # sha256sum -c --ignore-missing on Linux
tar -xzf kb-<version>-<platform>.tar.gz
./kb-<version>-<platform>/kb --version
```

Platforms: `macos-apple-silicon` (M1 and newer), `macos-intel`, `linux-x86_64` and `linux-arm64`
(the Linux builds are static, no libraries needed). macOS warns about a downloaded binary
because it is not notarized; run `xattr -d com.apple.quarantine kb` once, or install with Homebrew.

Homebrew (stable releases only):

```sh
brew tap cortiz/kb https://github.com/cortiz/kb
brew trust cortiz/kb
brew install kb
```

> **Note on Homebrew trust**: Modern Homebrew (v4.4+) requires explicitly trusting non-official taps before loading third-party formulas. If Homebrew prints:
> ```text
> Error: Refusing to load formula cortiz/kb/kb from untrusted tap cortiz/kb.
> ```
> Run `brew trust cortiz/kb` (or `brew trust --formula cortiz/kb/kb`), then re-run `brew install kb`.

Or build it yourself: `cargo install --git https://github.com/cortiz/kb kb`. Contributing and the
release process are in [CONTRIBUTING.md](CONTRIBUTING.md).

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
3. `root` in the [config file](#configuration)
4. `~/kb`

```toml
# ~/.config/kb/config.toml
root = "~/notes/kb"
```

## Configuration

kb works with no config file. `kb config init` writes a commented one with every setting and its
default (never overwriting an existing file); `kb config set clipboard.method osc52` changes one
value and keeps your comments and order.

| Platform | Config file |
|----------|-------------|
| Linux | `$XDG_CONFIG_HOME/kb/config.toml`, else `~/.config/kb/config.toml` |
| macOS | `~/.config/kb/config.toml`, else `~/Library/Application Support/kb/config.toml` |

On macOS the first file that exists wins (they are never merged), and `kb config init` and
`kb config set` always write `~/.config/kb/config.toml`. `kb config path --read` shows which file
is in use.

```sh
kb config init                      # write the commented default file; prints its path
kb config path [--read]             # where it is written (and everything that is read)
kb config get theme                 # one value
kb config set tui.enter print       # change one value, keeping comments and order
kb config list                      # every setting, its value and where the value comes from
```

| Setting | Values | Default |
|---------|--------|---------|
| `root` | a path | `~/kb` |
| `editor` | a command, arguments allowed | `$VISUAL`, then `$EDITOR`, then `vi` |
| `path_template` | text with `{type}`, `{slug}` (required) and `{tag}` | `{type}/{slug}` |
| `theme` | `auto`, `dark`, `light` | `auto` |
| `tui.enter` | `copy`, `print`, `primary` | `copy` |
| `tui.mouse` | `true`, `false` | `false` |
| `tui.keymap` | `emacs`, `vim` | `emacs` |
| `clipboard.method` | `auto`, `native`, `osc52`, `stdout` | `auto` |
| `clipboard.command` | a command that reads the text on stdin | none |
| `clipboard.osc52_max_bytes` | a whole number, at least 1 | `74000` |
| `clipboard.trim_trailing_newline` | `true`, `false` | `true` |
| `opener.command` | a command that gets the URL or file | `open` (macOS), `xdg-open` (Linux) |
| `vars.remember` | `true`, `false` | `true` |
| `sumo.endpoint_url` | Sumo Logic base URL | none |
| `aws.default_region` | AWS region for CloudWatch console | `$AWS_REGION`, `$AWS_DEFAULT_REGION`, else `us-east-1` |
| `code.search_paths` | list of paths scanned for clones | `["~/src", "~/repos", "~/code", "~/projects"]` |
| `code.cache_ttl_seconds` | clone discovery cache TTL | `86400` (24h) |
| `code.fetch_on_check` | auto-fetch before drift check | `false` |
| `run.dangerous_patterns` | list of regex patterns triggering confirmation | `["rm -rf", "DROP TABLE", ...]` |
| `run.require_confirmation`| `true`, `false` | `true` |
| `widget.key` | key combination to open picker | `\ek` (`Alt-K`) |
| `widget.save_key` | key combination to save last command | `\eK` (`Alt-Shift-K`) |
| `widget.default_type` | `cmd`, `all` | `cmd` |
| `widget.buffer` | `query`, `insert` | `query` |
| `widget.height` | `<N>%` or `<N>` lines | `40%` |
| `widget.preview` | `true`, `false` | `true` |

Custom actions are defined under `[[actions.<type>]]` tables (e.g. `[[actions.cmd]]`, `[[actions.sql]]`), specifying `name`, `command`, optional `key`, `input` (`"stdin"` or `"arg"`), `description`, and `confirm`. Local clone paths can be mapped explicitly via `[code.repos]` (e.g. `"org/repo" = "~/repos/repo"`).

A command-line flag beats an environment variable (`KB_<SECTION>_<KEY>`, for example
`KB_CLIPBOARD_METHOD` or `KB_ROOT`), which beats the file, which beats the default. An unknown key
or a bad value never stops kb: it warns once with the file and line and uses the default. The
older `config.yml` (keys `root`, `theme`, `mouse`) is still read when there is no `config.toml`,
with a warning; `mouse` is now `tui.mouse`. The full contract is in
[`specs/004-platform-clipboard/contracts/config.md`](specs/004-platform-clipboard/contracts/config.md).

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
| `type` | yes | `sql`, `sumo`, `cw`, `cmd`, `prompt`, `link`, `code` or `note` |
| `title` | yes (a `note` may take it from its first `# heading`) | Text, at most 120 characters |
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
| `note` | — (no `vars`) | no: the whole body is Markdown |
| `code` | `code:` with `repo` (`owner/name` or a git URL), `path`, `line`, `ref`, optional `symbol` and `end_line` | no |

Other keys are kept as they are and reported as info. kb accepts plain YAML: no anchors,
aliases, explicit tags or duplicate keys.

### Body

For `sql`, `sumo`, `cw`, `cmd` and `prompt`, the **payload** is the first fenced code block in the
body. Use a longer fence (four backticks, or tildes) to put code blocks inside a prompt. Everything
else in the body is **notes**. For `link` and `code`, the whole body is notes, and it may be empty.

### Notes

A `note` is free-form Markdown (an incident write-up, a decision, a how-to) in `note/<slug>.md`.
`kb note "Role cache TTL" --tag redis` opens your editor on a template; `--private` adds the
`private` tag, which keeps the note away from AI agents. With piped input or `--yes` it saves
without asking (`kb note --yes < notes.md`); `kb add --type note` does the same, and pasted
Markdown with headings, lists or tables is detected as a note.

```markdown
---
kb_format: 1
type: note
tags: [auth, redis]
---

# Role cache TTL

See [[sumo/5xx-by-authorizer]] for the error rate.

| Event | TTL |
|:--|--:|
| Role granted | 300 s |

> [!WARNING]
> Never go below 60 s during an incident.
```

`kb show` and the preview pane draw headings, lists, task lists, tables (wrapped to the width),
GitHub callouts (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`), framed and
highlighted code, footnotes and strikethrough. Images show as `[image: alt] (path)`, HTML is shown
as text, and `kb show --raw` prints the Markdown without frontmatter.

- **Entry links**: `[[sumo/5xx-by-authorizer]]` shows as `[sumo] 5xx by authorizer`, or `[?] id`
  when the entry does not exist (`kb lint` warns, KB074). Write `\[[` for a literal `[[`.
  *Follow entry link* in the action menu (`Alt-A`, `f`) goes to the entry, or lists them when
  there are several. `kb mv` points the links at the new ID in the same commit, and `kb rm`
  names the notes that link to what you delete.
- **Terminal safety**: escape sequences and control characters in a note, a title or a tag never
  reach your terminal (they show as `␛`; `--raw` drops them). Only `http`, `https` and `mailto`
  links become terminal hyperlinks, only in `kb show`, only where the terminal supports them
  (`FORCE_HYPERLINK=1` or `0` overrides).
- **Search**: headings count more than body text, `in:body` searches the body only, and typo
  matching ignores body words. A note over 64 KB gets a lint warning (KB072); `{{x}}` is plain
  text and `vars:` is an error (KB071).
- **Agents**: `search`, `get`, `create` and `update` work on notes. `get` returns the Markdown as
  `content`; `create` and `update` take `content`, and a title may come from its first heading.

### Runbooks

A `runbook` entry is an ordered sequence of steps that point to existing entries or carry manual instructions, with variables declared once for the whole runbook in `runbook/<slug>.md`.

```yaml
---
kb_format: 1
type: runbook
title: Deploy authorizer to prod
tags: [deploy, lambda, auth]
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  version: { kind: text, description: "Release tag, e.g. v2.14.0" }
steps:
  - id: baseline
    title: Check the current error rate
    entry: sumo/5xx-by-authorizer
    vars: { minutes: 30 }
  - id: deploy
    title: Deploy {{version}} to {{env}}
    entry: cmd/deploy-authorizer
    confirm: true
  - id: rollback
    title: Roll back if p99 > 800 ms
    do: |
      Only if p99 stayed above 800 ms for 5 minutes.
      Post in #auth-oncall before rolling back.
    optional: true
---

Owner: platform security.
```

- **Creation**: `kb runbook new "Deploy authorizer" sumo/5xx cmd/deploy` creates a runbook from existing entries, automatically lifting compatible variables to the runbook. You can also mark entries in the TUI (`Space` or `m`) and run `new-runbook-from-marks` (`C-R`).
- **Integrity**: `kb mv` updates step references across runbooks in the same commit. `kb rm` refuses to delete entries used in runbooks unless `--force` is passed.
- **Checklists**: `kb show <runbook> --checklist` exports a Markdown checklist. With `--resolve`, it prompts once for variables and inlines resolved query/command code blocks with secrets safely masked. `kb use <runbook>` copies the resolved checklist to the clipboard and records usage.
- **Search & Filters**: `type:runbook` lists only runbooks. `runbook:<id>` lists the entries used by that runbook in step order. `used-by:runbook` lists all entries referenced by any runbook. Ordinary entries display a "Used in" section in preview showing referencing runbooks.

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
- `escape` (optional) says how a value is quoted where it lands: `auto` (the default: shell quoting
  in a `cmd` payload and in `cwd`, percent-encoding in a `url`, as typed everywhere else), `shell`
  (always quote for the shell), `sql` (a single-quoted SQL string) or `none` (exactly as typed).
  See [Filling in variables](#filling-in-variables).

### Secrets

Entries must never hold credentials. Use a `secret` variable that reads `env` (or is prompted for),
and never give it a `default`. `kb lint` rejects AWS keys, bearer tokens, JWTs, private keys and
high-entropy values assigned to names like `password` or `token`. If a line only looks like a
secret, end it with `kb:allow-secret` (in whatever comment syntax the payload uses); every
suppression is listed in the lint output.

## Filling in variables

Copying an entry that has variables asks for their values first, in the UI (a small form with a
live preview of the result) or on the command line:

```sh
kb use 5xx --var env=prod --var minutes=30     # no questions: everything given
kb use 5xx                                     # asks for each variable on the terminal
kb use tail-logs -y --print                    # -y: take the pre-filled values; --print: to stdout
kb vars 5xx                                    # what the entry asks for and what would be pre-filled
kb copy --raw 5xx                              # the template itself, {{placeholders}} included
```

`kb copy` does the same as `kb use` for an entry with variables (and is unchanged for one without).
A variable starts with, in this order: `--var`, the environment variable named by its `env`, the value
you used last time **for this entry**, its `default`. Values must fit their kind (`number` is
`30`, `-5` or `0.25`; `enum` is one of its options; `text` is one line); a wrong value says what was
expected. Prompts use the terminal even when stdout is redirected, so `kb use x --print > file` works.
Exit codes: `0` done, `1` a value is missing and nobody can be asked (or no match), `2` a bad
`--var`, `130` cancelled. `--no-remember` (or `vars.remember = false`) stops kb remembering values;
kb never remembers a `secret` or a value that looks like a credential, and keeps the last 20 values
per variable name in `.kb/state.json` to suggest them.

**Quoting.** Values are quoted for where they land, so `feature/x; rm -rf ~` becomes one argument:
in a `cmd` payload (and `cwd`) for the entry's `shell` (`sh`, `bash`, `zsh` or `fish`), by the quotes
the template already has around the variable; in a `url`, percent-encoded. In a `sql`, `sumo`, `cw`
or `prompt` payload values are inserted as typed; use `escape: sql` for a string literal.

**Secrets.** A `secret` variable is read from its `env`, or typed with nothing echoed. It is used for
that one run and is never written down: not in `.kb/state.json`, not in `kb vars`, not in the preview
(which shows `••••`). It does end up on the clipboard (or stdout with `--print`), and kb says so;
clipboard managers may keep a history. `--var token=…` works but lands in your shell history. When
the clipboard has nowhere to go and kb falls back to printing (see [Copying](#copying)), the secret is
printed too, on stdout only.

In the UI, Enter on an entry with variables opens the form instead of copying; Ctrl-Y opens it too and
stays open afterwards. Tab and Shift-Tab (or ↓ and ↑) move between fields, ←/→ change a choice,
Enter copies, Alt-Enter prints to stdout and exits, Ctrl-Z resets a field, Ctrl-R replaces a secret
that came from the environment, Alt-↓ opens recent values for a text field, and Esc goes back to the
list (what you typed is kept until you quit). Alt-C in the list copies the template without asking.

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
sumo/bad.md:3:7: error[KB001-unknown-type]: unknown type "sumologic"; expected one of sql, sumo, cw, cmd, prompt, link, code, note
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
| `in:title`, `in:payload`, `in:notes`, `in:body` | Restrict the free text to one field (`in:body` is `in:payload`; for a note it is the Markdown body). |

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
kb                      # open the UI; type to search, Enter copies the payload
kb 5xx auth             # open it with the search line already filled in
x=$(kb --print id)      # --print: print the pick instead of copying (payload, id, path or json)
kb copy 5xx auth        # what Enter does, without the UI
kb show 5xx auth        # what the preview shows, without the UI
kb edit sumo/5xx-by-authorizer     # what Ctrl-E does, without the UI
```

The list updates on every keystroke from the same search as `kb search` (so `type:cw`, `tag:prod `,
`-tag:stage `, `"exact phrase"` all work; a filter followed by a space becomes a chip, and
Backspace on an empty line removes the last one). Tab and Shift-Tab cycle the type filter. The right
side previews the selected entry: syntax-highlighted payload with line numbers, `{variables}`
marked and listed, type-specific fields, notes rendered from Markdown.

Enter copies the payload to the clipboard and exits, with one line on stderr saying how
(see [Copying](#copying)). It counts as a use, which ranks the entry higher next time. Set
`tui.enter = "print"`, or pass `--print payload|id|path|json`, to print the pick on stdout instead,
like `fzf`: `kb --print payload | pbcopy`, `$(kb --print id)`. Ctrl-Y copies and stays open, and
the key bar says how it went for two seconds. Esc clears the search line, and on an empty one
exits with code 130 without printing anything. The UI is drawn on the terminal, never on stdout,
so redirecting stdout is safe; with no terminal at all, `kb` prints the help and exits 2. If the
config has problems, the header shows `⚠ config: N problems (run kb doctor)` and the details are
printed on stderr after the UI closes.

| Key | Action |
|-----|--------|
| `Enter` | pick: copy the entry and exit (or primary action if `tui.enter = "primary"`) |
| `Ctrl-Y` | copy the payload and stay open |
| `Ctrl-O` | open the link, console or code view |
| `Ctrl-G` | run the command safely |
| `Alt-A  F2` | open the action menu |
| `Alt-C` | copy the payload with its `{{variables}}` left in |
| `Ctrl-E` | edit the entry in $VISUAL / $EDITOR |
| `Ctrl-N` | add an entry from the clipboard |
| `Ctrl-R` | rename the entry |
| `Ctrl-X` | delete the entry |
| `↓  Ctrl-J` | select the next result |
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
| `Alt-S` | sync with the git remote (see [Git sync](#git-sync)) |
| `Alt-H` | history of the selected entry |
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
| typing | edit the query |

Terminals that keep Shift-PgUp/PgDn or Ctrl-Home/End for themselves never send them to kb; use
PgUp/PgDn, Shift-↑/↓ and Alt-P instead. Below 100 columns only the list shows and Alt-P zooms the
preview; below 60×15 kb shows "Terminal too small" until you resize.

### The vim keymap and your own keys

`tui.keymap = "vim"` (or `KB_TUI_KEYMAP=vim`, or `kb --keymap vim`) adds a second mode to the list.
kb still starts in **search** mode, where typing edits the query and every key above works. `Esc`
switches to **normal** mode (the header shows `NORMAL` or `SEARCH`), where single letters act on the
selected entry and the query stays as it is. Every Ctrl and Alt key above keeps working in both modes.

| Normal-mode key | Action |
|-----------------|--------|
| `j`  `↓` / `k`  `↑` | select the next / previous result |
| `gg`  `Home` / `G`  `End` | first / last result (`g` waits one second for the second `g`; `Esc` cancels) |
| `Ctrl-D` / `Ctrl-U` | half a page down / up (`PgDn` / `PgUp` move a full page) |
| `l`  `→` / `h`  `←` | next / previous type filter |
| `J` / `K` | scroll the preview down / up (`Shift-↓` / `Shift-↑` too) |
| `p` | full-screen preview |
| `Enter` | pick |
| `y` / `Y` | copy the payload / copy it with its `{{variables}}` left in |
| `o` | open the link, console or code view |
| `!` | run the command (with the usual confirmation) |
| `m`  `.` | open the action menu |
| `e` | edit the entry |
| `n` | add an entry from the clipboard |
| `R` / `D` | rename / delete the entry |
| `s` / `H` | sync / history of the selected entry |
| `/` | back to search mode with an empty query |
| `i`  `a` | back to search mode, keeping the query |
| `?` | this help, listing the keys of both modes |
| `q` | exit |
| `Esc` | clear the query; exit when it is empty |

In search mode `Ctrl-U` still clears the line, and `Home` / `End` move within it.

Every key can be changed, for either keymap, in `[tui.keys.<context>]` tables of the config file.
The contexts are `global` (both list modes), `search`, `normal`, `action-menu`, `code-view`,
`history`, `form` and `add`. A list replaces the command's keys in that context and an empty list
unbinds it; commands you do not mention keep their keys:

```toml
[tui]
keymap = "vim"

[tui.keys.normal]
run = ["x", "!"]

[tui.keys.search]
action-menu = ["ctrl-p", "alt-a", "f2"]

[tui.keys.global]
history = []
```

Keys are written `ctrl-o`, `alt-a`, `shift-down`, `enter`, `esc`, `tab`, `backtab`, `space`,
`backspace`, `delete`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `f1` to
`f12`, or one character (`G` is not `g`). Two keys separated by a space (`"g g"`) make a sequence,
in normal mode only. Plain printable keys cannot be bound in `search`, `form` or `add`, where
typing goes into a text field. A bad line is ignored with a warning naming the file and line, and kb
still starts; `Ctrl-C` always quits, the `y` / `run` answers of confirmation questions cannot be
changed, and each list mode keeps at least one key for help.

```sh
kb keys [--context normal] [--json]   # the keys in effect, and which come from your config
kb keys check [--json]                # validate the config; exit 1 on any problem (also in kb doctor)
kb keys defaults --keymap vim         # a commented [tui.keys.*] block to copy from
```

The inline shell picker keeps its own keys, and the conflict screen is not configurable yet.

`theme = "auto"` reads `COLORFGBG` and otherwise assumes a dark background; `NO_COLOR` turns colors
off. See [Configuration](#configuration) for the settings.

## Copying

`kb copy <id|words>`, Enter and Ctrl-Y all copy the same way. kb tries these in order and tells you
which one worked:

1. **Local tool**: `pbcopy` on macOS; on Linux `wl-copy` under Wayland (`WAYLAND_DISPLAY`), then
   `xclip` or `xsel` under X11 (`DISPLAY`). Install `wl-clipboard`, `xclip` or `xsel` if needed.
2. **Your terminal (OSC 52)**: used first over SSH or with no display, so the text lands on the
   clipboard of the machine you are sitting at. Works in iTerm2, kitty, Alacritty, WezTerm,
   Windows Terminal and recent GNOME Terminal. **Terminal.app ignores it.** Inside tmux, add
   `set -g allow-passthrough on` to `~/.tmux.conf`; GNU screen is wrapped too. kb cannot know
   whether the terminal accepted it, so it says `Sent … via terminal (OSC 52)`, not "Copied".
   Payloads over `clipboard.osc52_max_bytes` (74 KB) are refused; use
   `kb show --raw <id> | <clipboard tool>` for those.
3. **stdout**: when nothing else works the text is printed (and stderr says why), so
   `kb copy x | pbcopy` still gets it. Inside the UI nothing is printed; the key bar says the copy
   failed.

`clipboard.method` forces the first choice (`native`, `osc52` or `stdout`) and
`clipboard.command` names your own copy command; a choice that fails still falls back to the
others. Each program gets 2 seconds before the next is tried. `kb copy --method osc52 …` overrides
the setting for one run.

`kb copy <id|words>` copies the payload (the URL for a `link`, `path:line` for `code`) and records a
use. An exact ID, an ID with `.md` or a path inside the KB, or words: a search whose best match is
clearly ahead (1.5 times the score of the next) is copied; otherwise the UI opens with those words
so you can choose, or, with no terminal, the top matches are listed on stderr and the exit code is 1.
Exit codes: `0` copied, `1` not found or ambiguous, `2` error (including a payload that is too large
for the terminal and has nowhere else to go).

`kb show <id|words>` prints the entry as the preview shows it (colors only on a terminal);
`--raw` prints only the payload, `--json` the entry as JSON, `--file` the path of its file, and
`--no-color` turns colors off. It records no use.

## Actions: Opening, Running, and Custom Actions

kb entries support interactive execution and console deep linking via dedicated subcommands and TUI keys:

```sh
kb open <id|words>                      # open URL or console in default browser
kb open <id> --print-url                # print generated URL without opening
kb run <id|words>                       # run command interactively
kb run <id> -y                          # run command without confirmation
kb run <id> --dry-run                   # show command and environment without running
kb actions <id|words>                   # list available actions for an entry
kb actions <id> --json                  # machine-readable action list
kb run <id> --action <name>             # execute a custom action
```

### Opening links and consoles (`kb open`, `Ctrl-O`)

- **`link` entries**: opens `entry.url` in the default web browser (or the command configured in `opener.command`).
- **`sumo` entries**: synthesizes a Sumo Logic search URL from `sumo.endpoint_url` (or `KB_SUMO_ENDPOINT_URL`), the query payload, and time range `range: "-15m"`.
- **`cw` entries**: generates an AWS CloudWatch Insights query URL from the query, `log_groups`, `region`, and time range.
- **`code` entries**: launches the interactive Code View within the TUI, showing syntax-highlighted git snippet context and drift status. Pass `--web` to open directly on the git hosting provider (GitHub, GitLab, Bitbucket), or `--editor` to open the file in `$VISUAL` / `$EDITOR`.

### Running shell commands (`kb run`, `Ctrl-G`)

Commands (`cmd` entries) can be executed directly with their declared `shell` (`sh`, `bash`, `zsh`, `fish`) and `cwd`.

**Destructive check**: commands matching dangerous patterns (`rm -rf`, `DROP TABLE`, `mkfs`, `dd if=`, or custom patterns in `run.dangerous_patterns`) prompt for confirmation before execution. Pass `-y` / `--yes` to skip confirmation.

### Action Menu (`kb actions`, `Alt-A`, `F2`)

The Action Menu displays all built-in and custom actions for the selected entry, including their hotkeys, descriptions, and availability. Disabled actions explain why (e.g. missing clone or placeholder field).

### Custom Actions (`[[actions.<type>]]`)

Define custom actions in `config.toml` to pipe query payloads into external tools or formatters:

```toml
[[actions.cmd]]
name = "explain"
command = "gh copilot explain"
key = "e"
input = "stdin"                         # "stdin" (default) or "arg"
description = "Explain with GitHub Copilot"

[[actions.sql]]
name = "format"
command = "sqlfluff format -"
key = "f"
input = "stdin"
description = "Format query with sqlfluff"
```

Custom actions support placeholder interpolation (`{{title}}`, `{{path}}`, `{{repo}}`, `{{url}}`, etc.). Values are safely shell-escaped.

## Code entries: Drift Detection and Re-pinning

`code` entries track specific functions, classes, or configuration lines within local git repositories:

```sh
kb code check                           # check all code entries for drift
kb code check <id>                      # check specific entry
kb code check --fetch                   # fetch remote before checking
kb code check --json                    # output drift report as JSON
kb code repin <id>                      # re-pin to current tracking branch tip
kb code show <id>                       # display code preview snippet
```

### Drift statuses

When comparing pinned commit SHA against the tracking branch tip:
- `current`: commit matches tracking branch tip or code content/location has zero changes.
- `moved`: the referenced code lines shifted due to edits elsewhere in the file.
- `changed`: code within the pinned range was edited.
- `renamed`: file was renamed or moved.
- `gone`: file or ref was deleted.

### Code View Controls (`Ctrl-O` in TUI)

When viewing a `code` entry in the TUI:

| Key | Action |
|-----|--------|
| `g` | open file in local git clone in editor |
| `v` | open permalink on web host (GitHub / GitLab / Bitbucket) |
| `i` | toggle drift diff inspector |
| `y` | copy permalink to clipboard |
| `Y` | copy `path:line` to clipboard |
| `u` | repin entry to tracking branch tip |
| `F` | fetch remote and recheck drift |
| `Esc` | return to list |

### Enter Key Customization (`tui.enter = "primary"`)

Set `tui.enter = "primary"` in `config.toml` so pressing `Enter` executes the primary action:
- `link`, `sumo`, `cw`: opens in browser.
- `code`: opens the interactive Code View.
- `cmd`, `sql`, `prompt`: copies the payload to the clipboard.
- **Safety guarantee**: `Enter` never executes a command (`kb run` / `Ctrl-G` is required).

## Adding entries

`kb add` writes a valid entry for you: it reads the text, guesses the type, finds the
`{{variables}}`, suggests a title and a path, and checks the result with the same rules as
`kb lint` before anything touches the disk.

```sh
history | tail -1 | kb add -t "Restart ECS service" --tag ecs --yes   # prints cmd/restart-ecs-service
kb add --clipboard --yes -t "Authorizer p99" --tag lambda,cw          # from the clipboard
kb add --empty --type prompt --path prompts/standup -t "Standup"      # no body, type and path chosen
```

| Flag | |
|------|--|
| `-t`, `--title` | The title. Without it kb uses a leading comment (`-- title`, `# title`, `// title`), a link's host and last path segment, or a code location. |
| `--type` | `sql`, `sumo`, `cw`, `cmd`, `prompt`, `link`, `code` or `note`. Beats detection. |
| `--tag` | Repeatable or comma-separated. Lowercased; a leading `#` is dropped. |
| `--private` | Adds the `private` tag (hidden from AI agents). |
| `--path` | The new entry's ID. Folders are created. |
| `--clipboard`, `--stdin`, `--empty` | Where the body comes from. At most one. With none, stdin is used when it is piped in, else the clipboard. |
| `-y`, `--yes` | Never ask: save the entry or fail. |
| `--json` | Print the new entry as `kb show --json` does. |

The ID is `<type>/<slug of the title>`: lowercase ASCII, accents removed, words joined by `-`, at
most 60 characters. If it is taken, `-2`, `-3`, … is added. `path_template` changes the shape, for
example `{tag}/{slug}` (`{tag}` is the first tag, or the type when there are none); it must contain
`{slug}`.

**Detection** uses fixed local rules, in this order: a GitHub or GitLab file URL with a line
anchor (`/blob/<ref>/<path>#L147`) is a `code` entry with its repo, ref, path and lines filled in;
any other single URL is a `link`; Sumo Logic markers (`_sourceCategory`, `_index`, `_view`,
`_collector`, or operators such as `| timeslice` and `| count by`) are `sumo`; CloudWatch Insights
commands with `@` fields are `cw`; a leading `SELECT`, `WITH`, `INSERT`, `UPDATE`, `DELETE`,
`EXPLAIN`, `CREATE` or `ALTER` is `sql`; a few lines that look like shell commands are `cmd` (a
`$ ` or `% ` prompt marker on every line is removed); longer text is a `prompt`. A Markdown
fence's language (`sql`, `bash`, `sh`, `zsh`, `shell`) decides first, and a whole-text fence is
removed. When kb is not sure, `--yes` needs `--type` and the form puts the focus on the type.

`--yes` exits `0` with only the ID on stdout, `1` when the entry would not be valid (no title, an
unknown type, a taken path, lint errors, a likely secret, more than 1,048,576 bytes), `2` for a
usage error. An entry with a likely secret is never written: remove the value, use a
`{{variable}}` of kind `secret`, or mark the line with `kb:allow-secret`. Variable suggestions are
never applied by `--yes`.

Reading the clipboard uses `pbpaste` (macOS), `wl-paste` (Wayland), then `xclip -o` or `xsel -o`
(X11), 2 seconds each. Over SSH it is not read at all (terminals do not give the clipboard back),
and kb says so and starts with an empty body. `kb doctor` shows which program will be used.

### The add form

`Ctrl-N` in the list (or `kb add` in a terminal without `--yes`) reads the clipboard and opens the
add form: the type chips, title, tags, the variables found in the body, where the entry will be
saved, the body (colored, read-only here) and optional notes. The focus starts on the title; it
starts on the type when the clipboard was empty or unreadable or kb was not sure.

| Key | |
|-----|--|
| `Tab`, `Shift-Tab` | next / previous field (a partial tag is completed from the tags already in the KB) |
| `←` `→` on the type | change the type |
| `Ctrl-S` | save: the entry is checked like `kb lint` first; a likely secret is marked in the body and refused, and there is no "save anyway" in the form |
| `Ctrl-E` | edit the body in your editor; type and variables are detected again |
| `Ctrl-P` | preview the entry as the list will show it; `Esc` returns |
| `Esc` | close; asks `Discard this entry? (y/N)` if anything changed |

After saving, the entry is selected in the list (the search line holds its title).
**`Ctrl-N` used to select the next result**; it now adds. `↓` and `Ctrl-J` still move down.

### Variable suggestions

In the form, the Variables list also shows **suggestions**: literal values kb thinks are
parameters. Select one with `↑` `↓` and press `Space` to accept it: every occurrence of the value
becomes `{{name}}` and the variable is declared with the original value as its default, so the
entry still resolves to exactly the text you pasted. Nothing is ever applied on its own, and
`kb add --yes` applies none. Only these five patterns are looked for:

| Pattern | Becomes |
|---------|---------|
| `prod`, `production`, `stage`, `staging`, `dev`, `qa` (whole words) | `env`, an enum of the matching options |
| AWS regions such as `us-east-1` | `region` |
| `-30m`, `24h`, `7d`, `INTERVAL '1 day'` (only the number) | `minutes`, `hours` or `days`, a number |
| a 12-digit account ID in an ARN or after `account` | `account` |
| a UUID, or a run of nine or more digits | `id` |

On a declared variable, `k` cycles its kind (`text`, `number`, `enum`, `secret`), `d` sets the
default (refused for a secret), `o` sets an enum's options and `l` writes the reference as a
literal (`\{{name}}`), for text that only looks like a variable. A name that looks numeric
(`minutes`, `limit`, `days`, `count`, `port`) starts as a number and one that looks like a secret
(`token`, `password`, `secret`, `key`, `api_key`, or ending in `_token`, `_password`, `_secret`,
`_key`) as a secret with no default.

### Writing in your editor

`kb add --editor` skips the form: a template for the type (with `--title`, `--tag` and any body
already in place, the optional fields as `# key: value` comments) opens in your editor on a
private temporary file in the system temp directory, never in the KB. When the editor closes kb
checks the result; a valid one is saved and its ID printed. Otherwise the problems are listed and
you choose `[e]dit again`, `[s]ave anyway` (not offered when there is a likely secret) or
`[d]iscard`. Saving an unchanged template, or an editor that exits with an error, cancels (exit
130). The ID is `--path`, else made from the type and title as above. Editors that return at once
(`code`, `subl`) need their wait flag, for example `editor = "code --wait"`.

### Editing, renaming and deleting

```sh
kb edit <id|words>      # the entry's file in your editor, then the same checks as above
kb mv <id> <new-id>     # rename; folders are created, emptied ones removed
kb rm <id> [-y]         # delete after asking "Delete cw/foo? (y/N)"
```

`kb edit` takes an exact ID, or words that find exactly one entry (several matches list up to five
IDs and exit 1). A clean change gets a new `updated`; nothing else in the file is touched. With
errors you get the same three choices, and `[d]iscard` puts the old file back. `--json` prints
`{"id", "status", "findings"}` and never asks. The list's `Ctrl-E` still just opens the editor and
shows the first error; it does not touch `updated`.

`kb mv` carries the entry's usage and remembered variable values to the new ID, so its place in the
list does not change, even if the content was edited first. It refuses a target that exists (in
any letter case). `kb rm` forgets usage and remembered values. When the KB is a git work tree and
the file is tracked, both use `git mv` / `git rm` (and say so); otherwise, or if git fails, they
use plain file operations. Committing is up to you. `Ctrl-R` and `Ctrl-X` in the list do the same
with a prompt on the key bar.

## Shell integration

kb integrates with your shell prompt in `zsh`, `bash`, and `fish`:

- **Pull commands into your prompt** with `Alt-K`: opens a compact picker under the prompt line. Pick an entry, fill any variables via the inline form, and the resolved command is inserted onto your command line ready to review, edit, and run with Enter. kb inserts into the prompt buffer but **never auto-executes**.
- **Secrets stay out of shell history**: variables with `kind: secret` and an `env` source are resolved to environment variable references (`"$VAR_NAME"`), never plaintext secrets. Secrets without an `env` source are blocked from shell insertion.
- **Save the command you just ran** with `Alt-Shift-K` (or `kb-save-last`): captures the previous command from shell history and opens `kb add --type cmd` pre-filled.
- **Tab completion**: dynamic shell tab completion for entry IDs (with titles in zsh and fish), tags, entry types, variable names (`--var name=`), and enum options.
- **Scripted picking with `kb pick`**: invoke the compact picker from shell scripts or tmux keybindings, with support for tmux popups (`--tmux`).

### Setup

```sh
kb shell install              # detects shell, previews changes, and appends init block
kb shell uninstall            # removes only the marked kb block
```

Or configure manually in your shell profile:

- **zsh** (`~/.zshrc`): `eval "$(kb shell init zsh)"`
- **bash** (`~/.bashrc`): `eval "$(kb shell init bash)"`
- **fish** (`~/.config/fish/config.fish`): `kb shell init fish | source`

To generate standalone completion scripts:

```sh
kb completions zsh > ~/.zsh/completion/_kb
kb completions bash > /etc/bash_completion.d/kb
kb completions fish > ~/.config/fish/completions/kb.fish
```

### Key bindings and configuration

| Key | Action | Configuration |
|---|---|---|
| `Alt-K` | Open inline picker under prompt | `widget.key` / `KB_WIDGET_KEY` |
| `Alt-Shift-K` | Save previous command from history | `widget.save_key` / `KB_WIDGET_SAVE_KEY` |

In the inline picker:
- Type words to search in real time.
- `Tab` cycles the type filter (`cmd` -> all -> `cmd`).
- `Enter` selects an entry (opens inline variable form if the entry has variables, else inserts the command).
- `Esc` or `Ctrl-C` cancels and restores your prompt line unchanged.
- `Ctrl-Y` copies the resolved payload to the clipboard without touching the command line.

### Scripted picking (`kb pick`)

```sh
kb pick                                # open compact picker, print resolved payload to stdout
kb pick "tail auth" --type cmd         # pre-filter query and entry type
kb pick --print id                     # print picked entry ID instead of payload
kb pick --print json                   # print full entry JSON
kb pick --tmux                         # open in tmux popup (tmux 3.2+)
```

## AI agent integration (MCP server)

`kb mcp` exposes your knowledge base to AI coding agents (such as Claude Code, Cursor, or any Model Context Protocol client) via a local stdio JSON-RPC server:

- **No daemons or network listeners**: The client spawns `kb mcp` as a subprocess communicating over stdin/stdout.
- **Read tools by default**: The server exposes `search`, `get`, and `list_tags` tools to look up operational knowledge, resolve variables, and retrieve console/permalink URLs.
- **Strict secret shielding**: Secrets never reach the model. Variables with `kind: secret` backed by an environment variable resolve to `$VAR_NAME` references; secrets without an environment variable preserve `{{name}}` placeholders. Supplying secret values to tool calls is rejected.
- **Safe write capabilities**: `create` and `update` tools allow agents to capture or refine entries, but are **disabled by default**. Enable them with `--allow-write` or `mcp.write = true`. Edits use format-preserving YAML modifications and optimistic concurrency version hashes.
- **Reusable prompts as slash commands**: `prompt` entries are offered through `prompts/list` and `prompts/get`, turning your curated prompt snippets into agent slash commands with typed arguments.

### Client setup

```sh
kb mcp setup                  # print registration commands and configuration snippets
```

To register with Claude Code:

```sh
# Read-only (default)
claude mcp add kb -- kb mcp

# With write capabilities enabled
claude mcp add kb -- kb mcp --allow-write
```

For clients using JSON configuration (e.g. Claude Desktop):

```json
{
  "mcpServers": {
    "kb": {
      "command": "kb",
      "args": ["mcp"]
    }
  }
}
```

### Human review workflow (`kb review`)

Any entry created or modified by an agent is tagged as unreviewed and flagged with an `agent` badge in the TUI, preview pane, and `kb show`:

- **Execution protection**: Running an unreviewed `cmd` entry via `kb run` or the shell widget warns that the entry was modified by an agent and requires typed `run` confirmation before execution.
- **Interactive review**: Run `kb review` to inspect changes:

```sh
kb review                     # interactive review with unified diffs
kb review --approve-all       # batch approve all unreviewed entries
kb review --json              # machine-readable list of unreviewed entries
```

In the interactive review prompt:
- `a`: Approve the entry and record an approval commit.
- `r`: Revert to the last approved human version.
- `e`: Open the entry in your configured `$EDITOR`.
- `s`: Skip for now.

### MCP configuration

Add optional settings to your `config.toml`:

```toml
[mcp]
write = false                 # allow agents to create and update entries (default: false)
prompts = true                # expose prompt entries as MCP prompts (default: true)
exclude_tags = ["private"]    # hide entries with these tags from MCP tools and prompts
```

## Git sync

The KB is plain files, so git gives it history, backup and sync between machines through any remote
you control. kb uses your installed `git` (2.28 or newer), your identity, signing, credential helpers
and hooks; it stores no credentials. Nothing touches the network except the commands below.

```sh
kb sync setup git@github.com:me/kb.git   # git init, ignore .kb/, "kb: initial import", push -u
kb clone git@github.com:me/kb.git [path]  # a second machine: clone, build the index
kb init --git                             # a new KB under git from the start
kb sync                                   # commit hand edits, fetch, rebase, push, refresh the index
kb sync --dry-run                         # what would be committed, pulled and pushed
kb sync --no-push                         # integrate remote changes, do not push
kb sync status [--fetch] [--json]         # ahead/behind/uncommitted, offline unless --fetch
kb history <id> [--diff <rev>] [--json]   # commits that changed an entry, renames followed
kb restore <id> --at <rev>                # put an old version back (lint, secret check, commit)
kb git install-hooks                      # pre-commit hook that blocks staged secrets
kb git uninstall-hooks
```

**Every change through kb is committed**: `add`, `edit`, `mv`, `rm`, `code repin` and `restore` make
one commit limited to the files they touched (`git commit --only`), so your other staged or
unstaged changes are left alone. The message follows `git.commit_template`
(default `kb: {action} {id}`); with `git.auto_commit = false` or no git identity, nothing is committed.

**`kb sync`** refuses to run while non-entry files (a `README.md`, say) have changes, because it would
have to stash them; commit them first. Edits made outside kb are committed as one `kb: sync N
entries` (or `-m`). If the remote moved while pushing, kb fetches and integrates once more; a second
rejection stops cleanly with exit 1.

**Conflicts** are resolved one entry at a time: `m` mine, `t` theirs, `e` edit the merged text
(checked by lint and the secret scan), `b` keep both (theirs is saved as `<id>-conflict-<host>`),
`d` delete. In the terminal UI (`Alt-S`) each conflict gets a full-screen view, side by side from 120
columns; on the command line the same letters are read from the terminal or from stdin. Quitting
restores the repository to where it was before the integration. With nobody to ask (no input), the
conflict waits: run `kb sync --continue` or `kb sync --abort`.

**Passwords and passphrases**: when git or ssh needs one for a fetch, push or clone, it asks on your
terminal as usual (the UI steps aside for it). Background fetches never prompt.

**Secrets never leave the machine**: entries are scanned before every commit, and before a push every
unpushed commit is scanned, so a secret committed and later removed still stops the push (exit 1;
rewrite local history with `git rebase -i` or `git reset`). `# kb:allow-secret` marks a reviewed
exception.

Settings (`kb config set <key> <value>`):

| Key | Default | |
|-----|---------|---|
| `git.auto_commit` | `true` | commit what kb changes |
| `git.commit_template` | `kb: {action} {id}` | `{action}` and `{id}` |
| `sync.strategy` | `rebase` | or `merge` |
| `sync.timeout` | `30` | seconds, for fetch and push when there is no terminal to ask on |
| `sync.fetch_on_start` | `false` | fetch in the background when the UI opens (5 s limit, never prompts) |

The UI header shows `git ✓ in sync`, `git ↑2`, `git ↓3`, `git ● 4 changed`, `git ⚠ conflict` or
`git ⚠ fetch auth`. Exit codes: `0` done or already in sync; `1` blocked (secrets, unresolved
conflicts, push rejected twice, dirty non-entry files, not a git repository); `2` usage, git
missing or too old, I/O; `130` cancelled with the repository restored.

## Checking your setup

```sh
kb doctor               # a checklist with a fix for every warning or failure
kb doctor --copy-test   # copies a test string and asks you to paste it back
kb doctor --json
```

`kb doctor` looks at the KB root and index, the config file (and any problems in it), the display
server, SSH, tmux and screen, which clipboard method kb will use and which others work, true
color, the program `kb add` and Ctrl-N read the clipboard with (it only looks for the program; it never reads your clipboard), the editor Ctrl-E will launch, and the URL opener. It exits 1 if a check fails; warnings
alone exit 0.

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
