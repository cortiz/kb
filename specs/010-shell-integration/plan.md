# Implementation Plan: Shell Integration

**Branch**: `010-shell-integration` | **Date**: 2026-10-10 | **Spec**: [specs/010-shell-integration/spec.md](spec.md)

**Input**: Feature specification from `/specs/010-shell-integration/spec.md`

## Summary

Milestone 8 integrates `kb` into the developer's shell prompt across `zsh`, `bash`, and `fish`. It provides:
1. An inline compact picker rendering directly in the normal terminal buffer below the prompt without alternate screen flashing.
2. Shell widgets (`Alt-K`) that place resolved commands directly onto the command line without automatic execution.
3. Secret-safe variable resolution substituting environment variable references (`"$VAR"`) instead of plaintext secrets.
4. Fast dynamic tab completions (`kb __complete`) under 50 ms.
5. Quick capture of the last run command (`kb-save-last` via `Alt-Shift-K`).
6. Shell configuration management (`kb shell install`/`uninstall`) using idempotent marked blocks.

## Technical Context

**Language/Version**: Rust 1.88 (2024 edition)  
**Primary Dependencies**: `ratatui` (0.30), `crossterm` (0.29 with `use-dev-tty`), `clap` (4.x with `derive`), `rusqlite` (bundled), `syntect` (5.x), `portable-pty` (0.9, for test fixtures).  
**Storage**: Plain UTF-8 Markdown entry files in KB root; SQLite + FTS5 index in `.kb/index.db`.  
**Testing**: `cargo test --workspace` with automated pseudo-terminal integration tests in `crates/kb/tests/shell_integration_pty.rs`.  
**Target Platform**: macOS (arm64, x86_64) and Linux (x86_64, aarch64), zsh 5.0+, bash 4.0+ (with bash 3.2 fallback), fish 3.0+.  
**Project Type**: Terminal knowledge base CLI and TUI binary (`crates/kb`) over shared library core (`crates/kb-core`).  
**Performance Goals**:
- Inline picker first paint $\le 80$ ms on 10,000 entries.
- Keystroke-to-results $\le 50$ ms.
- Dynamic completion response $\le 50$ ms without index rebuild.  
**Constraints**:
- Normal screen buffer rendering (clears lines on exit, restores prompt and cursor byte-for-byte).
- Zero secrets in shell history files (references only).
- Insert only, never execute commands.  
**Scale/Scope**: 10,000+ entries, zsh/bash/fish support, emacs and vi editing modes, inside/outside tmux 3.2+.

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evaluation |
|-----------|--------|------------|
| **I. Plain Files** | PASS (Justified Exception) | Entries remain plain files. `kb shell install` writes only to the user's shell startup file within marked blocks (`# >>> kb >>>`) with explicit confirmation, reversible via `kb shell uninstall`. |
| **II. Disposable Index** | PASS | Tab completions query `.kb/index.db` in read-only mode and never trigger index mutations or full rebuilds. |
| **III. Local-Only** | PASS | Zero network access. Runs completely on the local workstation. |
| **IV. Terminal Only, One Core** | PASS | 100% terminal. Domain logic and shell scripts generation reside in `kb-core`; CLI and TUI rendering in `kb`. |
| **V. Portable by Construction** | PASS | Supports macOS and Linux; tested across zsh, bash (modern and legacy 3.2), and fish, including inside tmux and SSH. |
| **VI. Secrets Never Stored** | PASS | Secrets on the command line are inserted exclusively as references to environment variables (`"$VAR"`), preventing storage in shell history. Secrets lacking an `env` source block insertion. |
| **VII. Keyboard-First and Fast** | PASS | Full keyboard navigation; latency budgets $\le 80$ ms for picker, $\le 50$ ms for completions; confirmation required before any command execution. |
| **VIII. Test-First Core** | PASS | Red-green TDD strictly followed; automated pseudo-terminal tests verify prompt line restoration and terminal output. |
| **IX. Simplicity First** | PASS | No daemon or background server; clean shell function wrappers communicating via structured JSON on stdout and rendering on `/dev/tty`. |

---

## Project Structure

### Documentation (this feature)

```text
specs/010-shell-integration/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── cli.md
│   ├── config.md
│   ├── core-api.md
│   └── inline-picker.md
├── checklists/
│   └── requirements.md
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code Layout

```text
crates/kb-core/src/
├── shell/               # Shell dialects, completions, script generators
│   ├── mod.rs
│   ├── dialects.rs      # Zsh, Bash, Fish syntax and escaping
│   ├── completions.rs   # Fast index-backed tab completion queries
│   └── templates.rs     # Script generator for eval "$(kb shell init)"
├── varres/              # Updated with ShellWidget resolution target
│   └── mod.rs
└── config.rs            # Added widget.* settings

crates/kb/src/
├── commands/
│   ├── pick.rs          # kb pick CLI subcommand
│   ├── shell.rs         # kb shell init / install / uninstall
│   └── complete.rs      # kb __complete hidden command
├── tui/
│   ├── inline_picker.rs # Normal screen buffer compact picker
│   └── inline_form.rs   # Inline variable prompt form
└── main.rs              # CLI routing and doctor additions

crates/kb/tests/
└── shell_integration_pty.rs # Pseudo-terminal integration tests
```

---

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Writing outside KB root in `kb shell install` | Convenient one-line setup for shell integration | Requiring manual multi-step shell profile editing causes user friction and broken installations. Marked block ensures clean removal. |
