# Feature Specification: Entry Format and Parser

**Feature Branch**: `001-entry-format`
**Created**: 2026-10-08
**Status**: Draft
**Milestone**: M1
**Input**: User description: "Define the on-disk format for kb entries, parse and validate it, and give the user `kb init` and `kb lint` to create a KB and check it."

## Overview

A kb entry is one Markdown file with YAML frontmatter. This milestone defines that format precisely,
and delivers the parser, validator and two commands built on them. Every later milestone (indexing,
TUI, variables, actions, code links) reads entries through this parser, so the format is the
contract the rest of the project depends on.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Write an entry by hand and know it is valid (Priority: P1)

Carlos creates `~/kb/sumo/5xx-by-authorizer.md` in his editor, following the documented format, then
runs `kb lint`. He gets either a clean result or a precise list of problems with file, line and a
suggested fix.

**Why this priority**: until the tool can create entries (M6), writing files by hand is the only way
to add them. A trustworthy validator is the minimum useful product.

**Independent Test**: write one valid and one invalid entry of each type in a fixture folder, run
`kb lint`, and check the exit code and every reported finding.

**Acceptance Scenarios**:

1. **Given** a KB containing only valid entries, **When** the user runs `kb lint`, **Then** it prints
   a one-line summary (`284 entries, 0 errors, 0 warnings`) and exits with code 0.
2. **Given** an entry whose `type` is `sumologic` (not a known type), **When** the user runs
   `kb lint`, **Then** it reports an error at that file and line, lists the valid types, suggests
   `sumo`, and exits with code 1.
3. **Given** an entry with warnings but no errors, **When** the user runs `kb lint`, **Then** it
   reports the warnings and exits with code 0, or with code 1 when `--strict` is passed.
4. **Given** the user runs `kb lint sumo/5xx-by-authorizer.md`, **When** that file is valid,
   **Then** only that file is checked.

---

### User Story 2 - Start a new KB (Priority: P1)

Carlos runs `kb init` to create a KB root that is ready for git.

**Why this priority**: every other story needs a KB root, and the layout must be right from the
start.

**Independent Test**: run `kb init` in an empty temporary directory and check the resulting files.

**Acceptance Scenarios**:

1. **Given** the directory doesn't exist or is empty, **When** the user runs `kb init [path]`
   (default: the resolved KB root, FR-022, which is `~/kb` unless configured), **Then** kb
   creates the root, a `.kb/` directory, a `.kb/.gitignore`
   containing `*` so git ignores everything inside `.kb/` (nothing is written to the KB root
   itself), and one example entry for each type, all of which pass lint.
2. **Given** the directory already contains entries, **When** the user runs `kb init`, **Then** kb
   adds only what is missing (`.kb/` and `.kb/.gitignore`), changes no existing file, and
   reports what it added.
3. **Given** `--no-examples` is passed, **Then** no example entries are created.

---

### User Story 3 - Scripts and later milestones consume entries reliably (Priority: P2)

Tools and later kb features read parsed entries through `kb lint --json` or the core library, and
reading or rewriting a file never changes it unexpectedly.

**Why this priority**: the index (M2), authoring (M6) and re-pinning (M8) all rewrite or read files.
Losing a comment or reordering keys would erode trust in the plain-file principle.

**Independent Test**: for every file in the fixture KB, parse it and serialize it back, then compare
bytes.

**Acceptance Scenarios**:

1. **Given** any valid entry, **When** it is parsed and serialized back without changes, **Then** the
   output is byte-for-byte identical to the input, including key order, comments, quoting style and
   line endings.
2. **Given** an entry with a frontmatter key kb doesn't recognize, **When** it is parsed and
   serialized, **Then** the key and its value are preserved and lint reports an informational
   finding.
3. **Given** `kb lint --json`, **Then** the output is a single JSON document with each finding's
   file, line, column, severity, rule code, message and suggested fix.

---

### Edge Cases

- A file without frontmatter, or with an opening `---` but no closing one: error, located at line 1.
- An empty file, or a file that is only whitespace: error.
- A UTF-8 byte order mark: accepted and preserved. A file that isn't valid UTF-8: error naming the
  first bad byte offset.
- CRLF line endings: accepted and preserved. Mixed line endings: warning.
- A file larger than 1 MB: warning (entries are meant to be small).
- Non-`.md` files, and anything under hidden directories (`.git/`, `.kb/`): ignored silently.
- Symlinks: followed when they point inside the KB root, otherwise reported as a warning and
  skipped.
