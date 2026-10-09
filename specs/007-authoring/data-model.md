# Data Model: Authoring

Types live in `kb-core::author` unless noted. Plain values; no I/O except in `fsops`.

## Draft

An entry being created (this milestone does not edit existing entries through a `Draft`; `kb edit`
works on the file).

| Field | Type | Notes |
|---|---|---|
| `source` | `DraftSource` | `Clipboard`, `Stdin`, `Empty`, `Editor`. Shown in the form header. |
| `entry_type` | `EntryType` | Starts as the detected type (default `cmd` when detection returned nothing and focus goes to the type field). |
| `detection` | `Detection` | Kept so the header can say "detected: cw (high)". |
| `title` | `String` | May be empty until save; validated 1–120 characters (M1). |
| `tags` | `Vec<String>` | Lowercase kebab as M1 `tag` rule; duplicates dropped on entry. |
| `fields` | `DraftFields` | Type-specific: `Sql{dialect,database}`, `Sumo{range}`, `Cw{log_groups,range,region}`, `Cmd{shell,cwd}`, `Link{url}`, `Code{repo,git_ref,path,line,end_line,symbol}`. Pre-filled by detection (R2). |
| `body` | `String` | The payload text without the fence; `\n` endings. Empty for `link` and `code`. |
| `notes` | `String` | Optional Markdown after the fence. |
| `vars` | `Vec<VarDecl>` | Declared variables in first-appearance order (see below). |
| `suggestions` | `Vec<VariableSuggestion>` | Pending; removed when accepted. |
| `path` | `PathChoice` | `Auto` (derived from template, title, tags; re-derived on change) or `Fixed(String)` (user typed or `--path`). |
| `based_on` | `Option<Version>` | `None` for a new entry. Reserved for conflict detection when editing. |

Rules:

- Changing `entry_type` keeps `title`, `tags`, `body`, `notes`, `vars`; drops type fields that don't
  apply to the new type and re-runs nothing (the user chose).
- `link` / `code` ignore `body` on render (a pasted URL is converted: `fields.url` set, `body` cleared).
- `vars` is rebuilt from the body whenever it changes: names in first-appearance order; existing
  declarations (kind, default, options, literal) are kept by name; new names get the name-based default
  kind (R4); declarations whose name no longer appears are dropped.
- `check` fails when: title empty/too long (`KB014`/`KB015` come from lint, so no duplicate rule here),
  path empty after slug (needs a path), id invalid (`valid_id`), id exists, rendered file over 1,048,576 bytes.

## VarDecl

`{ name, kind: VarKind, default: Option<String>, options: Vec<String>, literal: bool }`.
`literal: true` is not written as a declaration; the body carries `\{{name}}` instead.
`secret` kind has `default: None` always; setting a default on a secret is refused by the form.

## Detection

`{ ty: EntryType, confidence: Confidence, fields: DraftFields (prefilled), body: String, hint: Option<String>, stripped_prompts: bool }`.
`Confidence = High | Medium | Low`. `detect` of empty or whitespace text is `{ cmd, Low }` with an empty
body (the form then focuses the type field, as the spec says for an empty clipboard).

## VariableSuggestion

`{ value: String, positions: Vec<Range<usize>> (byte ranges in body), name, kind, options, default, pattern: SuggestPattern }`.
`SuggestPattern = Env | Region | Time | Account | Id`. `positions` cover the replaced part only (digits
for `Time`). `accept(&mut Draft, index)` replaces every position (back to front), adds a `VarDecl` with
`default = Some(value)`, and removes the suggestion; other suggestions' positions are recomputed by
re-running `suggest_vars` and dropping ones already accepted.

## Version

`{ hash: u64, len: u64, mtime_ns: i64 }` from `usage::content_hash` and file metadata. Equality of `hash`
and `len` is the conflict test; `mtime_ns` is informational.

## Check

`{ bytes: Vec<u8>, id: String, findings: Vec<Finding>, too_large: bool }` (`too_large`: rendered bytes exceed `MAX_FILE_BYTES`, 1,048,576) with `fn secrets(&self)`,
`fn errors(&self)` (error severity, non-secret), `fn writable(&self, save_anyway: bool) -> bool`:
no secrets, not too large, and (no errors or `save_anyway`).

## ReadOutcome (kb crate, `platform::clipboard`)

`Text(String) | Empty | Unavailable(Unavailable)` where
`Unavailable = OverSsh | NoTool { hint: String } | Failed(String) | TimedOut | TooLarge`.
Only `Text` carries content. `Unavailable` is turned into a one-line explanation by the caller
(`kb add` prints it on stderr; the form shows it in the header).

## State changes (`usage.rs`)

- `State::move_entry(old, new)`: moves `usage[old]` and `vars.entries[old]` to `new` (overwriting any
  stale record), returns whether anything moved.
- `State::forget(id)`: removes both records, returns whether anything was removed.
- `STATE_VERSION` unchanged (no new keys).

## Config

`path_template` (`Kind::Text`, default empty = `{type}/{slug}`, env `KB_PATH_TEMPLATE`). Parsed once into
`PathTemplate { parts }`; invalid → default plus a `ConfigWarning`.

## State transitions: the add form

```text
Opening ──(clipboard text)──> Editing(focus: Title)
Opening ──(empty / unavailable)──> Editing(focus: Type)
Editing ──Ctrl-E──> InEditor ──(returned)──> Editing   (body replaced, detection + vars re-run)
Editing ──Ctrl-P──> Previewing ──Ctrl-P/Esc──> Editing
Editing ──Ctrl-S──> Saving ──ok──> Closed(Saved id)
                          └─fail─> Editing(error shown, focus on the offending field)
Editing ──Esc──> (unchanged) Closed(Cancelled) | (changed) Confirming ──y──> Closed(Cancelled) / ──n──> Editing
```
