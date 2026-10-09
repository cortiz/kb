# Feature Specification: Authoring

**Feature Branch**: `007-authoring`
**Created**: 2026-10-09
**Status**: Draft
**Milestone**: M6
**Depends on**: M1 (`001-entry-format`), M2 (`002-index-search`), M3 (`003-tui-core`), M4 (`004-platform-clipboard`), M5 (`006-variables`)
**Input**: Let the user capture new entries and manage existing ones without writing frontmatter by hand: an add form in the TUI (screen 4 of the design), `kb add` from the shell with type detection and variable detection, plus `kb edit`, `kb mv` and `kb rm`.

## Overview

Until now, every entry has been written by hand following the M1 format. That's fine for a few dozen entries, but the real moment of capture is mid-incident: a query just worked in Sumo or CloudWatch, and it should go into the KB in seconds, before it's lost in browser history.

This milestone adds the capture path. Content usually arrives from the clipboard, so the platform layer gains **clipboard reading**. kb looks at the content, guesses its type, finds variables, suggests a title, path and tags, and writes a valid entry. Every save goes through M1's validation and secret checks, so authoring through kb can never produce a file that hand-editing would have rejected.

## Clarifications

### Session 2026-10-09

- Q: `Ctrl-N` already means "select next result" in the list; should add take that key? → A: Yes. `Ctrl-N` adds and no longer selects the next result; `↓` and `Ctrl-J` still do.
- Q: Should Ctrl-E in the list bump `updated` and check the file, like `kb edit`? → A: No. List Ctrl-E stays as in M3 (no bump, only the first lint error shown); only `kb edit` bumps `updated` and offers edit-again / save-anyway / discard.
- Q: What does `kb edit <query>` do when several entries match? → A: An exact ID wins; otherwise exactly one match is opened; several matches exit 1 listing up to 5 IDs and open nothing.
- Q: Can `kb add --yes` apply variable suggestions? → A: No. Suggestions are only applied by the user accepting them in the form; this milestone has no flag for it.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Capture a query from the clipboard in the TUI (Priority: P1)

Carlos copies a CloudWatch Insights query from the AWS console, opens `kb`, and presses Ctrl-N. The add form opens with the query already in the body, the type set to `cw`, the `{{fn}}` and `{{ms}}` variables he typed detected, and the cursor on the title. He types a title, adds two tags, and presses Ctrl-S. The entry is saved, indexed and selected in the list.

**Why this priority**: this is the capture moment the milestone exists for.

**Independent Test**: with the platform test double holding fixture clipboard content of each type, open the form, type a title, save, and compare the written file with an expected file byte for byte.

**Acceptance Scenarios**:

1. **Given** the clipboard holds text, **When** the user presses Ctrl-N in the list, **Then** the add form opens (screen 4) with that text as the body, the detected type selected, detected variables listed, and the focus on the title field.
2. **Given** the clipboard is empty or unreadable, **Then** the form opens with an empty body and the focus on the type field.
3. **Given** the user presses Ctrl-S with a valid title, **Then** kb writes the entry, updates the index, closes the form, selects the new entry in the list, and shows `Saved cw/authorizer-p99-latency` in the key bar.
4. **Given** the form has unsaved changes, **When** the user presses Esc, **Then** kb asks to confirm discarding them. Without changes, Esc closes immediately.
5. **Given** the user presses Ctrl-E in the form, **Then** the body opens in the editor, and when the editor closes the form shows the edited body with type and variable detection rerun. A type the user chose is kept unless the new detection is High and different.

---

### User Story 2 — Capture from the shell (Priority: P1)

Carlos runs `kb add` after copying a query, or pipes into it: `history | tail -1 | kb add -t "Restart ECS service" --tag ecs`.

**Why this priority**: constitution principle IV; and piping from other tools is how entries get added from scripts.

**Acceptance Scenarios**:

1. **Given** `kb add` in a terminal with no input piped in, **Then** kb opens the TUI directly on the add form, filled from the clipboard (same as User Story 1).
2. **Given** content is piped in, **Then** kb uses stdin as the body instead of the clipboard.
3. **Given** `--title`, `--type`, `--tag` (repeatable), `--path` and `--yes`, **Then** kb uses those values. With `--yes` and enough information to produce a valid entry (at least a title), kb saves without opening any UI and prints the new ID.
4. **Given** `--yes` without a title, **Then** kb exits 1 explaining that a title is required.
5. **Given** `--editor`, **Then** kb skips the form and opens a pre-filled template in the editor (User Story 3).
6. **Given** `--clipboard` / `--stdin` / `--empty`, **Then** they force where the body comes from.
7. **Given** `--json`, **Then** kb prints the created entry as JSON (same shape as `kb show --json`).

