# Feature Specification: MCP Server

**Feature Branch**: `012-mcp-server`
**Created**: 2026-10-11
**Status**: Draft
**Milestone**: MCP (spec 012; runbook run mode and run records become specs 013 and 014)
**Depends on**: M1 (`001-entry-format`), M2 (`002-index-search`), M5 (`006-variables`), M6 (`007-authoring`), M7 (`008-actions-code-links`), M9 (`009-git-sync`), R1 (`011-runbook-format`)
**Input**: Expose the KB to AI agents (Claude Code and other MCP clients) through `kb mcp`, a local stdio MCP server that lets an agent **search**, **read**, **create** and **edit** entries, with the same validation, secret rules and history as every other kb front end, and with safeguards against an agent quietly changing what the user later runs.

## Overview

The KB holds exactly what an agent needs while helping with operational work: the right Sumo query for 5xx errors on the authorizer, the command that tails its logs, the code location of the handler, the runbook for deploying it. Without access, the agent writes its own versions, which are usually close but wrong in the details that matter.

`kb mcp` is a new front end over the same core (principle IV). The agent's MCP client starts it as a child process and talks to it over stdin and stdout for the length of the session, so there is still no daemon, no network listener and no account (principle III).

Reading and searching are on by default. Creating and editing are supported but must be turned on explicitly, because an agent that can change entries can change commands the user will later run. Every change an agent makes goes through the M6 save path (validation, secret gate, atomic write, auto-commit), is marked as agent-made in history, and is flagged as unreviewed until the user looks at it.

## Clarifications

### Session 2026-10-11

- Q: How should the interactive `kb review` command present diffs and review choices to the user? (FR-032) → A: Terminal interactive prompt (prints unified diff sequentially with `[a]pprove, [r]evert, [e]dit, [s]kip (a/r/e/s)?`), while the TUI uses the `unreviewed:true` filter for full-screen browsing.
- Q: How should modifications to an entry's tags and variables be structured in the MCP `update` tool `changes` payload? (FR-019) → A: Both deltas and full replacement: `changes` accepts incremental delta keys (`add_tags`, `remove_tags`, `set_vars`, `remove_vars`) as well as full replacements (`tags`, `vars`).
- Q: How should the MCP `create` and `update` tools handle variable placeholders `{{name}}` in payloads that are not declared in `vars`? (FR-022) → A: Auto-declare as `kind: text`: Automatically add `kind: text` declarations for referenced placeholders missing from `vars` before validating and saving, matching M6 authoring behavior.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — The agent finds and uses my queries (Priority: P1)

In Claude Code, Carlos asks "what's the 5xx rate on the authorizer in prod for the last 30 minutes?". The agent searches the KB, finds "5xx by authorizer", reads it, resolves `env=prod` and `minutes=30`, and gives him the exact query, with a link to open it in Sumo.

**Why this priority**: this is the main value of the milestone: the agent uses the user's curated knowledge instead of improvising.

**Independent Test**: an MCP test client starts `kb mcp` against the fixture KB, calls `search` and `get` with a table of inputs, and compares the structured results with expected outputs.

**Acceptance Scenarios**:

1. **Given** the agent calls `search` with `query: "5xx auth"`, **Then** it receives ranked results (same ranking as `kb search`, M2), each with ID, type, title, tags, a snippet and a score.
2. **Given** the agent calls `get` with an ID, **Then** it receives the entry: type, title, tags, type-specific fields, payload, notes, declared variables (kind, options, default, description), and a `version` string for later edits.
3. **Given** `get` with `resolve: true` and `vars: {env: "prod", minutes: 30}`, **Then** the response also includes the resolved payload and resolved fields (M5), plus any links the M7 actions would build (Sumo or CloudWatch console URL, link URL, code permalink).
4. **Given** `resolve: true` but some required variables have no value, **Then** the tool returns an error listing the missing variables with their kinds and options, so the agent can ask the user or choose.
5. **Given** a `cmd` entry, **Then** `get` returns the resolved command as text and never runs it; the agent runs it with its own tools, under its own approval prompts.

---

### User Story 2 — Secrets never reach the agent (Priority: P1)

An entry needs `token` (`kind: secret`, `env: SUMO_TOKEN`). When the agent resolves it, the command contains `"$SUMO_TOKEN"`, never the token.

**Why this priority**: constitution principle VI. Whatever reaches the agent may be logged, cached or sent to a model provider.

**Independent Test**: test suite resolves fixture entries containing secrets through all MCP tools, prompts and error paths, verifying that no secret value is present in any response or log.

