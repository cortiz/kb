# Data Model: Variables

## Variable (extends M1)

| Field | Type | Notes |
|---|---|---|
| name, kind, declared, default, options, description, env, references | as M1 | unchanged |
| `escape` | `Escape` = `auto` (default) \| `none` \| `shell` \| `sql` | new optional key; unknown value → lint KB054 (error), value treated as `auto` |

## VarPlan (not stored)

Built by `plan_vars(&Entry)`.

| Field | Notes |
|---|---|
| `vars: Vec<PlannedVar>` | one per referenced variable, in file order of first reference (FR-001) |
| `PlannedVar` | the `Variable` plus `locations: Vec<Location>` (`Payload`, `Url`, `Cwd`, `Range`, `LogGroup(i)`) |
| `shell: Shell` | the entry's shell (`sh` for non-`cmd`) |
| `template` | the strings to resolve: payload, and the four field values, each as scanned pieces |
| `signature: u64` | hash of names, kinds, options, defaults, escape, shell, pieces; used to detect an edit (R7) |

Rules: a declared variable never referenced is not in the plan. An undeclared one is `text`, no
default, `declared = false`. Notes are not part of the template (FR-002).

## Value

`Value { text: String, secret: bool }`. `Debug` of a secret prints `••••`. Not `Serialize`. `Clone` is allowed (`AppState` derives it); the redacting `Debug` and the absence of `Serialize` are the protections, not move-only ownership.

## Binding

| Field | Notes |
|---|---|
| `name` | |
| `value` | `Option<Value>`; `None` = missing |
| `source` | `Flag \| Env \| Remembered \| Default \| Typed \| None` |
| `secret` | from the variable's kind |

`prefill()` chooses in FR-010 order: Flag → Env (set and non-empty) → Remembered (not secret, and still
valid for the current kind/options) → Default (only if it passes `validate`; an invalid default is ignored, FR-005) → None. A remembered value that fails `validate` is
skipped silently (edge case).

## Validation (FR-005, FR-006)

| Kind | Valid when | Error text |
|---|---|---|
| text | one line, no NUL; non-empty unless `default == Some("")` | `env: expected a single line of text` / `a value is required` |
| number | matches `^-?\d+(\.\d+)?$` | `minutes: expected a number like 30, -5 or 0.25` |
| enum | equals one of `options` | `env: expected one of prod, stage, dev` |
| secret | non-empty, one line | `token: a value is required` (never echoes the input) |

Pasted multi-line text keeps its first line and sets a notice (TUI/prompt).

## Resolved (the spec's ResolutionResult)

| Field | Notes |
|---|---|
| `payload: Option<String>` | resolved payload (for `link`: the resolved `url`) |
| `fields` | resolved `range`, `url`, `cwd`, `log_groups` where present |
| `bindings` | the bindings used (secrets carry `Value` only) |
| `spans` | per output string: `Vec<(Range<usize>, name, secret)>` of substituted text, in output order |
| `has_secret` | true when any secret was substituted (clipboard notice, US3.4) |

Display form for the TUI/`--json`: `redacted()` replaces each secret span with `••••`. The copied/printed
text uses the real value.

## VarMemory (in `.kb/state.json`)

```json
"vars": {
  "entries": { "sumo/5xx-by-authorizer": { "env": "prod", "minutes": "30" } },
  "recent":  { "env": ["prod", "stage"], "minutes": ["30", "15"] }
}
```

| Rule | Source |
|---|---|
| Written only after a successful copy/print, never for `secret` variables | FR-011, FR-013 |
| `recent[name]`: most recent first, deduped, max 20 | FR-012 |
| Skipped values: flagged by `scan_secrets`, with a notice | R5 |
| Rename with unchanged content moves `entries[old]` → `entries[new]` | FR-014, M2 FR-018 |
| `entries[id]` dropped when the entry no longer exists at a full rebuild | FR-014 |
| Missing/invalid `vars` key → empty memory, no reset of `usage` | R5 |

## FormState (TUI, in memory only)

| Field | Notes |
|---|---|
| `entry_id`, `title`, `signature` | for the header and the verify step |
| `fields: Vec<Field>` | `Field { name, kind, options, default, escape, declared, source, env_name, editor: LineEditor \| choice index, secret: bool, error: Option<String> }` |
| `focus`, `scroll` | |
| `suggestions: Option<{items, selected}>` | text kind only, ≤ 5, from `recent[name]` (US5) |
| `then: Stay \| Exit` | `Stay` when opened with Ctrl-Y (US1.4) |
| `session_values` | per entry id, kept for the TUI session after Esc (US1.6), never written; secrets are not kept |

State transitions: `Closed → Loading (worker) → Open → (Submitting → Closed | Open with notice)`;
`Esc` from `Open` → `Closed`.
