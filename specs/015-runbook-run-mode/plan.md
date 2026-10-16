# Implementation Plan: Runbook Run Mode

**Branch**: `015-runbook-run-mode` | **Date**: 2026-10-15 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/015-runbook-run-mode/spec.md`

## Summary

Implements Milestone R2: interactive TUI execution for runbooks. Allows users to start or resume local runbook runs, prompting for runbook variables once via the existing M5 form, and interactively step through the runbook. Steps are acted upon directly (commands run in the terminal with M7 safety checks and secret injection, consoles/links opened, code views shown, notes previewed, or payloads copied). Steps transition across well-defined statuses (`pending`, `opened`, `copied`, `running`, `done`, `skipped`, `failed`, `interrupted`) with automatic marking on exit 0 and confirmation prompts for `confirm: true` steps. Multiple runs of the same runbook can run concurrently, tracked independently in atomic, local-only, git-ignored JSON files under `.kb/runs/<run-id>.json` protected by advisory `.lock` files.

## Technical Context

**Language/Version**: Rust 1.88+ (Rust 2024 edition)

**Primary Dependencies**: `ratatui 0.30`, `crossterm 0.29`, `serde 1`, `serde_json 1`, `clap 4`, `portable-pty 0.9` (PTY testing), `tempfile 3`

**Storage**: Local JSON files in `.kb/runs/<run-id>.json` written atomically via tempfile + rename; sidecar `.kb/runs/<run-id>.lock` with PID and timestamp; git-ignored, never synced

**Testing**: `cargo test --workspace --locked`, unit tests in `kb-core`, PTY integration tests in `crates/kb/tests/tui_run_mode_pty.rs`, CLI tests in `crates/kb/tests/cli_runbook_run.rs`, secret scanning tests across `.kb/runs/`

**Target Platform**: Linux (x86_64) and macOS (x86_64, aarch64)

**Project Type**: Rust workspace (`crates/kb-core` domain library, `crates/kb` CLI and TUI binary)

**Performance Goals**:
- Run file load and atomic save: $\le 5$ ms
- TUI step selection and status transition: $\le 16$ ms (60 fps rendering)
- Listing runs (`kb runbook runs` across 100 runs): $\le 20$ ms

**Constraints**:
- **Constitution VI (Secrets)**: Secret values (`$ENV_VAR`, entered secrets) MUST NEVER be serialized into `.kb/runs/*.json`. On resume, secrets are re-read from environment variables or re-prompted.
- **Constitution III (Local-only)**: Run state is strictly machine-local. No networking or remote sync of run progress.
- **Constitution IV (One Core)**: All run state, state machine transitions, variable resolution, and persistence logic live in `kb-core::runbook::run`. CLI and TUI are thin drivers.
- **Constitution VII (No Surprises)**: M7 confirmations, destructive pattern warnings, unreviewed entry confirmations, and `confirm: true` checks cannot be bypassed.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check | Status |
|-----------|-------|--------|
| **I. Plain Files** | Entry files are unchanged by run execution; only `.kb/runs/` stores run progress | PASS |
| **II. Disposable State** | Deleting `.kb/runs/` loses only active run progress, never snippet definitions | PASS |
| **III. Local-Only** | Run progress is completely local to the machine, never pushed by `kb sync` | PASS |
| **IV. One Core** | `kb-core::runbook::run` contains the entire state machine and file persistence | PASS |
| **V. Portable** | Command spawning and terminal suspension use existing cross-platform abstractions | PASS |
| **VI. Secrets** | Secrets explicitly excluded from run JSON schemas and verified via test scanners | PASS |
| **VII. No Surprises** | M7 confirmation, destructive warning, and agent review safety checks preserved | PASS |
| **IX. Simplicity** | Single-foreground execution; no complex background daemons or multi-machine syncing | PASS |

## Project Structure

### Documentation (this feature)

```text
specs/015-runbook-run-mode/
├── plan.md              # Implementation plan
├── research.md          # Technical analysis, state transitions, locking & terminal handling
├── data-model.md        # Run, StepState, RunLock, and JSON file schemas
├── quickstart.md        # End-to-end validation scenarios
├── contracts/
│   ├── cli.md           # CLI commands (run, runs, abandon)
│   └── run-file.json    # JSON schema for .kb/runs/<run-id>.json
└── checklists/
    └── requirements.md  # Specification quality checklist
```

### Source Code Architecture

```text
crates/kb-core/src/
├── runbook/
│   ├── mod.rs           # Runbook definition, steps, types
│   ├── binding.rs       # Binding table resolution
│   ├── checklist.rs     # Checklist generation
│   ├── lint.rs          # Format & binding lint rules
│   └── run/             # [NEW] Run execution core
│       ├── mod.rs       # Run state machine, lifecycle (active/finished/abandoned)
│       ├── state.rs     # Run, StepState, StepStatus, RunLock data models
│       ├── fs.rs        # Atomic read/write, locking, pruning, recovery (.bak)
│       └── resolve.rs   # Multi-tier variable resolution (run vars -> bindings -> step vars)
├── keys/
│   ├── command.rs       # Add Run mode commands (step-action, mark-done, mark-skipped, etc.)
│   └── context.rs       # Add Context::Run
└── config.rs            # Config keys: runbook.auto_done, runbook.order_warnings

crates/kb/src/
├── commands/
│   └── runbook.rs       # CLI: kb runbook run, kb runbook runs, kb runbook abandon
├── tui/
│   ├── app.rs           # Run mode state machine integration, open/resume routing
│   ├── run_view.rs      # [NEW] Run screen TUI rendering (left steps pane, right resolved preview)
│   ├── run_picker.rs    # [NEW] Run selection modal (active runs list + "Start new run")
│   └── keys.rs          # Key event dispatch for Context::Run
tests/
├── tui_run_mode_pty.rs  # Full PTY tests for run mode lifecycle and step execution
├── cli_runbook_run.rs   # CLI tests for runbook run/runs/abandon commands
└── run_secrets_scan.rs  # Scans .kb/runs/ to guarantee zero secret leakage
```
