# Implementation Plan: Vim Keymap and Configurable Key Bindings

**Branch**: `013-vim-keymap` | **Date**: 2026-10-14 | **Spec**: [specs/013-vim-keymap/spec.md](spec.md)

**Input**: Feature specification from `/specs/013-vim-keymap/spec.md`

---

## Summary

Milestone UX (`013-vim-keymap`) introduces a dual-mode `vim` keymap preset alongside the default `emacs` preset, with full user configurability across all TUI contexts:
1. **Keymap Presets**: `emacs` (default, type-to-search always focused, modifier chords) and `vim` (Search mode and Normal mode).
2. **Normal Mode**: Single-letter actions (`o` open, `e` edit, `y` copy, `!` run, `m` action menu, `n` add, `R` rename, `D` delete, `s` sync, `H` history) and vim navigation (`h j k l`, `gg`, `G`, `Ctrl-D`, `Ctrl-U`, `J`, `K`), while preserving arrow-key navigation in both modes.
3. **Mode Switching**: Esc enters normal mode ($\le 50$ ms delay); `/` clears and enters search mode; `i` or `a` keeps query and enters search mode; Esc in normal mode clears query or exits if query is empty.
4. **Configurable Key Overrides**: `[tui.keys.<context>]` configuration tables in TOML allow rebinding or unbinding (`[]`) any command.
5. **Reserved Key Invariants**: `Ctrl-C` always quits; execution and deletion confirmation prompts remain fixed; each list mode guarantees at least one `help` and one `quit` key.
6. **CLI & Diagnostics**: `kb keys [--json]`, `kb keys check` (exits 1 on warnings, 0 if clean), `kb keys defaults [--keymap vim]`, and `kb doctor` integration.

---

## Technical Context

**Language/Version**: Rust 1.88 (2024 edition)  
**Primary Dependencies**: `ratatui` (0.30), `crossterm` (0.29), `serde` (1.0), `serde_json` (1.0), `toml_edit` (0.22), `clap` (4.x with derive).  
**Storage**: TOML user configuration (`~/.config/kb/config.toml`).  
**Testing**: `cargo test --workspace` with automated unit, integration, and PTY tests (`portable-pty`).  
**Target Platform**: macOS (arm64, x86_64) and Linux (x86_64, aarch64); Terminal.app, iTerm2, kitty, Alacritty, tmux.  
**Project Type**: Terminal knowledge base CLI and TUI binary (`crates/kb`) over shared core library (`crates/kb-core`).  
**Performance Goals**:
- Mode switch from Search to Normal within $\le 50$ ms (SC-004).
- Keystroke to screen update $\le 16$ ms (60 fps interactive response budget).
- First paint and CLI start unchanged ($\le 100$ ms).  
**Constraints**:
- Single shared core: Key syntax parsing, command definitions, validation, and effective keymap resolution live in `kb-core` (Constitution Principle IV).
- Safety invariant: Rebinding cannot bypass run/delete confirmations or unbind Ctrl-C (Constitution Principle VI / VII).
- Backwards compatibility: Zero regressions or visual changes for users who do not opt in (Constitution Principle IX).  
**Scale/Scope**: 2 presets (`emacs`, `vim`), 10 key contexts, ~40 commands, arbitrary user overrides.

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evaluation |
|---|---|---|
| **I. Plain Files** | PASS | Key configuration uses standard TOML files (`config.toml`) preserved and edited without losing comments. |
| **II. Disposable Index** | PASS | Keymaps do not modify index structures; refresh (`Ctrl-L`) reloads index and configuration dynamically. |
| **III. Local-Only** | PASS | Keybindings and mode resolution execute 100% locally with zero network activity. |
| **IV. Terminal Only, One Core** | PASS | Domain entities (`Key`, `Code`, `Command`, `Context`, `EffectiveKeymap`, parser, and validation) reside in `kb-core::keys`. The TUI in `kb` is a thin consumer. `kb keys` CLI commands run directly against `kb-core`. |
| **V. Portable by Construction** | PASS | Esc vs Alt disambiguation and key events work identically on macOS and Linux across major terminal emulators (kitty, iTerm2, Terminal.app, Alacritty, tmux). |
| **VI. Secrets Never Stored** | PASS | Key overrides cannot bypass command review or echo secret variables. Confirmation answers for run/delete are immutable. |
| **VII. Keyboard-First and Fast** | PASS | Enables faster single-letter keyboard workflows without modifier fatigue. Esc disambiguation completes within $\le 50$ ms. Performance budgets preserved. |
| **VIII. Test-First Core** | PASS | Comprehensive test suite covering key parser, duplicate checking, preset invariants, override conflict resolution, PTY mode switching, and CLI outputs. |
| **IX. Simplicity First** | PASS | Two clean presets (`emacs`, `vim`) and declarative TOML overrides. No scripting engine, no complex multi-key leader layers beyond 2-key sequences (`gg`), no visual mode. |

---

## Project Structure

### Documentation (this feature)

```text
specs/013-vim-keymap/
├── spec.md              # Feature specification
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── cli.md
│   ├── config.md
│   └── key-commands.md
├── checklists/
│   └── requirements.md
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code Layout

```text
crates/kb-core/src/
├── keys/                # Core keymap domain, commands, parser & resolution
│   ├── mod.rs           # Module exports & types
│   ├── code.rs          # Code & Key definitions (independent of crossterm)
│   ├── command.rs       # Command enum & stable kebab-case names
│   ├── context.rs       # Context enum (Global, Search, Normal, etc.)
│   ├── parse.rs         # Key string parser ("ctrl-o", "g g")
│   ├── preset.rs        # Presets (emacs, vim) & compile-time duplicate assertions
│   └── effective.rs     # EffectiveKeymap builder, override validation & warnings
├── config.rs            # Added tui.keymap setting & [tui.keys.*] table parsing
└── lib.rs

crates/kb/src/
├── tui/
│   ├── keys.rs          # TUI key adapter (crossterm KeyEvent -> kb_core Key)
│   ├── state.rs         # PendingKey state & mode tracking (Search vs Normal)
│   ├── view.rs          # Header mode badge ('NORMAL' / 'SEARCH') & key bar ('g…')
│   └── mod.rs           # Event loop integration & 50ms Esc disambiguation
├── commands/
│   └── keys.rs          # CLI commands: kb keys, kb keys check, kb keys defaults
├── commands.rs          # Command registration & flag parsing
├── doctor.rs            # kb doctor check integration for kb keys check
└── main.rs

crates/kb/tests/
├── tui_vim_keymap.rs    # PTY tests for mode switching, navigation, and vim actions
├── cli_keys.rs          # Integration tests for kb keys, check, defaults
└── fixtures/config/     # Test fixture configs (good, bad, conflicts, reserved keys)
```

---

## Complexity Tracking

*No constitution violations or unjustified complexity.*
