# Implementation Plan: Variables

**Branch**: `006-variables` | **Date**: 2026-10-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/006-variables/spec.md`

## Summary

M5 makes `{{variables}}` work. A new `resolve_vars` module in `kb-core` lists an entry's variables,
pre-fills them, validates values, quotes them for where they land, and substitutes in one pass. The TUI
form (screen 2) and `kb use` / `kb copy` / `kb vars` are thin callers of it.

Five decisions carry the design (reasons and rejected alternatives in [research.md](research.md)):

1. **One pure engine, three callers.** `plan_vars()`, `prefill()`, `validate()` and `resolve()` are pure
   functions of plain values (entry, env lookup, flags, memory, typed values). The form, the CLI
   prompt and `--var` all feed the same `Values` map, so SC-005 (interactive == scripted, byte for byte)
   holds by construction.
2. **Quote by shell context, not just by value.** Under `escape: auto`, a `cmd` payload is scanned
   with a small quote-state tracker (unquoted / `'…'` / `"…"`), and the value is escaped for the state
   it lands in. Plain "quote the value" would turn `echo "{{x}}"` into `echo "'a b'"`, which is wrong.
   This is the one interpretation of FR-008 that goes beyond its literal text (R3).
3. **Secrets are a type, not a convention.** Values travel as `Value { text, secret }`; `Debug`
   prints `••••`; `Binding`, the form state and the `Outcome` carry no plain `String` for a secret.
   `State` has no field that could hold one (R6). SC-004 is checked by a scan test, not by review.
4. **Memory is an additive key in `state.json`.** `vars: { entries, recent }` next to `usage`;
   `STATE_VERSION` stays 1 (M2 already keeps unknown top-level keys). Rename-carry and prune extend the
   existing M2 code (R5).
5. **The form lives in the TUI as an overlay, fed by the worker.** Opening and submitting go through
   the worker (read the entry fresh, compare its variable signature), which gives the "re-read on open
   and before copying" edge case without a second code path (R7).

Facts found in the code that shaped the plan:

- `Entry::vars()` is in declaration order with undeclared ones appended, and each `Variable` carries
  `references` with file spans. **Field order is "first appearance"** (FR-001), so the engine sorts by
  the smallest reference span, not by `vars()` order.
- `payload_pieces()` already splits a payload into text and `Var` pieces with the lint scanner. The
  engine reuses `scan_refs` + `body::unescape` for the payload **and** for the four frontmatter string
  fields (their parsed value is scanned; file spans do not map onto YAML-escaped scalars).
- `PostPick` (M4) is `Copy | Print(mode) | Show`. The form opens only when the post-pick delivers the
  payload (`Copy`, `Print(Payload)`), so `--print id|path|json` and `Show` never ask for values.
- `Outcome::Picked { id }` exits the UI and `main` copies. The form adds `Outcome::Resolved` carrying
  the submitted values; Ctrl-Y with a form stays open and goes through the existing copy effect.
- `unsafe_code = forbid` rules out `termios`. Masked prompts use `crossterm` raw mode on the tty
  (already a dependency, `use-dev-tty` already on) and print nothing for the typed keys (R8).
- `kb copy` already has `--method` and `--json`. `kb use` and `kb copy` share one argument struct.
- Lint's `definition()` ignores unknown keys inside a variable; `escape` needs a new rule code
  (KB054: the 04x block for variable rules is full and KB050–053 are file-level; see contracts/state-and-format.md).

## Technical Context

**Language/Version**: Rust 2024, MSRV 1.88, `unsafe_code = forbid` (unchanged).

**Primary Dependencies**: existing `ratatui`, `crossterm`, `serde_json`, `clap`, `regex`. **No new
dependency.** Percent-encoding, shell quoting and the quote-state scanner are ~120 lines (R2, R3).

**Storage**: `.kb/state.json` gains a `vars` key (R5). Nothing new in the KB root.

**Testing**: `cargo test`. Red-first unit tests in `kb-core` for the engine (hostile-value tables for
each shell and for URLs, prefill order, validation); `FakePlatform` for CLI logic; the pty harness for
the form; a scan test for SC-004; release timing for SC-002.

**Target Platform**: macOS (arm64, x86_64) and Linux (x86_64), unchanged.

**Project Type**: Cargo workspace, `kb-core` + `kb`.

**Performance Goals**: re-resolve on each keystroke ≤ 16 ms for 1,000-line payloads (SC-002):
one linear scan over the payload; the pieces are computed once when the form opens and only the
substitution is redone.

**Constraints**: a secret never reaches disk, logs, JSON or error text (FR-013); `kb use` reads
prompts from `/dev/tty` (FR-023); the engine is the only substitution site (FR-004, enforced by a
boundary test like M4's FR-003).

**Scale/Scope**: 1 new core module (+ small edits to `vars.rs`, `usage.rs`, `lint`, `entry_json`),
2 new subcommands (`use`, `vars`), 1 changed (`copy`), 1 new TUI overlay, 1 prompt module.

## Constitution Check

*GATE: pass before research; re-checked after design (second column).*

| Principle | Before | After design | Notes |
|---|---|---|---|
| I. Plain files | Pass | Pass | Nothing written to the KB root. `escape` is a plain frontmatter key a human can write. |
| II. Index is disposable | Pass | Pass | Index untouched. Last-used values live in `state.json`, exactly as the principle says. |
| III. Local-only | Pass | Pass | No network. Env vars and `/dev/tty` only. |
| IV. Terminal only, one core | Pass | Pass | Engine in `kb-core`; TUI and CLI only collect input and render. FR-004 is enforced by a test. `use` and `vars` have `--json`; every form action has a flag (`--var`, `--yes`, `--print`). |
| V. Portable | Pass | Pass | Tty access goes through `Platform::open_tty` / `has_terminal`; env lookup is a closure so tests do not touch the process environment. |
| VI. Secrets never stored | Pass | Pass | See decision 3 and R6. `--var token=…` is allowed with a shell-history warning. |
| VII. Keyboard-first and fast | Pass | Pass | Form is keyboard-only; SC-002 has a release test. The "resolved command shown before running" rule is M7's; M5 only copies. |
| VIII. Test-first core | Pass | Pass, one open gate | Engine, escaping, prefill, memory: failing tests first. Form, prompt and wiring: tested after, as in M3/M4. "CI green on macOS and Linux" needs your push (task gate, as in M4). |
| IX. Simplicity first | Pass | Pass | No new dependency. Context-aware quoting is the one extra piece and is justified in Complexity Tracking. |

**Gate result**: no violations.

## Project Structure

### Documentation (this feature)

```text
specs/006-variables/
├── plan.md
├── research.md          # Decisions R1–R10
├── data-model.md        # Entities and rules
├── quickstart.md        # Runnable validation scenarios
├── contracts/
│   ├── engine.md        # kb-core API: plan, prefill, validate, resolve, escaping tables
│   ├── cli.md           # use, vars, copy changes: arguments, prompts, output, exit codes
│   ├── tui-form.md      # screen 2: layout, keys, effects, worker requests, outcomes
│   └── state-and-format.md  # `escape` key, lint rule, `vars` in state.json, config key
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks output (not created here)
```

### Source Code (repository root)

```text
crates/
├── kb-core/src/
│   ├── varres/              # NEW module directory
│   │   ├── mod.rs           # VarPlan, Binding, Values, plan_vars(), prefill(), validate(), resolve()
│   │   ├── escape.rs        # shell quoting (sh/bash/zsh/fish), sql literal, percent-encoding,
│   │   │                    #   quote-state tracker for cmd payloads
│   │   └── secret.rs        # Value / Secret newtype with redacting Debug
│   ├── vars.rs              # CHANGE: Variable gains `escape: Escape`; `definition()` reads it
│   ├── lint/mod.rs          # CHANGE: RuleCode InvalidEscape (error)
│   ├── usage.rs             # CHANGE: State.vars (VarMemory), carry_renames/prune extended,
│   │                        #   parse/save the `vars` key
│   ├── search.rs            # CHANGE: Kb::record_resolved_use(id, memory update)
│   ├── entry_json.rs        # CHANGE: variables carry `escape`
│   └── config.rs            # CHANGE: SETTINGS gains `vars.remember` (bool, default true)
└── kb/src/
    ├── varcli.rs            # NEW: shared args (VarArgs), prompt loop, masked tty input, `kb vars`
    ├── commands.rs          # CHANGE: run_use (copy_entry resolves first), run_pick resolved path
    ├── main.rs              # CHANGE: `use`, `vars`, copy flags (--var --yes --print --no-remember --raw)
    └── tui/
        ├── form.rs          # NEW: VarForm state, update(), field editing, suggestions
        ├── app.rs           # CHANGE: Overlay::Vars, Effect::{OpenForm, SubmitForm, CopyResolved}, Alt-C
        ├── keys.rs          # CHANGE: form mode bindings, Alt-C, Alt-Enter (no duplicates; build test)
        ├── view.rs          # CHANGE: draw_form (wide and narrow layouts)
        ├── worker.rs        # CHANGE: Request::LoadForm / Verify
        └── mod.rs           # CHANGE: Outcome::Resolved