**Acceptance Scenarios**:

1. **Given** a secret variable with an `env` source, **When** the entry is resolved for the agent, **Then** the secret is replaced by a reference to that environment variable in the entry's shell syntax, as in M8 FR-014.
2. **Given** a secret variable without an `env` source, **Then** the resolved payload keeps the `{{name}}` placeholder for that variable, and the response says the user must supply it outside the agent.
3. **Given** a value for a secret variable is passed in `vars`, **Then** the tool rejects the call, without echoing the value.
4. **Given** any tool response, **Then** it never contains a secret value, a remembered value of a secret, or the contents of an environment variable.

---

### User Story 3 — The agent saves something useful it just built (Priority: P1)

After an investigation, the agent has written a good CloudWatch Insights query. Carlos says "save that to my KB". The agent calls `create`; the entry is validated, saved, committed, and appears in kb with an "agent" badge until Carlos reviews it.

**Why this priority**: capture is where knowledge bases usually fail, and agents produce useful snippets constantly.

**Independent Test**: with writing enabled, client calls `create` with valid content; verify entry is validated, written to disk, indexed, committed with agent trailer, and marked unreviewed.

**Acceptance Scenarios**:

1. **Given** writing is enabled (FR-020), **When** the agent calls `create` with type, title, tags, payload, notes and variable declarations, **Then** kb validates the entry (M1 lint, secret checks), writes it at the default path (M6 FR-008) or the given one, updates the index, commits it (M9) with an agent marker (FR-028), and returns the new ID and version.
2. **Given** the type is omitted, **Then** kb uses M6 type detection, and returns an error asking for an explicit type if confidence is low.
3. **Given** the content contains a secret, **Then** nothing is written, and the error names the rule and line but not the secret text.
4. **Given** `dry_run: true`, **Then** kb returns the exact file it would write and any lint findings, and writes nothing.
5. **Given** writing is not enabled, **Then** `create` and `update` are not offered by the server at all.

---

### User Story 4 — The agent improves an existing entry (Priority: P2)

Carlos asks the agent to add a `region` variable to an existing CloudWatch query and tag it `prod`. The agent reads the entry, then calls `update` with only those changes. Everything else in the file stays exactly as it was.

**Why this priority**: iterative refinement of snippets without manual editing.

**Independent Test**: client reads an entry version, updates fields via `update`, and verifies byte-for-byte preservation outside the edits, version check enforcement, and git commit trailers.

**Acceptance Scenarios**:

1. **Given** the agent calls `update` with an ID, the `version` it read, and a set of changes, **Then** kb applies only those changes using format-preserving edits (M1 FR-017), validates the result, writes it, updates the index and commits it with the agent marker.
2. **Given** the file changed since the agent read it (the `version` no longer matches), **Then** nothing is written, and the error includes the current version so the agent can re-read and retry.
3. **Given** a change that would make the entry fail validation, **Then** nothing is written and the findings are returned.
4. **Given** `dry_run: true`, **Then** kb returns a unified diff of the change and the findings, and writes nothing.
5. **Given** the entry is used by runbooks (R1), **Then** the response lists them, so the agent can mention that the change affects them.

---

### User Story 5 — I see and approve what the agent changed (Priority: P1)

The next morning Carlos opens kb. Two entries carry an "agent" badge. He runs `kb review`, reads the diff of each change since he last approved it, and approves one and reverts the other.

**Why this priority**: an agent can be wrong, or be manipulated by content it read elsewhere (prompt injection). The most dangerous outcome is a changed command that the user later runs from kb without noticing.

**Independent Test**: verify unreviewed badges in TUI/CLI/widget, confirmation elevation on unreviewed command execution, interactive `kb review` diff presentation, approval commits, and reversion flow.

**Acceptance Scenarios**:

1. **Given** an entry whose latest change was made through MCP and not yet reviewed on this machine, **Then** the TUI list and preview show an "agent" badge, `kb show` mentions it, and `kb search --json` includes `unreviewed: true`.
2. **Given** the user runs an unreviewed `cmd` entry (M7), opens an unreviewed `link`, or uses an unreviewed entry in the shell widget (M8), **Then** kb shows a warning that it was changed by an agent and not yet reviewed, and requires the stronger confirmation (typing `run`, as for destructive commands, M7 FR-020); for open and insert, a `y` confirmation.
3. **Given** `kb review`, **Then** kb lists unreviewed entries, and for each shows the diff since the last version the user approved (or the whole entry, if it was created by the agent), with choices: **a**pprove, **r**evert to the last approved version (committed as `kb: revert agent change to <id>`), **e**dit, or **s**kip.
4. **Given** the user edits an unreviewed entry themselves through kb (M6), **Then** it counts as reviewed.
5. **Given** the user approves on one machine, **Then** other machines also see it as reviewed after sync (FR-030).