---

### User Story 3 — Write an entry in my editor with a template (Priority: P2)

Carlos prefers his editor. He runs `kb add --editor --type sql`, gets a file with the frontmatter already laid out for a SQL entry, writes it, saves and quits.

**Acceptance Scenarios**:

1. **Given** `kb add --editor`, **Then** kb creates a temporary file holding a template for the chosen type (frontmatter with every required field, type-specific optional fields as comments, and an empty payload fence), opens it in the editor, and validates it when the editor closes.
2. **Given** the result is valid, **Then** kb moves it into the KB at the chosen path and prints the ID.
3. **Given** the result has lint errors, **Then** kb lists them and offers: **e**dit again, **s**ave anyway (not offered for secret findings), or **d**iscard.
4. **Given** the user saves and quits without changing the template, **Then** kb treats it as cancelled and writes nothing.

---

### User Story 4 — Edit, rename and delete entries (Priority: P2)

**Why this priority**: once kb writes files, users expect to manage them through kb too, without losing usage history.

**Acceptance Scenarios**:

1. **Given** `kb edit <id|query>`, **Then** kb opens the entry's file in the editor (as Ctrl-E does in the TUI), then validates it. Errors are reported with the same three choices as User Story 3, except that "discard" restores the previous content.
2. **Given** `kb mv <id> <new-id>`, **Then** kb renames the file, creates folders as needed, and carries over usage and remembered variable values to the new ID, even if the content was also edited. Ctrl-R in the TUI list does the same with an inline prompt.
3. **Given** `kb rm <id>`, **Then** kb asks to confirm (unless `--yes`), deletes the file, and removes its usage and remembered values. Delete in the TUI list (Ctrl-X) asks the same.
4. **Given** the KB is a git repository, **Then** `kb mv` and `kb rm` use `git mv` and `git rm` so history is kept. (Committing is M10.)
5. **Given** the target of `kb mv` already exists, **Then** kb refuses and exits 1.

---

### User Story 5 — kb turns literal values into variables (Priority: P3)

Carlos pastes a Sumo query that contains `_sourceCategory=prod/lambda/authorizer` and `-30m`. kb suggests making `prod` into `{{env}}` and `30` into `{{minutes}}`.

**Acceptance Scenarios**:

1. **Given** the body contains values matching the suggestion rules (FR-015), **Then** the form lists each suggestion under "variables", showing the value, the proposed name and kind.
2. **Given** the user accepts a suggestion (Space on it), **Then** every occurrence of that value in the body is replaced by the variable reference, and a declaration with the original value as default is added.
3. **Given** suggestions are not accepted, **Then** nothing changes. Suggestions never apply on their own.

---

### Edge Cases

- **Nothing in the clipboard but a file path or image**: only text is used. Non-text clipboard content is treated as empty.
- **Clipboard over SSH**: reading the clipboard over OSC 52 is not supported by most terminals, so it is not attempted. Over SSH, `kb add` uses stdin or opens an empty form, and says why.
- **Pasted content from a shell prompt**: leading `$ ` or `% ` prompt markers on every line are removed when the type is `cmd`.
- **Content wrapped in a Markdown fence** (copied from docs or chat): the outer fence is removed and its language hint helps detect the type.
- **Content that already contains `{{...}}` meant literally** (for example a Go or Helm template): the form shows the detected variables, and the user can mark each as literal, which writes it as `\{{...}}`.
- **Title produces an empty or clashing path**: kb appends `-2`, `-3` and so on, or asks for a path if the title has no usable characters.
- **Very large content** (a file over the 1,048,576-byte lint limit from M1): kb refuses to save it and explains the limit.
- **Concurrent edits**: if the file changed on disk between kb reading it and saving, kb refuses to overwrite, says the file changed, and asks the user to run `kb edit` again (exit 1).
- **Editor exits with an error code**: treated as cancelled. The temporary file is deleted.
- **Secrets detected in pasted content**: the form highlights the offending text and refuses to save until it is removed, replaced with a `secret` variable, or marked with `kb:allow-secret`.
- **Temporary files**: created with owner-only permissions, inside the system temp directory, never inside the KB, and always deleted, including after a crash where possible.

## Requirements *(mandatory)*

### Clipboard reading

- **FR-001**: The platform layer (M4) MUST gain a clipboard read operation: `pbpaste` on macOS, `wl-paste` on Wayland, `xclip -o` or `xsel -o` on X11. Reading MUST time out after 2 seconds.
- **FR-002**: Over SSH or when no read method is available, reading MUST report "unavailable" rather than fail, so callers can fall back. `kb doctor` MUST report the read method too.
- **FR-003**: Only text clipboard content MUST be used, with line endings normalized to `\n`.

