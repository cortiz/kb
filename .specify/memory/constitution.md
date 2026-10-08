# kb Constitution

kb is a personal, serverless knowledge base for reusable operational snippets: SQL, Sumo Logic and
CloudWatch queries, commands, prompts, links and code locations. It is terminal-only: a single `kb`
binary that provides both a CLI and a full-screen TUI over one shared core.

## Core Principles

### I. Plain Files Are the Source of Truth

- Every entry MUST be a single UTF-8 Markdown file with YAML frontmatter, stored under the KB root
  (default `~/kb`).
- The file format MUST be readable and editable by hand in any text editor, with no tool required.
- Every file MUST declare `kb_format: <n>`. Format changes MUST ship with an automatic, lossless
  migration.
- kb MUST never write anything to the KB root except entry files and the `.kb/` directory.

**Rationale:** the data must outlive the tool. Plain files diff, grep, sync and back up with
standard tools.

### II. The Index Is Disposable

- Search state (SQLite + FTS5) MUST live in `.kb/index.db`, MUST be git-ignored, and MUST be fully
  rebuildable from the files with `kb reindex`.
- Deleting the index MUST lose nothing except usage statistics. Usage counts and last-used variable
  values live in `.kb/state.json`, which is also git-ignored.
- There are no schema migrations for the index: a version mismatch triggers a rebuild.

**Rationale:** derived data that can always be thrown away removes a whole class of corruption and
migration bugs.

### III. Local-Only, No Server

- kb MUST run with no daemon, no server process and no account.
- The only network activity allowed is `git` during an explicit or configured sync, and opening
  URLs the user selected.
- There MUST be no telemetry, analytics or automatic update checks.

**Rationale:** the KB holds production query patterns and internal links. It stays on the machine.

### IV. Terminal Only, One Core

- kb is 100% terminal. There is no GUI, webview or desktop app.
- All domain logic (parsing, validation, indexing, search, variable resolution, actions, code-link
  resolution) MUST live in the `kb-core` crate.
- The CLI commands and the TUI MUST be thin layers over `kb-core` that only handle input, rendering
  and platform glue. They MUST NOT duplicate core logic.
- Every TUI action MUST also be available as a scriptable CLI command, and every command MUST
  support `--json` output so kb composes with `fzf`, `jq` and shell scripts.

**Rationale:** one binary in the terminal where the work already happens. A clean core keeps the
CLI and TUI consistent and leaves the door open to other front ends later without committing to one
now.

### V. Portable by Construction

- macOS (arm64 and x86_64) and Linux (x86_64, Wayland and X11) are first-class targets from M1
  onward.
- All OS-specific behavior (clipboard, URL and file opening, config and data paths, shell
  execution) MUST sit behind a trait in a `platform` module, with one implementation per OS.
- The TUI MUST work in any modern terminal emulator over SSH and inside tmux. Clipboard falls back
  to OSC 52 when no local clipboard is available.
- Paths follow the platform's conventions (XDG on Linux, `~/Library` on macOS for app data). The KB
  root is configurable.

**Rationale:** porting later is expensive. Isolating platform code from day one makes it free.

### VI. Secrets Are Never Stored

- Entries MUST NOT contain credentials, tokens or keys. Secret values are supplied at use time
  through variables of kind `secret`, read from environment variables or prompted for, and never
  persisted.
- `kb lint` and every save path MUST reject content matching known secret patterns (AWS access
  keys, bearer tokens, private-key headers, high-entropy strings in assignments).
- Remembered variable values MUST exclude `secret` variables.

**Rationale:** a KB synced through git is one push away from leaking whatever it contains.

### VII. Keyboard-First and Fast

- Every action MUST be reachable from the keyboard. Mouse support in the TUI is optional and
  additive.
- Performance budgets, measured on a KB of 10,000 entries:
  - search results in ≤ 50 ms,
  - CLI cold start in ≤ 100 ms,
  - TUI first paint ≤ 100 ms after launch.
- Running a command entry MUST show the fully resolved command and require confirmation before
  executing it.

**Rationale:** the tool is used mid-incident. Being slow or surprising defeats its purpose.

### VIII. Test-First Core

- `kb-core` changes MUST follow red-green-refactor: tests are written and fail before the
  implementation.
- A fixture KB in `tests/fixtures/` covers every entry type, and parsers MUST pass round-trip tests
  (parse, then serialize, gives identical output).
- CI MUST run format checks, `clippy -D warnings` and all tests on both macOS and Linux. A milestone
  is not done until CI is green on both.

**Rationale:** the core is shared by every front end, so its regressions break everything at once.

### IX. Simplicity First

- No GUI, plugin system, sync service, multi-user features or encryption layer unless a
  constitution amendment adds them.
- Dependencies are added only when they remove meaningful complexity, and each one is justified in
  the plan.
- Every milestone MUST leave a usable tool. No milestone may depend on a future one to be useful.

**Rationale:** this is a personal tool. Scope creep is the main risk to it ever getting finished.

## Technical Constraints

- **Language:** Rust (stable), organized as a Cargo workspace: `kb-core` (library) and `kb` (binary
  with the CLI and the TUI).
- **Storage:** `rusqlite` with bundled SQLite and FTS5. Frontmatter is parsed with `serde` and a
  YAML parser.
- **TUI:** `ratatui` + `crossterm`. Syntax highlighting via `syntect`.
- **CLI:** `clap` with generated shell completions for bash, zsh and fish.
- **Shell integration:** a shell widget (zsh, bash, fish) that opens a compact picker and inserts
  the resolved command into the prompt line.
- **Entry types (v1):** `sql`, `sumo`, `cw`, `cmd`, `prompt`, `link`, `code`. Adding a type is a
  minor change to the format.
- **Variables:** written as `{{name}}`, with kinds `text`, `number`, `enum` and `secret`, each with
  an optional default.
- **Distribution:** a single static binary, published through a Homebrew tap (macOS and Linux),
  `cargo install`, and release tarballs for macOS (arm64, x86_64) and Linux (x86_64, aarch64).

## Development Workflow

- Every milestone goes through Spec Kit: `/specify`, then `/clarify` if anything is ambiguous, then
  `/plan`, `/tasks`, `/implement`.
- Every plan MUST include a Constitution Check that lists each principle as pass or justified
  exception.
- One branch per milestone (`NNN-short-name`), merged when the milestone's acceptance criteria and
  CI pass.
- Commits follow Conventional Commits.
- User-visible behavior is documented in `README.md` within the same change that introduces it.

## Governance

- This constitution takes precedence over specs, plans and tasks. Where they conflict, the
  constitution wins until it is amended.
- Amendments are made in a dedicated commit that updates this file, explains the reason, and bumps
  the version:
  - MAJOR for removing or redefining a principle,
  - MINOR for adding a principle or section,
  - PATCH for clarifications.
- Exceptions granted in a plan apply only to that milestone and MUST be recorded in its plan.

**Version**: 1.0.0 | **Ratified**: 2026-10-08 | **Last Amended**: 2026-10-08
