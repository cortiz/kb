# Research: Authoring

Decisions for M6. Each has the choice, why, and what was rejected. No `NEEDS CLARIFICATION` remained
after reading the spec and the code.

## R1. `Ctrl-N` becomes "add"; `↓`, `Ctrl-J` stay "next"

- **Decision**: In the list (Search mode) `Ctrl-N` opens the add form (FR-027). `Ctrl-N` is removed from
  the `Down` binding; `↓` and `Ctrl-J` keep selecting the next result, `↑` and `Ctrl-P` and `Ctrl-K`
  the previous. `Ctrl-R` (rename) and `Ctrl-X` (delete) are free in the table today.
- **Rationale**: the spec names the key and SC-001 counts it. M3's KeyMap fails to build on duplicates,
  so the change has to be explicit anyway.
- **Alternatives**: `Alt-N` for add (keeps readline muscle memory, contradicts FR-027 and SC-001);
  keeping both on Ctrl-N by mode (the list has one mode, so impossible).
- **Cost**: `Ctrl-N` / `Ctrl-P` stop being a pair. Documented in README and the help overlay.

## R2. Type detection

- **Decision**: `detect(text) -> Detection { ty, confidence, fields, body }`, a fixed pipeline:
  1. Normalize line endings (FR-003), trim leading/trailing blank lines.
  2. If the whole text is one Markdown fence (``` or ~~~), unwrap it and remember the language hint
     (`sql` → sql; `bash`, `sh`, `zsh`, `shell` → cmd). A hint decides the type with **High** confidence.
  3. Rules 1–7 of FR-005 in order, with these refinements:
     - **Rule 3 vs 4**: the Sumo *operator* route (`| parse`, `| json`, `| count by`, `| timeslice`) only
       fires when the text has no `@timestamp`, `@message`, `@ptr` or `@log*` field. `_sourceCategory`,
       `_index`, `_view` and `_collector` always mean Sumo. Without this, every CloudWatch
       `| parse @message …` query would be called Sumo.
     - **Rule 4** needs a command keyword at the start of a line **and** an `@` field, or a `stats …
       by` / `filter @…` line; keywords alone (`filter`, `sort`, `limit`) are too common in prose.
     - **Rule 5** looks at the first non-comment token, case-insensitively, and requires a second SQL
       marker (`FROM`, `INTO`, `SET`, `TABLE`, `;`, `(`) for **High**; a lone `WITH` / `CREATE` is **Low**.
     - **Rule 6** (cmd) scores: known command name first (a built-in list of ~80: `aws`, `kubectl`,
       `docker`, `git`, `curl`, `grep`, `sed`, `ssh`, `terraform`, `helm`, `jq`, …), flags, pipes,
       redirections, `$`/`%` prompt markers; ≤ 8 lines. Two or more signals: **Medium**; one: **Low**.
     - **Rule 7** (prompt): more than 8 lines or sentences with spaces and no command signal →
       `prompt`, **Low** (it is the fallback, so it never claims High).
  4. After the type is known and it is `cmd`, strip `$ ` / `% ` from **every** non-empty line (edge case);
     the stripped text becomes the body. Stripping is not done if only some lines have a marker.
- **Confidence**: `High` (rules 1, 2, a fence hint, or two independent markers), `Medium` (one clear
  marker), `Low` (fallbacks and conflicts). Only `Low` changes behavior (FR-006).
- **Never High and wrong (SC-002)**: ambiguous snippets in the corpus live in `detect/low/` and the test
  asserts `Low` for them; a general test asserts every snippet detected `High` has the right type.
- **Rationale**: ordered rules are easy to read, test and extend; the corpus makes accuracy a number.
- **Alternatives**: scoring every type and picking the max (harder to explain a wrong pick, no clear
  place to add a rule); any ML or n-gram model (spec forbids AI and the binary should not grow).

## R3. Title, path and tag defaults

- **Title** (FR-007): `--title` → first comment line (`-- `, `# `, `// `, ignoring a `#!` shebang, only
  within the first 3 lines, 3–120 chars, which is the lint title limit) → link: `host + last path
  segment` (`github.com/kb-cli/kb` → `github.com kb`) → code: `symbol` or `file.rs:147`. Otherwise empty.
- **Slug** (FR-008): NFKD decompose, drop combining marks, lowercase, ASCII alphanumerics kept, any run
  of other characters becomes one `-`, trimmed, cut at 60 characters on a `-` boundary when possible.
  Empty slug → the form asks for a path; `--yes` without `--path` fails (exit 1) with that message.
- **Path** is `<type>/<slug>`; collisions append `-2`, `-3`, … until free (checked against the files,
  not the index). Case-insensitive collisions count as clashes (M1 KB053).
- **`path_template`** (config): placeholders `{type}`, `{slug}`, `{tag}` (the first tag; `{type}` when
  there are none). It must contain `{slug}`; an invalid template falls back to the default with a config
  warning. Not a template engine.
- **Tag completion** (FR-010): `Kb::tag_counts()` reads the index's tag table and sorts by count then
  name. Free, because the index already stores tags; falls back to a file scan when the index is stale.

## R4. Variable detection and suggestions

- **Detected variables** (FR-011): the same scanner M1/M5 use (`scan_refs`), on the body only, giving
  names in first-appearance order. Kind defaults by name (FR-013): exact words `minutes`, `limit`,
  `days`, `count`, `port` → `number`; `token`, `password`, `secret`, `key`, `api_key` and names ending
  `_token`, `_password`, `_secret`, `_key` → `secret` (no default). Everything else `text`.
- **Literal marking** (edge case): marking a reference literal rewrites `{{x}}` to `\{{x}}` in the body
  and drops the declaration (M1 `body::unescape` already reads that form).
- **Suggestions** (FR-014, FR-015), one `VariableSuggestion` per *distinct value*, each with all its
  positions:
  - env: whole word, lowercase, `\b(prod|production|stage|staging|dev|qa)\b` (so `prod/lambda`
    matches and `product` does not). Kind `enum`; options are
    the value's family: short form `[dev, qa, stage, prod]`, long form `[dev, qa, staging, production]`;
    default is the original value. Name `env`.
  - region: `\b(us|eu|ap|sa|ca|me|af|il|mx)-[a-z]+-\d\b`. Name `region`, `text`.
  - time: `-?\d+(m|h|d)` as a whole token (not preceded or followed by a letter or digit) and
    `INTERVAL '<n> <unit>'` (unit: minute(s), hour(s), day(s)). Only the **digits** are replaced
    (`-30m` → `-{{minutes}}m`), name from the unit (`minutes`, `hours`, `days`), `number`.
  - account: 12 digits in an ARN (`arn:aws:…:<12>:`) or following `account` / `account_id` / `account-id`
    with `=`, `:`, space or quote between. Name `account`, `text`.
  - id: UUID `8-4-4-4-12` hex, or a run of ≥ 9 digits not already claimed by account/time. Name `id`, `text`.
- **Collisions**: matches inside an existing `{{…}}` or `\{{…}}` are ignored; if the proposed name is
  already declared or used, the suggestion gets `_2`, `_3`, …; if two suggestions want the same name
  (`prod` and `stage` are both `env`) the second gets `_2`.
- **Safe by construction (SC-006)**: every suggestable value is made of `[A-Za-z0-9._/-]`, which M5's
  quoting leaves unchanged in `cmd` (any context), `sql`, and URLs, so resolving with defaults gives the
  original text byte for byte. A table test checks each pattern in each context.
- **Not applied on their own**: accepting is a user action; `kb add --yes` never accepts (the spec lists
  no flag for it, and non-interactive guessing would break "predictable").
- **Alternatives**: suggest any repeated token (noisy, unpredictable); an `--accept-suggestions` flag
  (not in the spec; can be added later without a format change).

## R5. The Draft and canonical rendering

- **Decision**: `Draft` holds typed fields, not text; `Draft::render(now)` returns the bytes. Layout
  (FR-018): `---`, `kb_format: 1`, `type`, `title`, `tags` (flow list `[a, b]`, omitted when empty),
  type-specific fields in `EntryType::keys()` order, `vars` block (declaration order = first appearance),
  `created`, `updated`, `---`, blank line, payload fence, blank line, notes (omitted with its blank line
  when empty). `link` and `code` have no payload fence (`has_payload()`); their URL / code reference is
  in the frontmatter (`url:`, `code:` with `repo`, `ref`, `path`, `line`, `end_line`, `symbol`).
- **Fence length**: longest run of backticks in the body + 1, minimum 3, so a body containing a fence
  cannot break out (SC-003 round trip).
- **YAML quoting**: titles and strings are plain when `is_plain_safe` (reuse `edit.rs`), else
  double-quoted with the same escaper as `set_field`. Single code path for quoting.
- **Dates**: `YYYY-MM-DDTHH:MM:SSZ`, UTC, from `SystemTime` (no `chrono`). `created` equals `updated` on
  creation.
- **Round trip**: `parse(render(draft))` gives back the same fields and `to_bytes()` equals the rendered
  bytes (M1 SC-001), asserted in a test over the golden set.
- **Alternatives**: a YAML serializer crate (a dependency for a fixed 10-key layout, and it would reorder
  or restyle); building the file from a per-type template string (duplicates the key lists).

## R6. Validation before writing

- **Decision**: `Draft::check(now) -> Check { bytes, findings }` renders, then runs
  `lint::check_text(id, bytes)`, which is `check_file` on in-memory bytes. `Check::secrets()` and
  `Check::errors()` filter by severity and rule class.
- **Policy** (FR-017): secret findings (`KB060`–`KB065`) → not writable, ever. Other errors → writable
  only through an explicit "save anyway" in the interactive editor flows (US3.3, US4.1). The form
  treats every error as blocking (it can fix them in place), and `--yes` fails with exit 1 listing
  findings. Size over 1 MB: `KB051` is a warning in M1, but this milestone refuses (edge case): `check`
  adds a synthetic `TooLarge` result before rendering and the caller reports the limit.
- **Why not validate the typed `Draft` directly**: the lint on the bytes is the same code that will judge
  the file later, so a pass here means a pass in `kb lint` (SC-003, SC-004).

## R7. File operations and conflicts

- **Create**: write a temp file next to the target, `fsync`, then `hard_link` it to the final path and
  remove the temp. `hard_link` fails if the target exists, so it is an atomic no-clobber create. If hard
  links are not supported (some network filesystems) fall back to `rename` after an existence check.
  Missing folders are created (`create_dir_all`) before.
- **Overwrite detection** (FR-020): `Version { hash, len, mtime }` captured when a file is read (for
  `kb edit` and TUI save of an existing entry; only `kb edit` exists in this milestone). Before writing,
  re-read and compare `content_hash`; mismatch → `Conflict`, nothing written.
- **Rename**: `Kb::rename_entry(old, new, mover)`: validate both IDs, refuse if `new` exists (also
  case-insensitively), create parent folders, call `mover(old_path, new_path)`, then
  `State::move_entry(old, new)` and `Kb::refresh()`. A failing `mover` leaves everything untouched.
  Empty parent folders left behind are removed (but never the root or `.kb`).
- **Remove**: `Kb::remove_entry(id, remover)`, then `State::forget(id)`, empty-folder cleanup, refresh.
- **Index freshness** (FR-021): all three end with `Kb::refresh()` so the next search is exact, and the
  TUI uses its existing refresh path to re-run the query and select the new ID.
- **State move** (FR-022, SC-005): `UsageRecord` and `VarMemory.entries[old]` move to `new` (recent
  per-variable lists are keyed by variable name, not entry, and stay). The record's `hash` is refreshed
  to the file's current hash. A stale record at `new` is overwritten.

## R8. Git

- **Decision**: `platform::run_git(root, args) -> Result<Output, ToolError>` (2 s timeout is wrong for
  git; use 10 s). `kb mv` / `kb rm` call `git -C <root> rev-parse --is-inside-work-tree` once; if true,
  `git ls-files --error-unmatch -- <path>`: tracked → `git mv` / `git rm -q`, untracked → plain
  operation. Any git failure (not installed, error exit) falls back to the plain operation, and the
  output says which one ran. `git rm` of a file with staged changes would refuse; the fallback deletes
  the file normally, because the user confirmed the delete.
- **Rationale**: the spec wants history kept (FR-024) and says committing is M10. A plain rename on a
  tracked file is also fine for git; this keeps the index tidy for people who `git status` right after.
- **Alternatives**: always plain operations (simplest; contradicts the spec); a libgit2 binding (a big
  dependency to run two commands).

## R9. `kb edit` and Ctrl-E

- **Decision**: `kb edit <id|query>` resolves an exact ID first; otherwise runs a normal search and
  takes the top result only when exactly one entry matches at the best tier; several → exit 1 listing the
  first five IDs (compose with `kb --print id | fzf`). Then:
  1. Snapshot the bytes (`Version`).
  2. Run the editor on the real file (as today).
  3. Editor exit code ≠ 0 → treated as cancelled: the snapshot is restored if the file changed, exit 130.
  4. Bytes unchanged → nothing to do, exit 0, no `updated` bump.
  5. `check_text` on the new bytes. Clean → set `updated` with `set_field`, write atomically, refresh,
     exit 0.
  6. Errors → list them (`path:line: error[KBxxx]: …`) and ask **e**dit again / **s**ave anyway (hidden
     for secrets) / **d**iscard (restore snapshot, exit 130). No terminal for the prompt → print and
     exit 1, leaving the file as the editor wrote it (as today).
  7. "Save anyway" on a file that parses bumps `updated`; on one that doesn't parse leaves it as is.
- **Ctrl-E in the list stays as M3** (spec): no loop inside the TUI, no `updated` bump. Documented
  difference; `kb edit` is the checked path.
- **Why edit in place**: the editor, its swap files, `:w` semantics and plugins all expect the real
  path; editing a copy would add a window where the real file changes (sync tools) for no benefit.

## R10. Clipboard read

- **Decision**: new `Platform::run_read_tool(&NativeTool, Duration) -> Result<Vec<u8>, ToolError>`
  captures stdout (cap 2 MiB, so a huge clipboard cannot exhaust memory; larger → treated as too large
  and reported). `clipboard::read(platform, env) -> ReadOutcome`:
  - SSH (`env.ssh`) → `Unavailable(OverSsh)`, no tool tried (spec edge case; X11 forwarding is not
    trusted for a clipboard the user did not copy on this machine).
  - tools: macOS `pbpaste`; Linux Wayland `wl-paste` (plain, so the bytes are exact); then X11
    `xclip -selection clipboard -out`, `xsel --clipboard --output`.
  - tool missing → try next; none → `Unavailable(NoTool(hint))` with the install hint from the copy side.
  - bytes not valid UTF-8, or contain NUL → `Empty` (non-text: images, files; edge case).
  - text → CRLF/CR → LF; whitespace-only → `Empty`.
- **No setting** to override the read command in this milestone (YAGNI; `clipboard.command` is copy-only
  and documented as such).
- **Doctor**: a `clipboard-read` check: pass with the tool name, warn when unavailable with the reason
  and the hint, in the same JSON shape as the other checks.

## R11. TUI add form and list prompts

- **Decision**: the add form is a full screen held in `AppState.add: Option<Box<AddState>>`, pure
  `update`/`handle_key` like M5's `FormState`, drawn by `add_view.rs`. Effects: `ReadClipboard` (runs the
  platform read on the effect thread, ≤ 2 s), `EditBody(text)` (suspends the terminal like `edit_entry`,
  runs the editor on a private temp file with an extension from the type, returns the new text),
  `SaveDraft(Box<Draft>)` (create + refresh), `RenameEntry`, `RemoveEntry`.