### Type detection

- **FR-004**: The core MUST provide type detection that returns the most likely type and a confidence level. Detection MUST be deterministic and use only local rules (no network, no AI).
- **FR-005**: Detection rules, checked in this order:
  1. a single line that is a GitHub or GitLab file URL with a line anchor (`/blob/<ref>/<path>#L147`) → `code`, with `repo`, `ref`, `path`, `line` and `end_line` filled in from the URL;
  2. a single line that is an `http` or `https` URL → `link`, with `url` filled in;
  3. contains `_sourceCategory`, `_index`, `_view` or `_collector`, or a pipe followed by Sumo operators such as `timeslice`, `parse`, `json`, `count by` → `sumo`;
  4. starts with or contains CloudWatch Insights commands (`fields`, `filter`, `stats`, `parse`, `sort`, `limit`, `display`) and `@` fields such as `@timestamp`, `@message` → `cw`;
  5. starts with an SQL keyword (`SELECT`, `WITH`, `INSERT`, `UPDATE`, `DELETE`, `EXPLAIN`, `CREATE`, `ALTER`) → `sql`;
  6. one to a few lines that look like shell commands (a known command name first, flags, pipes, redirections, prompt markers) → `cmd`;
  7. longer natural-language text → `prompt`.
  A Markdown fence's language hint (`sql`, `bash`, `sh`) MUST take precedence when present.
- **FR-006**: When confidence is low, the TUI form MUST put the focus on the type field and the CLI with `--yes` MUST require `--type`.

### Defaults for a new entry