---

### User Story 6 — Prompts become slash commands (Priority: P2)

Carlos's `prompt` entries ("Summarize a 5xx spike from logs") appear in Claude Code as prompts he can pick, with their variables as arguments.

**Why this priority**: makes curated reasoning templates easily discoverable and executable directly within the agent environment.

**Independent Test**: create prompt entries with variables, query MCP prompt list and get prompt results, verifying arguments, required defaults, and exclusion of prompts with secrets or excluded tags.

**Acceptance Scenarios**:

1. **Given** `prompt` entries, **Then** the server offers them as MCP prompts, named from their IDs, described by their titles, with one argument per variable (required when there is no default).
2. **Given** the user picks one and fills the arguments, **Then** the server returns the resolved prompt text (M5).
3. **Given** a prompt entry with a secret variable, or tagged with an excluded tag (FR-016), **Then** it is not offered.

---

### User Story 7 — Easy setup in my agent (Priority: P2)

Carlos runs `kb mcp setup` and registers kb with Claude Code and other MCP clients in seconds.

**Why this priority**: reduces integration friction so users can start using MCP immediately.

**Independent Test**: run `kb mcp setup` and `kb doctor` to verify correct client config snippets and operational health checks.

**Acceptance Scenarios**:

1. **Given** `kb mcp setup`, **Then** kb prints the exact command or configuration snippet to register the server with Claude Code (`claude mcp add kb -- kb mcp`) and the generic JSON configuration used by other MCP clients, with `--allow-write` shown as an option and explained.
2. **Given** `kb doctor` (M4), **Then** it also checks that `kb mcp` starts and answers a basic request.

---

### Edge Cases

- **Huge results**: `search` caps results (default 20, maximum 100); `get` caps payloads at the M1 1 MB limit and notes larger than 64 KB are truncated with a marker and an `include_full_notes` option.
- **Runbooks**: `get` on a runbook returns its steps and binding table (R1 FR-011); with `resolve: true`, each step's referenced entry is resolved with the runbook variables, so the agent can walk through it. Runbook execution itself stays in kb (R2).
- **Code entries**: `get` returns the code fields and the permalink; with `include_code: true` it returns the snippet from the pinned ref and the drift status (M7), when a local clone is known.
- **Entries with lint errors**: not returned by `search` (M2 behavior); `get` on one returns it with its findings, so the agent can help fix it.
- **Path in `create`**: must be a relative ID inside the KB root, with no `..`, no hidden directories and no absolute path; otherwise the call fails.
- **ID already exists**: `create` fails, suggesting `update` or a different path.
- **Concurrent use**: the TUI, the CLI and the MCP server can run at the same time; the index handles concurrency (M2 FR-004) and writes use version checks.
- **Not a git repository**: writes still work; the agent marker is stored locally only (FR-029), and the review flow still works on this machine.
- **Client disconnects mid-write**: writes are atomic (M6 FR-020), so either the whole change is on disk and committed, or nothing is.
- **Agent sends a deliberately oversized or malformed request**: rejected with a clear error; the server keeps running.
- **Entry text containing instructions aimed at the agent** ("ignore previous instructions…"): returned as data. Tool descriptions state that entry content is user-written data, not instructions; kb cannot stop an agent from obeying it, which is why the review flow exists.

## Requirements *(mandatory)*

### Server

- **FR-001**: `kb mcp` MUST run a Model Context Protocol server over stdio only, implementing the current stable MCP specification revision at planning time. It MUST NOT open network sockets.
- **FR-002**: The server MUST resolve the KB root as every kb command does (`--root`, `KB_ROOT`, config, `~/kb`), and serve one KB per process.
- **FR-003**: stdout MUST carry only protocol messages; all logs and diagnostics MUST go to stderr.
- **FR-004**: The server MUST use only the core APIs (M2 search, M1 parsing, M5 resolution, M6 save path, M7 actions, R1 runbooks); it MUST NOT read or write entry files or the index directly.
- **FR-005**: The server MUST start and answer its first request within 150 ms on a KB of 10,000 entries with a warm index, and each `search` call MUST meet M2's 50 ms budget.

### Read tools (always available)