- **Ctrl-R / Ctrl-X** use a one-line prompt on the key bar (the bar already switches content for the
  copy confirmation): rename pre-fills the current ID with the cursor at the end; delete shows
  `Delete cw/foo? (y/N)`. Both go through the same core functions as the CLI.
- **Detection while typing**: re-run on body edits made through Ctrl-E only (the body is read-only in
  the form; the spec edits it in the editor), so no per-keystroke cost. Title, tags and path are plain
  line inputs reusing M3/M5's `LineEditor`.
- **Dirty tracking**: a `changed` flag set by any field edit, type change, suggestion accept or body
  edit; the initial values (clipboard text, detected type) do not count (Esc on an untouched form closes).
- **Narrow terminals**: below the M3 wide breakpoint the right pane (body) moves under the left fields
  and Ctrl-P switches between them, like the M3 preview.
- **Temp files** are created by `platform::private_temp_file(ext)` (`0600`, system temp dir, random
  suffix, `create_new`), wrapped in a drop guard; the guard is also in the signal path, so SIGTERM during
  the editor removes it. A hard crash (SIGKILL) can leave one file in the temp dir; documented ceiling.

## R12. `kb add` input selection

- **Order**: `--empty` → empty; `--stdin` → stdin; `--clipboard` → clipboard; none → stdin if it is not
  a terminal, else clipboard. Combining two of the three flags is a usage error (exit 2; clap
  `conflicts_with_all`). With `--stdin` and a terminal on stdin, kb reads until EOF as `cat` would.
- **Reading stdin** is capped at 1 MB + 1 so an accidental `yes | kb add` stops (then the size rule
  refuses the save with the limit message).
- **UI choice**: no `--yes` and no `--editor` → open the TUI on the add form (needs a terminal: if
  stdout is not a terminal, exit 2 "kb add needs --yes when there is no terminal").
- **After TUI save** the process exits 0 and prints the new ID to stdout, so `kb add` composes in
  shell scripts that ran it interactively; cancelled → 130.

## Open items carried to tasks

- Corpus of ≥ 100 snippets (SC-002) is written first and committed with the failing detector.
- Manual terminal matrix (Terminal.app, iTerm2, tmux, an SSH session) for Ctrl-N / Ctrl-R / Ctrl-X and
  the editor suspend/resume, and CI on both OSes, are person-tasks like M5's T060–T062.
