# Implementation Plan: Actions and Code Links

**Branch**: `008-actions-code-links` | **Date**: 2026-10-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/008-actions-code-links/spec.md`

## Summary

Milestone 7 transforms kb from a copy-centric tool into an active operations hub: opening links and console deep links (Sumo Logic and CloudWatch Logs Insights), safely running saved commands with terminal suspension and secret isolation, piping queries into custom external tools via stdin/args, and providing first-class code links with local git clone previews, drift tracking, and re-pinning.

Ten technical decisions structure the design (detailed in [research.md](research.md)):

1. **Unified Action Model**: Every entry type defines a registry of available actions, including a designated primary open action. The TUI, CLI, and future picker query and execute actions via one core API.
2. **Console Deep Links with Length Protection**: Sumo and CloudWatch URLs encode resolved queries, time ranges, and log groups. URLs exceeding 8,000 characters trigger a fallback: the query is copied to the clipboard and the console opens with query omitted (FR-011).
3. **Safe Foreground Execution**: Command runs suspend ratatui (leaving alternate screen and raw mode), hand the terminal to the child process, forward signals, measure duration, and restore the terminal cleanly regardless of exit code or signal termination (FR-015, SC-007).
4. **Destructive Pattern Safeguards**: Commands matching destructive operations require explicit typed `run` confirmation interactively and `--yes --force` non-interactively (FR-019, FR-020, SC-002).
5. **Secret Variable Isolation**: `secret` variables in `cmd` payloads are passed exclusively via environment variables (`KB_SECRET_<NAME>`) and referenced as `"$KB_SECRET_<NAME>"` in command text, ensuring secret values never leak into process arguments, confirmation dialogues, or logs (FR-021, SC-003).
6. **Local Git Clone Discovery & Caching**: Clones are matched via explicit config mappings or 1-level scans of `code.search_paths`, normalized across SSH/HTTPS, and cached in `.kb/clones.json` (FR-022–FR-024).
7. **Read-Only Git CLI Integration**: Git operations (diff, show, grep, remote inspection) use the `git` command-line tool in read-only mode, with `git fetch` invoked only upon explicit request (FR-025, Constitution III).
8. **Diff-Mapped Drift Detection**: Drift between pinned commits and tracking branch tips is computed via `git diff -M -U0`, mapping line numbers through hunks and falling back to symbol search when necessary (FR-029–FR-031, SC-004).
9. **Format-Preserving Re-pinning**: Re-pinning updates only `ref`, `path`, `line`, `end_line`, and `updated` frontmatter keys byte-for-byte using M1's `set_field`, with optimistic concurrency protection (FR-032, SC-005).
10. **TUI Action Menu & Code View**: Adds an Action Menu overlay (`Alt-A`/`F2`), a dedicated Code View (Screen 3) for code entries, and configurable Enter key behavior (`tui.enter = "primary"`).

---

## Technical Context

**Language/Version**: Rust 2024, MSRV 1.88, `unsafe_code = forbid` (unchanged).

**Primary Dependencies**: `ratatui`, `crossterm`, `syntect`, `clap`, `regex`, `unicode-normalization`, `serde_json`, `toml_edit`. **No new dependencies.** Git operations use the installed `git` CLI; no C libraries (like libgit2) are added.

**Storage**:
- Entry files updated only during re-pinning (`ref`, `path`, `line`, `end_line`, `updated`).
- Clone discovery cache in `.kb/clones.json` (disposable).
- Usage statistics recorded in `.kb/state.json` on action execution.
- Search index updated incrementally on re-pinning.

**Testing**:
- `cargo test --workspace`.
- Unit tests in `kb-core` for action resolution, destructive pattern scanning, secret env mapping, diff hunk mapping, permalink formatting, and format-preserving re-pinning.
- Integration tests in `tests/` using test doubles (`FakePlatform`) and scripted fixture git repositories (covering lines moved, changed, renamed, deleted).
- PTY harness tests for command suspension/restoration (`tui_run_pty.rs`) and the Code View (`tui_code_pty.rs`).
- Hostile command test matrix verifying all 30 destructive commands require strong confirmation (SC-002).

**Target Platform**: macOS (arm64, x86_64) and Linux (x86_64, Wayland and X11).

**Project Type**: Cargo workspace (`crates/kb-core` library and `crates/kb` binary).

**Performance Goals**:
- Drift check on a single entry $\le$ 500 ms on a 100,000 commit repo without network (FR-033).
- `kb code check` across 200 entries in 10 repos $\le$ 10 seconds (SC-006).
- Search and action listing $\le$ 50 ms.
- CLI cold start $\le$ 100 ms.

**Constraints**:
- Strictly local-only: network access occurs only via explicit `git fetch` or when opening external URLs (Constitution III).
- Secrets never exposed in process args, confirmation views, or persistent files (Constitution VI).
- Terminal must be fully restored after running commands, even if killed by signals (Constitution V, SC-007).

**Scale/Scope**:
- Core modules: `kb_core::action`, `kb_core::run`, `kb_core::console`, `kb_core::code` (clone, drift, permalink, repin, snippet).
- Platform layer: `run_interactive_command` with terminal suspension and signal handling.
- CLI: 6 subcommands (`open`, `run`, `actions`, `code check`, `code repin`, `code show`).
- TUI: Action Menu overlay (`Alt-A`), Code View (`Screen 3`), Run Confirmation modal.

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Before Phase 0 | After Phase 1 | Notes |
|---|---|---|---|
| **I. Plain Files** | Pass | Pass | Re-pinning modifies only frontmatter keys `ref`, `path`, `line`, `end_line`, `updated` using format-preserving serializer. Comments and formatting are untouched byte-for-byte. |
| **II. Disposable Index** | Pass | Pass | Clone cache in `.kb/clones.json` is purely disposable; deleting it triggers automatic rediscovery. Usage records live in `.kb/state.json`. |
| **III. Local-Only** | Pass | Pass | No hosting APIs (GitHub/GitLab API) or auth tokens used. Git operations run locally via `git` CLI; network access occurs only on explicit `--fetch` / `F` or opening URLs. |
| **IV. Terminal Only, One Core** | Pass | Pass | All action logic, run planning, drift mapping, and console link builders reside in `kb-core`. CLI and TUI share the exact core API. All CLI commands support `--json`. |
| **V. Portable** | Pass | Pass | Terminal suspension, raw mode toggling, URL opening, and process spawning sit behind the `Platform` trait. Works on macOS and Linux (Wayland/X11); graceful SSH fallbacks. |
| **VI. Secrets Never Stored** | Pass | Pass | Secrets passed to commands exclusively via `KB_SECRET_<NAME>` env vars; masked in confirmation prompts and process listings. |
| **VII. Keyboard-First & Fast** | Pass | Pass | All actions accessible via single keys. Destructive commands require explicit confirmation. 500 ms drift check budget met. |
| **VIII. Test-First Core** | Pass | Pass | Comprehensive test suite planned: red-first core unit tests, scripted fixture git repositories, hostile command matrices, and PTY terminal tests. |
| **IX. Simplicity First** | Pass | Pass | No new heavy dependencies (no libgit2). Custom actions use simple command strings with stdin/arg piping instead of a plugin system. |

**Gate Result**: All principles pass with 0 violations.

---

## Project Structure

### Documentation (this feature)

```text
specs/008-actions-code-links/
├── spec.md              # Feature specification
├── plan.md              # This implementation plan
├── research.md          # Technical decisions R1–R10
├── data-model.md        # Entities, structs, and lifecycles
├── quickstart.md        # End-to-end verification guide
├── checklists/
│   └── requirements.md  # Specification quality checklist
└── contracts/
    ├── core-api.md      # Rust domain APIs for kb-core
    ├── cli.md           # CLI commands, arguments, exit codes, JSON schemas
    ├── tui-views.md     # Screen 3 (Code View), Action Menu, Run Modal
    └── config.md        # Config schema additions
