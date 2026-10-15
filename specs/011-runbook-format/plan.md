# Implementation Plan: Runbook Format

**Branch**: `011-runbook-format` | **Date**: 2026-10-15 | **Spec**: [specs/011-runbook-format/spec.md](spec.md)

**Input**: Feature specification from `specs/011-runbook-format/spec.md`

---

## Summary

Milestone Runbook Format (`011-runbook-format`) introduces a first-class `runbook` entry type:
1. **Format**: Adds `EntryType::Runbook` in `kb_format: 1`. Runbooks have no payload block; their body holds Markdown notes. Frontmatter contains 1–100 steps (references to existing entries or manual `do` instructions).
2. **Variable Binding**: Computes an authoritative binding table in `kb-core`. Step entry variables are bound by default to runbook variables of the same name, or explicitly via literal/reference in step `vars`. Validates kind compatibility (`enum` subset, `number` -> `text`, `secret` -> `secret`).
3. **Validation & Linting**: Implements diagnostic rules `KB070` to `KB084` with rename detection & fuzzy match suggestions for broken references. `kb lint --fix` applies fixes using format-preserving edits.
4. **Reference Integrity**: Bumps `INDEX_VERSION` to 4, adding the `runbook_steps` relational table. Renames via `kb mv` or TUI update referencing runbooks in a single commit. Deletes via `kb rm` or TUI refuse if referenced unless `--force`.
5. **Search & Preview**: Runbook text is indexed (titles, tags, notes, step titles, do text, step notes); query filters `runbook:<id>` and `used-by:runbook` return within 50 ms. Preview renders variables, numbered steps with entry badges/titles, confirm/optional flags, and non-default bindings. Entries show "Used in".
6. **MCP Server Integration**: MCP `get` returns steps and binding table; `resolve: true` resolves payloads and action links; masks hidden entries as `hidden step`. `create` and `update` write runbooks with `Kb-Source: mcp` and `unreviewed` badge.
7. **Creation & Authoring**: `kb runbook new` lifts compatible variables across 2+ steps and builds runbooks; TUI provides `mark-entry` and `new-runbook-from-marks` configurable keybindings.
8. **Output**: `kb show --checklist` and `kb use` format runbooks as Markdown checklists; secret variables are never exposed in checklists.

---

## Technical Context

**Language/Version**: Rust 1.88 (2024 edition)  
**Primary Dependencies**: `rusqlite` (0.40 bundled), `serde` (1.0), `serde_json` (1.0), `serde_yaml` (0.9), `pulldown-cmark` (0.13), `ratatui` (0.30), `crossterm` (0.29), `clap` (4.x). Zero new external crate dependencies.  
**Storage**: UTF-8 plain Markdown files (`~/kb/runbook/<slug>.md`) and SQLite disposable index (`.kb/index.db`).  
**Testing**: `cargo test --workspace` with automated unit tests, lint fixtures, rename integrity git sync tests, and performance benchmarks.  
**Target Platform**: macOS (arm64, x86_64) and Linux (x86_64, aarch64).  
**Project Type**: CLI and TUI binary (`crates/kb`) over shared core library (`crates/kb-core`).  
**Performance Goals**:
- "Used in" lookups and `runbook:` / `used-by:` filters $\le 50$ ms on 10,000 entries (SC-004).
- Rename and reference integrity updates $\le 500$ ms for 50 runbooks (SC-002).

---

## Constitution Check

| Principle | Status | Evaluation |
|---|---|---|
| **I. Plain Files** | PASS | Runbooks are plain Markdown files with YAML frontmatter under `~/kb/runbook/<slug>.md`. Valid and editable without kb. |
| **II. Disposable Index** | PASS | Runbook references live in disposable index table `runbook_steps`. Fully reconstructible via `kb reindex`. `INDEX_VERSION` bumped to 4. |
| **III. Local-Only** | PASS | Parsing, binding, linting, and refactoring execute entirely locally. |
| **IV. Terminal Only, One Core** | PASS | Binding table, reference tracking, checklist generation, and linting reside in `kb-core`. Shared by CLI, TUI, and MCP. |
| **V. Portable by Construction** | PASS | Cross-platform text measurement and standard POSIX/macOS path handling. |
| **VI. Secrets Never Stored** | PASS | Secret variables follow existing M5/M7 rules. Checklists never expose secret values (use `$VAR` or `{{name}}`). |
| **VII. Keyboard-First and Fast** | PASS | Keymap commands `mark-entry` and `new-runbook-from-marks` with emacs and vim defaults. 50 ms query budget preserved. |
| **VIII. Test-First Core** | PASS | Comprehensive test suite for all KB070–KB084 lint rules, variable binding combinations, and rename sync integrity. |
| **IX. Simplicity First** | PASS | No nested runbooks, no branching, no inline commands in R1. |

---

## Project Structure

### Documentation
```text
specs/011-runbook-format/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── runbook-format.md
│   ├── cli.md
│   └── mcp.md
└── checklists/
    └── requirements.md
```

### Source Code Layout
```text
crates/kb-core/src/
├── entry.rs             # Add EntryType::Runbook to enum and maps
├── runbook/             # NEW: Runbook data models, step parser, binding table evaluator
│   ├── mod.rs
│   ├── binding.rs       # Binding table logic and kind compatibility
│   └── checklist.rs     # Checklist generator (plain & resolved)
├── lint/
│   └── rules.rs         # Lint rules KB070-KB084, rename suggestions
├── index/
│   ├── schema.rs        # Bump INDEX_VERSION to 4, add runbook_steps table
│   ├── doc.rs           # Runbook column mapping for FTS
│   └── sync.rs          # Populate runbook_steps during reindex
├── author/
│   ├── fsops.rs         # Format-preserving rename & reference updater
│   └── runbook_new.rs   # kb runbook new variable lifting logic
crates/kb/src/
├── cli/
│   ├── runbook.rs       # kb runbook new command handler
│   ├── show.rs          # --checklist and --resolve support
│   └── use_cmd.rs       # kb use on runbooks
├── tui/
│   ├── mark.rs          # MarkState for marked entries
│   ├── preview.rs       # Runbook preview & "Used in" rendering
│   └── keys.rs          # mark-entry & new-runbook-from-marks commands
└── mcp/
    └── server.rs        # MCP get/create/update runbook support & hidden masking
```
