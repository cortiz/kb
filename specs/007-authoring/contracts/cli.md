# Contract: CLI

Conventions from M3–M5 apply: `--json` is accepted, errors go to stderr (or as JSON with `--json`), the
KB root comes from the usual `--root` / config / `KB_ROOT` chain, prompts read from `/dev/tty`.

## `kb add`

```text
kb add [--title T | -t T] [--type TYPE] [--tag T | --tag T ...] [--path P]
       [--clipboard | --stdin | --empty] [--editor] [--yes | -y] [--json]
```

| Flag | Meaning |
|---|---|
| `--title`, `-t` | Title. Skips the title suggestion. |
| `--type` | One of `sql sumo cw cmd prompt link code`. Wins over detection (confidence High). |
| `--tag` | Repeatable; also accepts comma-separated values. Normalized and de-duplicated. |
| `--path` | ID of the new entry (no `.md`). Folders are created. |
| `--clipboard` / `--stdin` / `--empty` | Force the body source. Mutually exclusive (usage error, exit 2). |
| `--editor` | Skip the form; edit a template in the editor (below). |
| `--yes`, `-y` | Never open a UI and never prompt: save or fail. |
| `--json` | Print the created entry as `kb show --json` does (id, path, type, title, tags, variables, …). |

### Source when none of the three flags is given

stdin is a pipe/file → stdin; otherwise clipboard. Clipboard `Unavailable` or `Empty` → empty body, with
one stderr line saying why when it is `Unavailable` (`clipboard not readable over SSH; opening an empty
form`).

### Flow

```text
body ← source                       (stdin capped at 1 MB + 1)
draft ← Draft::from_text(body, opts)

if --editor        → editor template flow (below)
else if --yes      → non-interactive save
else               → TUI add form on the draft (needs a terminal)
```

**Non-interactive save (`--yes`)**:

1. Title missing and none suggested → stderr `a title is required (use --title)`, exit **1**.
2. Detection confidence `Low` and no `--type` → stderr `could not tell the type (use --type)`, exit **1**
   (FR-006).
3. Path: `--path`, else derived with `-2`, `-3` on clash; empty slug → `a path is required (use --path)`,
   exit 1.
4. `check`: any finding that makes `writable(false)` false → print each as
   `path:line: error[KBxxx-slug]: message` to stderr, write nothing, exit **1**. Secret findings add
   `remove the value, use a {{variable}} of kind secret, or mark it with kb:allow-secret`.
5. Rendered entry over 1,048,576 bytes → `the entry is N bytes; entries are limited to 1048576`, exit 1.
6. Otherwise create, refresh the index, print the ID (`cw/authorizer-p99-latency`) on stdout (or the JSON
   document with `--json`), exit **0**. Suggestions are not applied.

**Interactive form**: needs a terminal on stdout and a tty; otherwise exit **2** with
`kb add needs --yes or --editor when there is no terminal`. Closes with the ID printed after the UI
restores the terminal (0), or nothing printed and exit **130** when cancelled.

### Editor template flow (`--editor`)

1. Needs a terminal (`open_tty`); none → exit 2 `no terminal to run the editor on`.
2. Template for the type (from `--type`, else detection, else `cmd`) is written with `--title`,
   `--tag`, and the body (when a source gave one) already in place, to a private temp file with the
   `.md` extension. Template content: all required frontmatter fields filled, type-specific optional
   fields as `# key: value` comments, `created`/`updated` set, an empty payload fence (`link`/`code`
   have none and show `url:` / `code:` lines).
3. The editor runs (`resolve_editor`). Exit code ≠ 0 or interrupted → temp file deleted, exit **130**.
4. Bytes equal to the original template → "unchanged", exit **130**, nothing written.
5. The file's `type` and `title` are read back; the ID is `--path` or derived from them as in the form.
   A file that does not parse yet is shown the errors with the choices below (no ID yet: the choice
   "save anyway" is hidden until an ID can be derived).