```

### Source Code Architecture

```text
crates/kb-core/src/
├── action.rs            # Action model, ActionKind, registry & availability
├── run.rs               # RunPlan builder, secret env mapping, destructive checks
├── console.rs           # Sumo and CloudWatch deep link URL generators
├── code/
│   ├── mod.rs           # CodeService facade
│   ├── clone.rs         # Clone discovery, remote normalization, cache
│   ├── drift.rs         # Git diff hunk line mapping & symbol search fallback
│   ├── permalink.rs     # Web permalink generation (GitHub, GitLab, Bitbucket, custom)
│   ├── repin.rs         # Format-preserving re-pinning with concurrency check
│   └── snippet.rs       # Git show snippet extraction
├── config.rs            # Config schema additions (sumo, aws, code, run, actions)
└── lib.rs               # Module exports

crates/kb/src/
├── platform/
│   ├── mod.rs           # Platform trait additions: run_interactive_command
│   ├── unix.rs          # Terminal raw mode release, signal forwarding, duration timing
│   └── opener.rs        # Support for vscode:// and idea:// URL schemes
├── commands.rs          # CLI handlers: open, run, actions, code (check, repin, show)
├── tui/
│   ├── app.rs           # AppState additions for Action Menu, Code View, Run Modal
│   ├── keys.rs          # Keybindings: Ctrl-O, Ctrl-G, Alt-A, F2, Enter logic
│   ├── action_view.rs   # Alt-A Action Menu popup overlay
│   ├── code_view.rs     # Screen 3: Code view with syntax highlighting & drift badge
│   └── run_modal.rs     # Run confirmation prompt & destructive input handler
└── main.rs              # Clap CLI argument definitions

tests/
├── cli_open.rs          # Integration tests for kb open (link, sumo, cw, code)
├── cli_run.rs           # Integration tests for kb run, hostile matrix, secret envs
├── cli_actions.rs       # Integration tests for kb actions
├── cli_code.rs          # Integration tests for kb code check, repin, show
├── tui_code_pty.rs      # PTY tests for Screen 3 Code View
└── tui_run_pty.rs       # PTY tests for command execution & terminal restoration
```

---

## Complexity Tracking

No constitution violations or exceptions are required. All decisions adhere strictly to local-only execution, core isolation, and terminal-first principles.
