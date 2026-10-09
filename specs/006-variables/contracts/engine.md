# Contract: the resolution engine (`kb-core::varres`)

The only place `{{name}}` is replaced (FR-004). No I/O, no environment reads, no terminal.

```rust
pub fn plan_vars(entry: &Entry) -> VarPlan;
pub fn prefill(plan: &VarPlan, inputs: &Inputs) -> Vec<Binding>;
pub fn validate(var: &Variable, value: &Value) -> Result<(), VarError>;
pub fn resolve(plan: &VarPlan, values: &Values) -> Result<Resolved, Vec<VarError>>;
/// The same for a live preview: a variable without a valid value keeps its `{{name}}`.
pub fn preview(plan: &VarPlan, values: &Values) -> Resolved;

pub struct Inputs<'a> {
    pub flags: &'a [(String, String)],            // --var, in order; last wins
    pub env: &'a dyn Fn(&str) -> Option<String>,  // env lookup, injected
    pub memory: Option<&'a BTreeMap<String, String>>, // the values remembered for this entry
}
```

- `resolve` validates every variable first and returns **all** errors (FR-006), not the first. It
  never partially substitutes.
- `Values` is `BTreeMap<String, Value>`; the engine ignores names not in the plan (the CLI rejects
  them earlier, exit 2).
- `Resolved` = `ResolutionResult` from the data model. Deterministic: the same entry and values give
  byte-identical output (SC-005).

## Escaping tables (tests are written from these)

`unquoted(value, shell)` → returns `value` if it is non-empty and every char is in
`[A-Za-z0-9_@%+=:,./-]`; else:

| Shell | Output |
|---|---|
| sh, bash, zsh | `'` + value with each `'` → `'\''` + `'` |
| fish | `'` + value with `\` → `\\`, `'` → `\'` + `'` |
| any, empty value | `''` |

`in_single(value, shell)`: sh/bash/zsh replace `'` with `'\''`; fish replaces `\`→`\\`, `'`→`\'`.
`in_double(value, shell)`: sh/bash/zsh backslash-escape `\`, `"`, `$`, `` ` ``; fish `\`, `"`, `$`.

`sql(value)` → `'` + value with `'`→`''` + `'`. `percent(value)` → UTF-8 bytes outside
`A-Za-z0-9-._~` as `%XX` (uppercase hex).

Quote-state tracker: starts *unquoted* at the beginning of a template string; `'` toggles single
(unless inside double or escaped); `"` toggles double (unless inside single or escaped); `\` escapes
the next char when not in single quotes. State is updated only from template literal text.

## Hostile-value table (SC-003)

Every row runs for each shell and each context: `a b`, `a'b`, `a"b`, `` a`b` ``, `$(id)`, `${HOME}`,
`a;b`, `a&&b`, `a|b`, `a>b`, `*`, `~`, `-rf`, `\`, `''`, empty, `{{x}}`, `é日本`, a 10,000-char string,
`feature/x; rm -rf ~`, `a&b c`. Expected outputs are literals in the test, not recomputed. Where
`sh` is available, a test also runs `sh -c 'printf %s ARG'` with the quoted value and compares with the
original (bash/zsh/fish likewise when installed; skipped otherwise).
