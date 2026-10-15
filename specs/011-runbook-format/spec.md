# Feature Specification: Runbook Format

**Feature Branch**: `011-runbook-format`
**Created**: 2026-10-11 (rewritten 2026-10-14 to fit the shipped MCP server, vim keymap and configurable keys)
**Status**: Draft
**Milestone**: R1 (first runbook milestone; run mode is R2 / spec 015, run records are R3 / spec 016)
**Depends on**: M1 (`001-entry-format`), M2 (`002-index-search`), M3 (`003-tui-core`), M5 (`006-variables`), M6 (`007-authoring`), M7 (`008-actions-code-links`), M9 (`009-git-sync`), M8 (`010-shell-integration`), MCP (`012-mcp-server`), keymap (`013-vim-keymap`). Works with notes (`014-markdown-notes`) if it lands first.
**Input**: Add a `runbook` entry type: an ordered list of steps that point to existing entries or carry manual instructions, with variables declared once for the whole runbook. Make runbooks valid, searchable, previewable, easy to create (by hand, from the TUI, from the CLI and by agents through MCP), and keep their references intact across renames, deletes and sync. Fill the runbook gaps the MCP server spec already anticipates. Running a runbook step by step is R2.

## Overview

During an incident or a deploy, the right sequence matters as much as the right query: check the error rate, deploy, watch latency, roll back if needed. Today each of those is a separate entry, and the sequence lives in the user's head or in a wiki page that drifts from the real queries.

A runbook is an entry that **references** other entries instead of copying them. When the Sumo query is improved, every runbook using it improves too. Variables such as `env` are declared once on the runbook and flow into every step that uses a variable of the same name, so R2 will ask `env=prod` once for a whole run.

Since this spec was first drafted, kb has shipped the MCP server (012), auto-commit and sync (009), the shell picker (010) and the vim keymap with configurable keys (013). This rewrite plugs runbooks into all of them:

- **MCP**: spec 012 already describes "used in" runbook lists, runbook steps returned by `get`, `steps` in `create`, and "hidden step" for private entries. This spec defines what those mean, so they can be implemented.
- **Review**: runbooks created or changed by agents get the `unreviewed` badge, and a runbook shows which of its steps use unreviewed entries.
- **Keys**: new actions are commands in the configurable keymap, with defaults for both the emacs and vim keymaps.
- **Sync**: renames update references in the same auto-commit, so the other machine never sees a broken runbook.
- **Safety**: runbook text (titles, manual steps, notes) can come from agents, so it goes through the same sanitizing as other free text before reaching the terminal.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Write a runbook that reuses my entries (Priority: P1)

Carlos writes "Deploy authorizer to prod" with four steps: three point to existing entries and one is a manual instruction. `kb lint` either passes or tells him exactly which step is wrong.

**Why this priority**: the format is the foundation for every later runbook milestone.

**Independent Test**: fixture runbooks covering every rule in FR-014, valid and invalid; run `kb lint` and `kb lint --json` and check each finding, its code and the exit code.

**Acceptance Scenarios**:

1. **Given** a runbook whose steps all reference existing, valid entries, **When** the user runs `kb lint`, **Then** it passes.
2. **Given** a step that references an entry that doesn't exist, **Then** lint reports an error on that step's line and suggests the new ID if the entry was renamed with unchanged content (M2 rename detection), otherwise the closest existing ID.
3. **Given** a runbook variable `env: enum [stage, prod]` and a step entry declaring `env: enum [prod, stage, dev]`, **Then** lint passes. **Given** the step's options lack `stage`, **Then** lint reports an error naming the step, the variable and the missing option.
4. **Given** a step that is itself a runbook, **Then** lint reports an error: runbooks can't contain runbooks in this version.

Example:

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
  - id: watch
    title: Watch p99 latency for 10 minutes
    entry: cw/authorizer-p99-latency
  - id: rollback
    title: Roll back if p99 > 800 ms
    do: |
      Only if p99 stayed above 800 ms for 5 minutes.
      Post in #auth-oncall before rolling back.
    optional: true