6. `check`. Clean → create at the ID, refresh, print the ID, exit **0**.
7. Findings → print them, then prompt on the tty:
   `[e]dit again  [s]ave anyway  [d]iscard: `
   - `e`: reopen the editor on the same temp file (content kept), go to 4.
   - `s`: only when no secret findings and an ID exists. Writes as is (a warning line lists what was
     accepted), exit **0**. The entry stays out of search until fixed (M2).
   - `d` / EOF / Ctrl-C: delete the temp file, exit **130**.
   `s` is not shown (and typing it re-prompts) when any secret finding exists.
8. The temp file is always deleted (drop guard + signal path).

`--yes` with `--editor` is a usage error (exit 2): the editor is interactive by definition.

## `kb edit <id|query>`

```text
kb edit <id|query> [--json]
```

Resolution: exact ID first; else a normal search; exactly one best match is used; several →
stderr list of up to 5 IDs and exit **1**; none → `no entry matches "<q>"`, exit **1**.

Flow and exit codes as research R9: unchanged → 0 (nothing written); clean → `updated` bumped through
`set_field`, 0; errors → `e / s / d` prompt (same wording as above; `d` restores the snapshot and exits
130); no terminal for the prompt → errors printed, file left as the editor wrote it, exit 1; editor exit
code ≠ 0 → snapshot restored if the file changed, 130; `Conflict` (the file changed on disk between
the post-editor read and the write) → `the file changed while it was being saved; run kb edit again`,
exit 1. `--json` prints a single object `{"id","status":"saved|unchanged|discarded","findings":[…]}`;
prompts are not shown with `--json` (a non-interactive run, exit 1 when invalid).

## `kb mv`

```text
kb mv <id> <new-id> [--yes | -y] [--json]
```

1. `<id>` must exist (`not found`, exit 1); `<new-id>` must be a valid ID (`.md` suffix tolerated and
   removed) (exit 1 `bad id`).
2. `<new-id>` exists (any case) → `cw/foo already exists`, exit **1**. Same as `<id>` → exit 0, nothing
   done.
3. Moves with `git mv` when the root is a git work tree and the file is tracked, else a plain rename
   (R8); folders are created; empty source folders are removed.
4. Usage and remembered values are carried to `<new-id>` (FR-022); the index is refreshed.
5. Prints `renamed cw/foo -> cw/bar (git mv)` or `(rename)`; `--json`: `{"id","from","to","method"}`.
   `--yes` is accepted and has no prompt to skip for `mv` today (kept for symmetry with the spec's
   signature; no confirmation is asked on rename). Exit 0.

## `kb rm`

```text
kb rm <id> [--yes | -y] [--json]
```

1. Not found → exit 1.
2. Without `--yes`: prompt on the tty `Delete cw/foo? (y/N) `; anything but `y`/`yes` → exit **130**.
   No terminal and no `--yes` → exit 2 `kb rm needs --yes when there is no terminal`.
3. Deletes with `git rm` (tracked in a work tree) else `remove_file`; drops usage and remembered
   values; removes empty folders; refreshes.
4. Prints `deleted cw/foo (git rm)`; `--json`: `{"id","method"}`. Exit 0.

## Exit codes (FR-030)

| Code | When |
|---|---|
| 0 | saved, renamed, deleted; `kb edit` with no change |
| 1 | validation failure, entry not found, ambiguous query, target exists, missing title/type/path with `--yes`, conflict |
| 2 | usage error (bad or conflicting flags), no terminal where one is needed, I/O errors |
| 130 | cancelled: discard, Esc in the form, `n` at a delete prompt, editor exit ≠ 0, unchanged template |

## Output formats

- Human output goes to stdout only for the ID (add) and the one-line result (mv, rm); everything else
  (warnings, findings, prompts) goes to stderr, so `id=$(kb add --yes -t X --tag y < file)` works.
- Finding lines use the format `kb lint` and `kb edit` already print.