- `kb_format` greater than the highest version this kb supports: error telling the user to upgrade
  kb, and the file is never rewritten.
- `kb_format` missing, not an integer, or less than 1: error.
- Two entries whose IDs differ only by letter case (`SQL/a.md` and `sql/a.md`): error, on every
  platform, because the KB may be synced to a case-insensitive file system.
- A payload containing `{{` that is not meant as a variable: escaped as `\{{`, and the escape is
  reported back as a literal `{{` by the parser.
- A `sql`, `sumo`, `cw`, `cmd` or `prompt` entry with no fenced code block in the body: error. A
  payload code block that is empty or only whitespace: warning.

## Requirements *(mandatory)*

### Functional Requirements

#### Entry file format

- **FR-001**: An entry MUST be a UTF-8 file with the `.md` extension anywhere under the KB root,
  outside hidden directories.
- **FR-002**: The entry's **ID** MUST be its path relative to the KB root, without the `.md`
  extension, using `/` as separator (for example `sumo/5xx-by-authorizer`). No ID is stored inside
  the file.
- **FR-003**: The file MUST start with a YAML frontmatter block delimited by `---` lines, followed by
  a Markdown body.
- **FR-004**: The frontmatter MUST contain:
  - `kb_format` (integer; this milestone defines version `1`),
  - `type` (one of `sql`, `sumo`, `cw`, `cmd`, `prompt`, `link`, `code`),
  - `title` (non-empty string, at most 120 characters).
- **FR-005**: The frontmatter MAY contain:
  - `tags`: a list of strings, normalized to lowercase without a leading `#`; duplicates after
    normalization are a warning;
  - `vars`: a map of variable definitions (FR-008);
  - `created` and `updated`: ISO 8601 dates or date-times;
  - the type-specific fields in FR-006.
- **FR-006**: Type-specific fields:
  - `sql`: optional `dialect` (for example `postgres`, `mysql`), optional `database`.
  - `sumo`: optional `range` (for example `-15m`; may use variables).
  - `cw`: optional `log_groups` (list of strings), optional `range`, optional `region`.
  - `cmd`: optional `shell` (`sh`, `bash`, `zsh`, `fish`; default `sh`), optional `cwd`.
  - `prompt`: no extra fields.
  - `link`: required `url` (absolute `http` or `https` URL).
  - `code`: required `code` map with `repo` (`owner/name`, or a full git remote URL), `path`
    (relative to the repo root), `line` (positive integer) and `ref` (commit SHA, tag or branch),
    and optional `symbol` and `end_line`.
- **FR-007**: The **payload** of `sql`, `sumo`, `cw`, `cmd` and `prompt` entries MUST be the first
  fenced code block in the body. Longer fences (four or more backticks, or tildes) MUST be supported
  so prompts can contain code blocks. All body text outside the payload is **notes**. For `link` and
  `code` entries the whole body is notes, and the body may be empty.

#### Variables

- **FR-008**: A variable definition under `vars.<name>` MUST have a `kind`: `text`, `number`, `enum`
  or `secret`. It MAY have:
  - `default` (not allowed for `secret`),
  - `options` (a non-empty list, required for `enum`; `default` must be one of the options),
  - `description`,
  - `env` (name of an environment variable to read; for `secret`, either `env` or prompting is the
    only allowed source).
- **FR-009**: A variable reference MUST be written `{{name}}`, where the name matches
  `[a-z][a-z0-9_]*`. References MUST be recognized in the payload and in the string values of
  `range`, `url`, `cwd` and `log_groups`.
- **FR-010**: A reference to an undeclared variable MUST be accepted and treated as `kind: text` with
  no default, and lint MUST report it as informational. A declared variable that is never referenced
  MUST be a warning.
- **FR-011**: `\{{` MUST be treated as a literal `{{` and never as a variable reference. The file
  bytes keep the `\{{` (FR-016); only the parsed payload value presents it as `{{`.

#### Validation (lint)

- **FR-012**: Each finding MUST have a severity (`error`, `warning`, `info`), a stable rule code (for
  example `KB001-unknown-type`), a file, a 1-based line and column, a human-readable message, and a
  suggested fix when one exists.
- **FR-013**: Lint MUST flag likely secrets as **errors**: AWS access key IDs, AWS secret keys in
  assignments, bearer tokens, JWTs, private-key headers, and high-entropy strings assigned to names
  such as `password`, `token`, `secret` or `api_key`. Secret detection MUST cover the whole file,
  frontmatter and body. A finding MUST be suppressible for one line with a trailing
  `kb:allow-secret` marker, and every suppression MUST appear in the summary.