crates/kb/tests/
├── cli_use.rs  cli_vars.rs   # NEW
├── tui_form_pty.rs           # NEW (screen 2 end to end)
├── secrets_scan.rs           # NEW (SC-004)
├── varres_boundary.rs        # NEW (FR-004: no `{{` substitution outside kb-core::varres)
└── perf.rs                   # extend: SC-002
tests/fixtures/vars/          # NEW: one entry per kind/escape/location, hostile-value table
```

**Structure Decision**: engine in `kb-core` (constitution IV); everything that reads a key or a
terminal is in `kb`. `varres` is a directory because escaping is separately testable; it is not split
further.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| Quote-state tracker for `cmd` payloads | `echo "{{x}}"` and `echo '{{x}}'` are common in saved commands; value-only quoting corrupts them or, worse, leaves `$(…)` live inside `"…"` | Plain quoting is ~10 lines shorter and wrong in exactly the places users write quotes. The tracker is ~40 lines and has a stated ceiling (R3: no heredocs, `$(…)` nesting, or comments). |
| `Value` newtype with redacting `Debug` | `AppState` and `Options` derive `Debug`/`Clone` (so `Value` is `Clone` too); a bare `String` in the form would print in a panic message (FR-013, "crash reports") | Remembering not to log is not a control. The newtype is ~25 lines. |
| Worker round trip to open/verify the form | Gives "re-read when opened and again before copying" for free and keeps file reads off the UI thread, like every other read in M3 | Reading synchronously in `update()` would break the pure-update pattern the tests rely on. |

## Behavior changes and documentation

- **`kb copy` on an entry with variables now resolves** (FR-021). Scripts that relied on getting
  `{{x}}` back must add `--raw`. Entries without variables behave exactly as before.
- **`kb --print payload` / `tui.enter = "print"` now print the resolved text** for entries with
  variables, after the form. The spec does not say this; it follows FR-021's reason (no accidental
  placeholders). `kb show`, `--print id|path|json` are untouched.
- **Ctrl-Y on an entry with variables opens the form** (stays open after copy, US1.4).
- **README**: new sections for `use`, `vars`, the `escape` key, secrets, and the new keys; the TUI
  key table gains Alt-C. Done in the same change (constitution).
- **M1 format doc / fixtures README**: `escape` documented as an optional variable key.

## Post-design re-check

Re-read after the data model and contracts: the table above stands. Risks that are real:

- **Alt-Enter and Ctrl-Z in terminals.** Alt-Enter arrives as ESC+CR and crossterm decodes it as
  Alt+Enter, but some terminals (and tmux without `xterm-keys`) swallow it; Ctrl-Z needs raw mode so
  it is delivered as a key and does not suspend. Both are in the pty test and the manual matrix.
- **fish quoting is from documented practice** (`\\` and `\'` inside single quotes), not run here
  (no fish on this machine unless installed); the tests check bytes. A `fish -c` round trip test is
  skipped when `fish` is absent.