- **FR-006**: `search` — inputs: `query` (M2 syntax, may be empty), `types`, `tags`, `limit` (default 20, max 100). Output: results with `id`, `type`, `title`, `tags`, `snippet`, `score`, `unreviewed`.
- **FR-007**: `get` — inputs: `id`; optional `resolve` (bool), `vars` (map), `include_code` (bool), `include_full_notes` (bool). Output: the entry's fields, payload, notes, variable declarations, `version`, `unreviewed`, "used in" runbooks (R1), lint findings if any; with `resolve`, the resolved payload and fields and the action links (M7 console URLs, link URL, code permalink).
- **FR-008**: `list_tags` — output: every tag with its entry count, so agents reuse the user's vocabulary when creating entries.
- **FR-009**: Read tools MUST NOT record usage (M2 FR-017) by default, so agent activity doesn't distort the user's frecency ranking. `mcp.record_usage = true` MAY enable it.
- **FR-010**: Every tool response MUST include both structured content and a short human-readable text form.

### Variables and secrets

- **FR-011**: Resolution MUST use the M5 engine and validation. Remembered values MUST NOT be used as pre-fills for the agent by default (`mcp.use_remembered = false`), so results don't depend on what the user last typed; declared defaults are used.
- **FR-012**: Secret variables MUST follow User Story 2: environment-variable references when an `env` source exists, the unresolved placeholder otherwise, and rejection of any secret value passed in `vars`.
- **FR-013**: No tool response, log line or error message MAY contain a secret value. Secret-detection errors on writes MUST name the rule and location, never the matched text.

### Prompts

- **FR-014**: When `mcp.prompts = true` (default), every `prompt` entry without secret variables and without an excluded tag MUST be offered as an MCP prompt (User Story 6). The prompt list MUST update when entries change, within the time of the next request.
- **FR-015**: Prompt names MUST be derived from entry IDs in a form clients accept (lowercase, `-` and `_`), and be stable across sessions.

### Read scope

- **FR-016**: `mcp.exclude_tags` (default `["private"]`) MUST hide entries with those tags from every tool, prompt and response, including "used in" lists and runbook steps (shown as "hidden step").
- **FR-017**: `mcp.exclude_types` MAY hide whole types (for example `cmd`).

### Write tools (opt-in)

- **FR-018**: `create` — inputs: `type` (optional; detected when omitted), `title`, `tags`, `payload`, `notes`, `vars` (declarations), type-specific fields (`url`, `code`, `range`, `log_groups`, `region`, `shell`, `cwd`, `steps` for runbooks…), optional `path`, optional `dry_run`. Output: `id`, `version`, findings; with `dry_run`, the file content that would be written.
- **FR-019**: `update` — inputs: `id`, `version`, and a `changes` object that may set `title`, `payload`, `notes`, type-specific fields, modify tags (via delta keys `add_tags`, `remove_tags`, or full replacement `tags`), and modify variable declarations (via delta keys `set_vars`, `remove_vars`, or full replacement `vars`); optional `dry_run`. Output: the new `version`, findings, "used in" runbooks; with `dry_run`, a unified diff.
- **FR-020**: Write tools MUST be offered only when enabled by `kb mcp --allow-write` or `mcp.write = true`. `--read-only` MUST force them off regardless of config.
- **FR-021**: `mcp.write_types` MAY restrict which types the agent can create or edit (default: all types).
- **FR-022**: Every write MUST go through the M6 save path: M1 lint, secret gate (no "save anyway" for agents: any lint error blocks the write), canonical layout for new files (M6 FR-018), format-preserving edits for changes (M6 FR-019), auto-declaration of undeclared variable placeholders as `kind: text`, atomic write with version check (M6 FR-020), index update (M6 FR-021).
- **FR-023**: `version` MUST be a content hash of the file as read; `update` MUST fail when it doesn't match the file on disk.
- **FR-024**: Rename and delete MUST NOT be available through MCP in this version.
- **FR-025**: `create` MUST reject paths that are absolute, contain `..`, point into hidden directories, or would overwrite an existing entry.
- **FR-026**: Each `create` or `update` call MUST be limited to one entry, and payloads to the M1 size limit.

### Agent changes: history and review