- **FR-007**: The suggested **title** MUST come from, in order: `--title`; the first comment line of the payload (`-- title`, `# title`, `// title`); for `link`, the URL's host and last path segment; for `code`, the `symbol` or the file name and line. The title MUST be left empty for the user when none of these apply.
- **FR-008**: The default **path** MUST be `<type>/<slug-of-title>`, where the slug is lowercase ASCII with words joined by `-`, accents removed, and at most 60 characters. A `path_template` setting in config MAY change this (for example `{tag}/{slug}`).
- **FR-009**: `kb_format`, `created` and `updated` MUST be set automatically. `updated` MUST change on every save through kb (the form, `kb add` and `kb edit`; the list's Ctrl-E opens the editor as in M3 and is not a kb save); `created` MUST never change.
- **FR-010**: Tag entry MUST offer completion from tags already used in the KB, ordered by how often they are used.

### Variables in new content

- **FR-011**: Every `{{name}}` in the body MUST be detected and listed in the form. Each detected variable MUST be declared in the saved entry with `kind: text` unless the user changes it.
- **FR-012**: In the form, the user MUST be able to set each variable's kind (`text`, `number`, `enum`, `secret`), default and options, or mark it as literal.
- **FR-013**: Variables that look numeric from their name (`minutes`, `limit`, `days`, `count`, `port`) MUST default to `kind: number`. Variables named like secrets (`token`, `password`, `secret`, `key`, `api_key`) MUST default to `kind: secret` with no default.

### Variable suggestions (User Story 5)

- **FR-014**: The core MUST suggest variables for literal values; suggestions MUST never be applied without the user accepting them (so `kb add --yes` never applies any).
- **FR-015**: Suggestions MUST only be made for these patterns, so they stay predictable:
  - environment names (`prod`, `production`, `stage`, `staging`, `dev`, `qa`) → `env`, `enum` with the matching options;
  - AWS region codes (`us-east-1` and so on) → `region`, `text`;
  - relative time ranges (`-30m`, `24h`, `7d`, `INTERVAL '1 day'`) → `minutes`/`hours`/`days`, `number`;
  - AWS account IDs (12 digits in an ARN or after `account`) → `account`, `text`;
  - UUIDs and long numeric IDs → `id`, `text`.
- **FR-016**: Accepting a suggestion MUST replace every occurrence of the same value with the variable and add a declaration whose default is the original value, so the resolved entry still produces the exact pasted text.

### Saving

- **FR-017**: Every save path (form, `--yes`, editor template, `kb edit`) MUST validate the entry with M1's lint before writing. Entries with secret findings MUST NOT be written. Entries with other errors MAY be written only after the user explicitly chooses "save anyway".
- **FR-018**: New files MUST be serialized in a canonical layout: frontmatter keys in the order `kb_format`, `type`, `title`, `tags`, type-specific fields, `vars`, `created`, `updated`; a blank line; the payload fence; a blank line; notes.
- **FR-019**: Changes to existing files (for example `updated` after `kb edit`) MUST use M1's format-preserving editing (M1 FR-017), never re-serialize the whole file.
- **FR-020**: Writes MUST be atomic (write to a temporary file in the same directory, then rename) and MUST refuse to overwrite a file that changed on disk since it was read.
- **FR-021**: After any write, rename or delete, the index MUST be updated before the command returns, so the next search reflects it.

### Rename and delete

- **FR-022**: `kb mv` MUST carry usage and remembered variable values to the new ID regardless of content changes (unlike M2's automatic rename detection, which requires identical content).
- **FR-023**: `kb rm` MUST remove the entry's usage and remembered values.
- **FR-024**: When the KB root is a git work tree and `git` is available, `kb mv` and `kb rm` MUST use `git mv` and `git rm`, falling back to plain file operations otherwise.

### TUI (screen 4)

- **FR-025**: The add form MUST match screen 4 of the design: a header with the content source and detected type; on the left the type chips, title, tags and variables, and the path the entry will be saved to; on the right the body with syntax highlighting and an optional notes field; and the key bar.
- **FR-026**: Keys in the form:

  | Key | Action |
  |---|---|
  | Tab / Shift-Tab | next / previous field |
  | ← → (type field) | change type |
  | Ctrl-S | save |
  | Ctrl-E | edit the body in the editor |
  | Ctrl-P | preview the entry as it will appear in the list |
  | Space (on a variable suggestion) | accept the suggestion |
  | Esc | discard (with confirmation if changed) |

- **FR-027**: New keys in the main list: Ctrl-N add, Ctrl-R rename, Ctrl-X delete. `Ctrl-N` is removed from the "next result" binding (`↓` and `Ctrl-J` keep it). Ctrl-E (edit) stays as in M3. The M3 KeyMap rule against duplicate bindings still applies.

### Commands

- **FR-028**: `kb add [--title T] [--type T] [--tag T]... [--path P] [--clipboard | --stdin | --empty] [--editor] [--yes] [--json]`.
- **FR-029**: `kb edit <id|query>` (an exact ID wins; a query with several matches exits 1 listing up to 5 IDs and opens nothing), `kb mv <id> <new-id> [--yes]`, `kb rm <id> [--yes]`.
- **FR-030**: Exit codes: `0` saved, renamed or deleted; `1` validation failure, not found, target exists, a query with several matches, the file changed while saving, or missing title with `--yes`; `2` usage or I/O errors; `130` cancelled.

### Key Entities

- **Draft**: an entry being created or edited: source of the content, detected type and confidence, fields, body, notes, variable declarations, pending variable suggestions, target path, and the on-disk version it was based on (for conflict detection).
- **Detection**: type, confidence, and any fields pre-filled from the content (for example from a GitHub URL).
- **VariableSuggestion**: the literal value, its positions in the body, the proposed name, kind, options and default.

## Success Criteria *(mandatory)*

- **SC-001**: Capturing a query that is on the clipboard (open kb, Ctrl-N, type a title, Ctrl-S) takes under 30 seconds and at most 5 key presses besides the title text.
- **SC-002**: On a fixture set of at least 100 real-world snippets (at least 10 per type), type detection picks the right type for 95% or more, and never picks with high confidence a type that is wrong.
- **SC-003**: Every file written by kb in the test suite passes `kb lint` with no errors and round-trips byte for byte (M1 SC-001).
- **SC-004**: No test path (form, `--yes`, editor template, `kb edit`) can write an entry that contains a fixture secret.
- **SC-005**: After `kb mv`, the entry's position in an empty-query search (frecency order) is unchanged.
- **SC-006**: Accepting all variable suggestions on a pasted snippet and then resolving the entry with its defaults reproduces the original pasted text exactly.

## Out of Scope

- Committing and syncing changes (M10).
- Running or opening entries (M7).
- Checking code links against the repository and re-pinning them (M8). This milestone only fills code fields from a pasted URL.
- Importing from other tools (navi, pet, shell history) in bulk.
- AI-generated titles, tags or descriptions.
- Editing frontmatter fields one by one in the TUI for existing entries; `kb edit` and Ctrl-E cover editing.

## Assumptions

- Most captures start from the clipboard on a local machine, so reading the clipboard is the default source and SSH sessions fall back to stdin.
- The suggestion patterns listed are enough to be useful at first. More can be added later without changing the format.
- Entries the user saves with errors ("save anyway") stay out of search until fixed, as defined in M2, and that is acceptable because the user chose it.