---

Owner: platform security. Last rehearsed in stage on 2026-10-02.
```

---

### User Story 2 — Find and read runbooks (Priority: P1)

Carlos searches `deploy auth` and sees the runbook. The preview shows its variables and numbered steps, each with the type badge and title of the entry it uses, and marks optional, confirm and manual steps.

**Why this priority**: a runbook that can't be found or read at a glance won't be used under pressure.

**Acceptance Scenarios**:

1. **Given** a search, **Then** runbooks match on title, tags, notes, step titles and manual step text, and appear with their own type badge and chip.
2. **Given** a runbook is selected, **Then** the preview shows: title and metadata; the runbook variables; the numbered steps, each with the referenced entry's type badge and title (or a `manual` badge), `optional` and `confirm` markers, and the step's bindings when they differ from the defaults; and the notes.
3. **Given** a step whose entry is missing, has lint errors, or is unreviewed (spec 012), **Then** that step is marked accordingly (`missing`, `⚠ errors`, `agent`), and the runbook's header line counts them.
4. **Given** an ordinary entry used by runbooks, **Then** its preview shows "Used in" with the runbook titles.
5. **Given** `runbook:deploy/authorizer`, **Then** only the entries that runbook uses are listed, in step order. **Given** `used-by:runbook`, **Then** only entries used by at least one runbook are listed. **Given** `type:runbook`, **Then** only runbooks are listed.

---

### User Story 3 — Renames, deletes and sync never break runbooks (Priority: P1)

Carlos renames `sumo/5xx-by-authorizer` to `sumo/authorizer/5xx`. The three runbooks that use it are updated in the same operation and the same commit, and his other machine receives a consistent KB on the next sync.

**Why this priority**: references are only better than copies if they never silently break.

**Independent Test**: fixture KB with runbooks sharing entries, in a git repo with a bare remote and a second clone; rename and delete entries through `kb mv`, `kb rm`, the TUI keys and MCP; check every runbook file byte for byte except the updated references, the resulting commits, and the second clone after `kb sync`.

**Acceptance Scenarios**:

1. **Given** `kb mv <old> <new>` (M6) or the TUI rename on an entry used by runbooks, **Then** kb updates every `entry:` reference to it using format-preserving edits (M1 FR-017), lists the runbooks it updated, and with auto-commit on (M9) puts the rename and the reference updates in **one** commit whose message body lists the updated runbooks.
2. **Given** `kb rm <id>` or the TUI delete on an entry used by runbooks, **Then** kb refuses, lists the runbooks and steps that use it, and exits 1. With `--force`, kb deletes it and the steps become broken references that lint reports.
3. **Given** the user renamed an entry by hand (outside kb), **Then** `kb lint --fix` offers to update runbook references to the detected new ID, one at a time or all at once with `--yes`, and auto-commits the fix.
4. **Given** a rename made on another machine arrives through `kb sync` (M9), **Then** the reference updates arrive in the same commit, no runbook is ever broken in between, and usage carried across the rename (M9 FR-021) is unaffected.
5. **Given** a sync conflict on a runbook file, **Then** it is resolved with the normal per-entry conflict flow (M9), and the merged result is linted like any save.

---

### User Story 4 — Create a runbook quickly (Priority: P2)

Carlos already has the four entries he needs and wants a runbook out of them without hand-writing YAML.

**Acceptance Scenarios**:

1. **Given** `kb runbook new "Deploy authorizer" sumo/5xx-by-authorizer cmd/deploy-authorizer cw/authorizer-p99-latency`, **Then** kb creates a runbook with one step per entry in that order, step titles and ids taken from the entries, and opens it in the editor for review (M6 editor flow) unless `--yes` is passed.
2. **Given** the entries share variables of the same name with compatible kinds, **Then** kb lifts them to the runbook (FR-027).
3. **Given** `kb add --type runbook --editor` (M6), **Then** the template shows every step field with a short comment.
4. **Given** the TUI list, **When** the user marks entries with the `mark-entry` command and then runs `new-runbook-from-marks` (FR-031), **Then** the scenario 1 flow starts with the marked entries in the order they were marked.
5. **Given** shell completion (M8), **When** the user types `kb runbook new "x" sumo/5<Tab>`, **Then** entry IDs are completed.

---

### User Story 5 — Agents read and write runbooks (Priority: P2)

In Claude Code, Carlos asks "walk me through the authorizer deploy runbook for prod". The agent calls `get` with `resolve: true` and `vars: {env: prod, version: v2.14.0}` and receives every step with its resolved query or command. Later he asks the agent to "add a step to check the error rate again after the deploy"; the agent calls `update` with new `steps`, and the runbook gets the "agent" badge until he reviews it.

**Why this priority**: spec 012 already advertises runbook support; agents are a natural way to read and maintain procedures.

**Acceptance Scenarios**:

1. **Given** MCP `get` on a runbook, **Then** the response includes its variables, its steps (id, title, entry or manual text, flags, bindings) and the binding table (FR-011).
2. **Given** `get` with `resolve: true` and `vars`, **Then** each step that references an entry also includes that entry's resolved payload, fields and action links (spec 012 FR-007), using the runbook variables plus step bindings; secrets follow spec 012 FR-012.
3. **Given** a step whose entry is hidden from agents (`mcp.exclude_tags`, `mcp.exclude_types`), **Then** the step is returned as `hidden step` with its position only, and its entry ID and title are not revealed (spec 012 FR-016).
4. **Given** MCP `create` with `type: runbook` and `steps`, or `update` with `changes.steps`, **Then** kb validates the runbook with this spec's lint rules and the spec 012 write rules (no "save anyway" for agents), saves it, commits it with the `Kb-Source: mcp` trailer, and marks it unreviewed.
5. **Given** `get` or `update` on any entry used by runbooks, **Then** the response lists those runbooks ("used in"), so the agent can tell the user what a change affects.

---

### User Story 6 — Share a runbook as a checklist (Priority: P2)

Before R2 exists, and afterwards for planning, Carlos wants to paste a runbook into a deploy ticket.

**Acceptance Scenarios**:

1. **Given** a runbook picked in the TUI or copied with `kb copy`, **Then** its pick output is a Markdown checklist: the title as a heading, then one `- [ ]` line per step with its number, title (with runbook variables shown as `{{name}}`) and the referenced entry ID in backticks; optional steps marked "(optional)"; manual instructions indented below their step.
2. **Given** `kb use <runbook>` (M5) or `kb show <runbook> --checklist --resolve`, **Then** kb asks for the runbook variables once and prints the checklist with titles resolved and each referenced entry's resolved payload as a fenced block under its step.
3. **Given** a secret variable in a resolved checklist, **Then** it appears as an environment-variable reference (as in spec 010 FR-014) or as its unresolved `{{name}}` placeholder, never as its value, because checklists are meant to be pasted into tickets.

---

### Edge Cases

- **The same entry used twice** in one runbook (an error-rate check before and after a deploy): allowed; each use is a separate step with its own id.
- **Runbook variables used only in step text** (`title`, `do`, `note`): valid; they count as used.
- **Kind compatibility**: runbook `number` → step `text` is valid; runbook `text` → step `number` is an error unless the step binds a literal (FR-009).
- **Secret variables** on a runbook: allowed; they flow into steps like any variable, and spec 012 and M5/M7 secret rules apply where steps are used.
- **Long runbooks**: up to 100 steps; lint warns above 50.
- **Step ids**: derived from titles when absent; lint suggests writing them explicitly, because R2 uses them to resume a run after the runbook is edited.
- **Zero steps**: error.
- **Reordered steps** (by hand or by sync): valid; ids keep their meaning.
- **Notes as steps** (when spec 014 has landed): a step may reference a `note`; it is shown as reading material, with no variables to bind.
- **Escape sequences in runbook text**: step titles, `do` text, notes and runbook titles are sanitized before drawing, exactly as note text is in spec 014 (FR-021, FR-022). If spec 014 has not landed, this spec introduces the same sanitizer for runbook text.
- **Code steps**: a step that references a `code` entry shows a drift marker only if a drift result is already cached (M7); the preview never runs git.
- **Agent edits to entries a runbook uses**: the runbook itself isn't modified, but its preview marks the affected steps as `agent` until reviewed (User Story 2, scenario 3).
- **Shell widget (M8)**: runbooks are excluded from the widget's default `cmd` filter; with the filter cleared, picking a runbook inserts nothing and shows a hint to use the TUI or `kb copy` instead.

## Requirements *(mandatory)*

### Format

- **FR-001**: `runbook` MUST be added to the entry types as a minor addition to `kb_format: 1`. Runbooks have no payload; the body is notes (as for `link` and `code`, M1 FR-007).
- **FR-002**: A runbook MUST have a `steps` list in its frontmatter with 1 to 100 steps.
- **FR-003**: Each step MUST have `title` (non-empty, at most 120 characters) and exactly one of `entry` (an entry ID) or `do` (manual instruction text, may be multi-line).
- **FR-004**: Each step MAY have:
  - `id`: `[a-z0-9][a-z0-9-]*`, unique within the runbook; when absent, derived from the title as a slug with `-2`, `-3` appended for duplicates;
  - `note`: extra guidance shown with the step (may be multi-line);
  - `optional`: boolean, default `false`;
  - `confirm`: boolean, default `false` (in R2, the step needs explicit confirmation before it is marked done);
  - `vars`: variable bindings for this step (FR-008).
- **FR-005**: Runbook-level `vars` MUST use the standard declaration format (M1 FR-008, plus `escape` from M5 FR-007).
- **FR-006**: Variable references (`{{name}}`) MUST be recognized in step `title`, `do` and `note`, and refer to runbook variables.
- **FR-007**: When spec 014 is present, `do` and `note` text MUST be rendered as Markdown with its renderer; otherwise as plain wrapped text.

### Variable binding

- **FR-008**: A step entry's variable MUST be bound by default to the runbook variable of the same name, if one exists. A step's `vars` map MAY bind any of its entry's variables to a literal (`minutes: 10`) or to another runbook variable (`minutes: "{{watch_minutes}}"`), overriding the default.
- **FR-009**: A binding is valid only if every value the source can take is valid for the step variable:
  - `enum` → `enum`: runbook options are a subset of the step options;
  - `enum` → `text`, `number` → `number`, `number` → `text`: valid;
  - `text` → `number` or `enum`: invalid;
  - `secret` → `secret`: valid; `secret` to or from any other kind: invalid.
  Literals MUST be valid for the step variable's kind.
- **FR-010**: Unbound step variables remain unbound; in R2 they are asked for when that step is used. Lint reports them as info.
- **FR-011**: The core MUST expose, for a runbook, the full **binding table**: for each step, each of its entry's variables and its target (runbook variable, literal, or unbound), with a validity result. The TUI preview, MCP `get` and R2 MUST all use this one table.

### Validation

- **FR-012**: Lint rule codes for runbooks MUST continue kb's numbering, in a new block starting at `KB070` (for example `KB070-runbook-no-steps`, `KB071-step-entry-and-do`, `KB072-step-missing-entry`, `KB073-step-is-runbook`, `KB074-duplicate-step-id`, `KB075-invalid-step-id`, `KB076-invalid-binding`, `KB077-binding-unknown-var`, `KB078-binding-not-in-entry`, `KB079-too-many-steps`, `KB080-long-runbook`, `KB081-step-entry-has-errors`, `KB082-unused-runbook-var`, `KB083-unbound-step-var`, `KB084-implicit-step-id`). Codes are final once released.
- **FR-013**: Broken-reference errors MUST include a suggested fix when one exists: the ID the entry was renamed to (M2 content-hash rename detection), else the closest existing ID by edit distance.
- **FR-014**: Severities:
  - errors: no steps; more than 100 steps; both or neither of `entry` and `do`; missing entry; entry is a runbook; duplicate or invalid step id; invalid binding; binding to an unknown runbook variable; binding for a variable the entry doesn't have;
  - warnings: more than 50 steps; step entry with lint errors; unused runbook variable;
  - info: unbound step variable; step without an explicit `id`.
- **FR-015**: `kb lint --fix` MUST apply suggested reference fixes after confirmation (or with `--yes`), using format-preserving edits and auto-commit (M9).

### Reference integrity

- **FR-016**: The index MUST store every runbook → entry reference (runbook ID, step id, position, entry ID) in a new table, and bump `INDEX_VERSION` so existing indexes rebuild (constitution II).
- **FR-017**: `kb mv` (M6 FR-022), the TUI rename and any other rename path MUST update every reference to the renamed entry in the same operation, with format-preserving edits, and MUST include the updated runbooks in the same commit when auto-commit is on (M9 FR-005), listing them in the commit message body.
- **FR-018**: `kb rm` (M6 FR-023) and the TUI delete MUST refuse to delete an entry referenced by runbooks unless `--force` (or an explicit confirmation in the TUI that names the runbooks), and MUST list the referencing runbooks and steps either way.
- **FR-019**: MCP has no rename or delete (spec 012 FR-024), so no MCP path can break references.

### Search and preview

- **FR-020**: Runbooks MUST be indexed on title, tags, notes, step titles, `do` text and step notes. Referenced entries' content MUST NOT be indexed as part of the runbook.
- **FR-021**: New query filters: `runbook:<id>` (entries used by that runbook, in step order) and `used-by:runbook` (entries used by any runbook). Both MUST also be accepted by `kb search` flags and MCP `search`.
- **FR-022**: `runbook` MUST have its own type color, badge and chip (M3 FR-008).
- **FR-023**: The preview MUST show User Story 2, scenario 2 and 3. Every entry's preview MUST show "Used in" when runbooks reference it. All runbook text MUST be sanitized before drawing (Edge Cases).
- **FR-024**: Lookups for "used in" and the new filters MUST use the index only and meet M2's 50 ms budget.

### MCP (fills spec 012's runbook references)

- **FR-025**: MCP `get` on a runbook MUST return variables, steps and the binding table; with `resolve: true`, each entry step MUST include its resolved content and action links computed with the runbook variables and step bindings (User Story 5). Hidden entries MUST be returned as `hidden step` with position only.
- **FR-026**: MCP `create` and `update` MUST accept runbooks: `steps` in `create`, and `changes.steps` (full replacement) in `update`, validated with FR-012 to FR-014. Any lint error blocks the write (spec 012 FR-022). Writes are committed with the `Kb-Source: mcp` trailer and marked unreviewed (spec 012 FR-027, FR-028). The "used in" lists in `get` and `update` responses (spec 012 FR-007, FR-019) MUST come from FR-016.

### Creating runbooks

- **FR-027**: `kb runbook new <title> <entry-id…> [--path] [--tag]… [--yes] [--json]` MUST create a runbook as in User Story 4 through the M6 save path. Variables MUST be lifted to the runbook only when they appear in two or more steps with compatible kinds; enum options become the intersection of the steps' options, and the default is set only if all steps share it.
- **FR-028**: The M6 editor template for `runbook` MUST include every step field with a short comment.
- **FR-029**: The default path for runbooks MUST be `runbook/<slug-of-title>` (M6 FR-008; `path_template` applies).
- **FR-030**: Dynamic completion (M8 FR-017) MUST complete entry IDs for `kb runbook new` and runbook IDs for the `runbook:` filter.

### Keys (configurable, spec 013)

- **FR-031**: Two commands MUST be added to the configurable keymap, available in the list contexts (`search` and `normal`):
  - `mark-entry`: toggle a mark on the selected entry, shown with a marker and its order number; defaults `Alt-M` (emacs, both modes) and `v` (vim normal mode);
  - `new-runbook-from-marks`: start the User Story 4 flow with the marked entries; defaults `Alt-B` and `B` (vim normal mode).
  Both MUST appear in `kb keys`, `kb keys defaults`, the help overlay and the key bar using the effective bindings (spec 013 FR-027, FR-028), and MUST pass the startup validation (spec 013 FR-023).
- **FR-032**: Marks MUST be cleared after a runbook is created, by `Esc` in normal mode when the query is already empty (before quitting), and when the TUI exits.

### Output

- **FR-033**: A runbook's pick output (M3 FR-017, `payload` mode) MUST be the Markdown checklist of User Story 6. `--print json` MUST include the steps, the binding table and resolved references.
- **FR-034**: `kb show <runbook>` MUST print the preview contents; `--checklist` the checklist; `--resolve` adds resolved payloads, asking for runbook variables once (M5). `kb use <runbook>` MUST behave like `kb show --checklist --resolve` and copy the result (M4), recording a use of the runbook. Secrets follow User Story 6, scenario 3.

### Key Entities

- **Runbook**: an entry of type `runbook` with variables, steps and notes.
- **Step**: id, title, either an entry reference or manual text, note, optional and confirm flags, bindings.
- **Binding**: step id, step variable name, target (runbook variable, literal, or unbound), validity.
- **Reference**: runbook ID, step id, position, referenced entry ID; stored in the index.
- **Mark**: a TUI-session list of entry IDs in marking order.

## Success Criteria *(mandatory)*

- **SC-001**: A runbook from four existing entries can be created with `kb runbook new` (or four marks and one key) and saved in under a minute.
- **SC-002**: In a fixture KB with 50 runbooks and 500 entries in a git repo with a second clone, renaming any entry through kb leaves zero broken references, changes only reference values in runbook files (byte for byte otherwise), produces exactly one commit, and the second clone is consistent after one `kb sync`.
- **SC-003**: Every lint rule in FR-012 has at least one failing and one passing fixture, and every broken reference caused by a hand rename gets the right suggested fix.
- **SC-004**: "Used in" lookups and the `runbook:` and `used-by:` filters return within M2's 50 ms budget on 10,000 entries.
- **SC-005**: Through MCP, an agent can read, resolve, create and update runbooks in the fixture KB; no response reveals a hidden entry; every agent write is one commit with the `Kb-Source: mcp` trailer and shows as unreviewed.
- **SC-006**: A resolved checklist built from runbooks with secret variables never contains a secret value.
- **SC-007**: With a remapped `mark-entry` and `new-runbook-from-marks` in config, both keymaps create runbooks with the user's keys, and the help and key bar show them.

## Out of Scope

- Running a runbook step by step, progress, resume and confirmation gates (R2, spec 015).
- Run records, timelines and CLI stepping (R3, spec 016).
- Automated checks, polling and conditions (R4).
- Runbooks inside runbooks; branching or conditional steps (conditions are written as manual steps with `optional: true` on the action).
- Inline commands or queries inside a step; a step references an entry or is a manual instruction.
- Renaming or deleting entries through MCP (spec 012 FR-024).

## Assumptions

- Referencing entries instead of copying them is what users want: improving a query should improve every runbook that uses it.
- Most runbooks have 3 to 15 steps; the 50-step warning and 100-step limit don't get in the way.
- Agents will often draft runbooks; the existing review flow (spec 012) is the right safeguard, since runbooks lead to commands being run in R2.

## Constitution Check notes

- **I. Plain files**: a runbook is one Markdown file with YAML frontmatter, hand-editable like any entry.
- **II. Disposable index**: references and "used in" data are derived; the new table comes with an `INDEX_VERSION` bump.
- **IV. Thin front ends**: the binding table, references and checklist are core APIs used by the TUI, CLI and MCP alike.
- **VI. Secrets**: secret variables flow through runbooks under the existing rules; checklists never contain secret values.
- **VII. No surprises**: runbook text is sanitized before drawing; agent-written runbooks are flagged until reviewed; confirmation answers stay fixed (spec 013 FR-024).
- **IX. Simplicity**: no nesting, no branching and no inline steps in this version.
- **Technical constraints**: the list of entry types gains `runbook` (PATCH amendment, together with `note` from spec 014 if both land).