- **FR-014**: Lint MUST report the edge cases listed above with the severities given there.
- **FR-015**: Unknown frontmatter keys MUST be reported as informational and preserved.

#### Parser guarantees

- **FR-016**: Parsing then serializing an unmodified entry MUST reproduce the original bytes
  exactly.
- **FR-017**: Changing a single field through the core API (for example `code.line` or `updated`)
  MUST change only the bytes of that value and leave every other byte, comment and key order
  unchanged.
- **FR-018**: The parser MUST never write to a file whose `kb_format` it doesn't support.

#### Commands

- **FR-019**: `kb init [path]` MUST behave as described in User Story 2. `kb init` MUST never
  overwrite or delete an existing file, and MUST write nothing to the KB root except example
  entries and the `.kb/` directory; a root `.gitignore` is never created or edited.
  It MUST support `--json`, reporting each path it created and each it left untouched.
- **FR-020**: `kb lint [paths...]` MUST check the whole KB when no path is given, accept files and
  directories, and support `--json`, `--strict` and `--quiet` (summary only).
- **FR-021**: Exit codes MUST be `0` for no errors (and no warnings under `--strict`), `1` when
  findings fail the run, and `2` for usage or I/O errors (for example the KB root is missing).
- **FR-022**: The KB root MUST be resolved in this order: `--root` flag, the `KB_ROOT` environment
  variable, the config file, then `~/kb`.

### Key Entities

- **Entry**: ID, file path, type, title, tags, variables, type-specific fields, payload, notes,
  timestamps, plus a lossless representation of the original file.
- **EntryType**: one of the seven types, with its own required and optional fields and whether it
  has a payload.
- **Variable**: name, kind, default, options, description, environment source, and every location
  where it is referenced.
- **CodeRef**: repo, path, line, end line, ref and symbol.
- **Finding**: severity, rule code, location, message and suggested fix.
- **LintReport**: the findings plus counts of entries, errors, warnings, info and suppressions.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of the fixture entries round-trip byte-for-byte, on both macOS and Linux.
- **SC-002**: `kb lint` checks a KB of 10,000 entries in under 2 seconds on a recent laptop.
- **SC-003**: Every error message names the file and line, and at least 80% of error rule codes
  include a suggested fix.
- **SC-004**: A first-time user can run `kb init`, open an example entry, copy it to write a new one,
  and get a clean lint in under 5 minutes, using only the examples and the `kb lint` output.
- **SC-005**: Every secret pattern in the fixture set is detected, and none of the clean fixture
  entries produces a secret finding.

## Example Entries

A Sumo query with variables:

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

A code link:

```markdown
---
kb_format: 1
type: code
title: AuthorizerHandler.handleRequest
tags: [lambda, auth]
code:
  repo: snapfinance/lambda-authorizer
  path: src/main/java/com/snap/auth/AuthorizerHandler.java
  line: 147
  ref: a1b9c3e
  symbol: AuthorizerHandler.handleRequest
---

Entry point for every authorization request.
```

## Out of Scope

- The search index and the `kb search` command (M2).
- Copying to the clipboard, and `kb show` (M3).
- Resolving and prompting for variables at use time (M5); this milestone only parses and validates
  them.
- Creating or editing entries through kb (M6).
- Opening links and running commands (M7).
- Checking code links against the repository (M8).
- Format migrations; only `kb_format: 1` exists.

## Clarifications

### Session 2026-10-08

- Q: Where does the rule that keeps `.kb/` out of git live, given constitution Principle I? → A:
  In `.kb/.gitignore` containing `*`; kb never creates or edits a root `.gitignore`.

## Assumptions

- Hand-editing files is the main way to add entries until M6, so lint output must be friendly enough
  to learn the format from.
- The file name has no meaning beyond the ID; grouping entries into folders by type is a convention,
  not a rule.
- `title` uniqueness is not enforced, because the ID is already unique.
- The `kb:allow-secret` marker is recognized at the end of a line whatever comment syntax the
  payload's language uses (`--`, `#`, `//`), since entries hold many languages.
- `kb init` adds example entries only when the target holds no entries yet; in a KB that already has
  entries it adds only `.kb/` and `.kb/.gitignore`.
- The config file's location and format follow the platform conventions in the constitution
  (Principle V) and are decided in the plan; this milestone reads only the KB root setting from it.
- Case-only ID collisions are errors on every platform, not only on case-insensitive file systems,
  so a KB that lints clean on Linux also works on macOS.