- **FR-027**: With auto-commit on (M9), each agent write MUST be its own commit, with the M9 message template and a `Kb-Source: mcp` trailer, plus the client's name when the client provides it (`Kb-Client: claude-code`).
- **FR-028**: An entry is **unreviewed** when its latest change was made through MCP and the user has not approved it since.
- **FR-029**: Review state MUST be determined from the git trailer when the KB is a git repository, and from a local record in `.kb/state.json` otherwise.
- **FR-030**: Approving MUST record the approval so that it syncs: when the KB is a git repository, as an empty commit `kb: review <id>` with a `Kb-Reviewed: <commit>` trailer; otherwise in `.kb/state.json`. Edits made by the user through kb count as approval (User Story 5, scenario 4).
- **FR-031**: Using an unreviewed entry MUST trigger the warnings and confirmations in User Story 5, scenario 2, in the TUI, the CLI (`kb run`, `kb open`, `kb use`) and the shell widget.
- **FR-032**: `kb review [id…] [--approve-all] [--json]` MUST implement User Story 5, scenario 3 as a terminal interactive prompt presenting colored unified diffs sequentially with choices `[a]pprove`, `[r]evert` (restoring the last approved version through the M6 save path), `[e]dit` (opening configured editor), and `[s]kip`. The full-screen TUI provides inspection via the `unreviewed:true` filter.
- **FR-033**: The M3 header MUST show the number of unreviewed entries when there are any, and `unreviewed:true` MUST be usable as a search filter (M2).

### Audit

- **FR-034**: The server MUST append one line per write call to `.kb/mcp.log` (time, client name, tool, entry ID, outcome), never including entry content or variable values. Read calls MAY be logged with `mcp.log_reads = true`.

### CLI

- **FR-035**: `kb mcp [--allow-write | --read-only] [--root <path>]`, `kb mcp setup`, `kb review`.
- **FR-036**: Exit codes for `kb mcp`: `0` when the client closes the session normally, `2` on startup errors (KB not found, invalid config). Tool-level failures MUST be reported as MCP tool errors, never by exiting.

### Key Entities

- **ToolCall**: tool name, inputs, client name, outcome.
- **EntryView**: the entry as returned to the agent: fields, payload, notes, variables, version, review state, "used in", findings, and optional resolved content and links.
- **ChangeSet**: the changes requested by `update`, validated against the current file.
- **ReviewState**: entry ID, last agent change (commit or local record), last approved version, and whether it is unreviewed.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In a scripted agent session against the fixture KB, an agent finds the intended entry with one `search` call for at least 90% of a 40-question set of realistic operational questions.
- **SC-002**: A test suite that resolves every fixture entry with secrets through every tool, prompt and error path finds no secret value in any response, log or commit.
- **SC-003**: With writing disabled, the server offers no write tools, and no file under the KB root changes during a full test run.
- **SC-004**: Every agent write in the test suite produces exactly one commit with the `Kb-Source: mcp` trailer, changes only what was requested (byte-for-byte outside the change), and leaves the entry flagged as unreviewed until approved.
- **SC-005**: Running an agent-modified `cmd` entry from the TUI, CLI and shell widget always shows the unreviewed warning and requires the stronger confirmation until the user approves it.
- **SC-006**: Registering the server in Claude Code with the command from `kb mcp setup` works on the first try on macOS and Linux.

## Out of Scope

- Renaming and deleting entries through MCP.
- Running commands, opening URLs or executing runbooks through MCP; the agent receives text and links and acts with its own tools.
- Network transports (HTTP, SSE) and remote access; stdio only.
- Serving several KBs from one server process.
- Semantic or embedding-based search.
- MCP resources for every entry; tools and prompts cover the use cases without listing thousands of resources.

## Assumptions

- Users run MCP clients that ask for approval before running commands, so returning commands as text keeps the client's own safety prompts in charge of execution.
- Most agent value comes from reading; writing is valuable but less frequent, which is why it is opt-in.
- Recording review approvals as empty commits is acceptable history noise for a personal KB, in exchange for review state that syncs between machines.

## Constitution Check notes

- **III. Local-only**: stdio only; no listening socket, no network access, no account.
- **IV. Terminal only, one core**: the MCP server is a thin adapter over `kb-core`, like the CLI and TUI. It is not a GUI. Principle IV currently names only the CLI and TUI, so it needs a MINOR amendment adding "agent adapters over stdio (MCP)" as an allowed front end.
- **VI. Secrets**: secrets never reach the agent (User Story 2) and never enter the KB through it (secret gate with no override for agents).
- **VII. Keyboard-first and fast, no surprises**: agent changes are visible (badge, header count), attributable (git trailers, audit log), reversible (`kb review`), and gated by stronger confirmation before anything they touched is run.
- **IX. Simplicity**: four tools plus `list_tags`, prompts derived from existing entries, no rename or delete.