- **Echo-free input on `/dev/tty`** relies on raw mode working on the tty fd; verified in the pty
  harness, not on a real SSH session.

## Post-implementation re-check

What changed between the plan and the code, so the documents stay true:

- **Constitution**: unchanged result, no new dependency. VIII's CI-green gate is task T062 and still
  needs your push; T060 (hand walk of the quickstart) and T061 (terminal matrix) need a person.
- **Lint rule is KB054, not KB052.** KB052 and KB053 were already taken (M2's symlink and ID-case
  rules); the plan had only looked at the 04x block.
- **No worker round trip for the form.** `Effect::Pick` / `CopyPayload` read the entry in the effect
  handler (`FormSpec::load`) and `Effect::SubmitForm` re-reads it (`recheck`), exactly like the
  existing Ctrl-Y copy does. Same two re-read points, no new protocol (R7 and `contracts/tui-form.md`
  say so).
- **The form handles its own keys.** The shared KeyMap only gained `Alt-C` (list modes) and a
  `Mode::Form`; Alt-Enter, Alt-Down, Ctrl-Z, Ctrl-R are in `FormState::handle_key`. The README's
  "every bound key is documented" test and the help-overlay snapshots (6 files, one new row) were
  updated for Alt-C.
- **`resolve(plan, values)`** takes no entry (the plan already holds the strings); `preview()` is the
  lenient variant for the live preview; `Inputs.memory` is the entry's map, not the whole `VarMemory`.
- **The preview shows every resolved string**: `cwd`, `range`, `url` and `log_groups[i]` first (one
  line each, `label: text`), then the payload. Long lines wrap; it does not scroll (a `ponytail:`
  marker in `tui/form_view.rs`), so a payload taller than the pane is cut. `kb use --json` carries the
  same strings under `fields`, secrets masked.
- **Review fixes**: `kb copy --raw` from the picker copies the template (`PostPick::Raw`) instead of
  opening the form; Alt-Enter in a form opened with Ctrl-Y prints and exits; the `stdout` clipboard
  fallback prints a secret like `--print` does (documented, and pinned by a test).
- **Quote tracker ceiling** is as planned (`ponytail:` in `varres/escape.rs`). `sh`, `bash` and `zsh`
  round-trip every hostile value in the tests; `fish` is skipped here because it is not installed.
- **Existing tests changed on purpose**: `cli_copy.rs` (`--print` on `kb copy` is now a flag, not a
  usage error), `cli_config.rs` and `config.rs` (ten settings became eleven), `cli_index.rs` and
  `usage.rs` (the unknown top-level key they used to prove "kept" was `vars`, which now means
  something; they use `future`), `lint/mod.rs` and `lint_fixtures.rs` (one more rule, one more
  fixture), and the six `help.*` snapshots.
- **Measured** (release, this Mac): a keystroke plus a full 200x60 frame for a 1,000-line payload,
  p95 6.2 ms (budget 16 ms); `kb use <id> -y --print` on 10,000 entries, median 6.7 ms (budget 100 ms).
- **Not built**: preview scrolling; `fish` execution in CI; anything under T060–T062.
