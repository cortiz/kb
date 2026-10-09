# Contract: additions to `kb-core` (M3)

Everything the TUI needs from the core. All read-only except `record_use`, which already exists.
The TUI layer calls only these and the M1/M2 APIs (FR-020). Signatures are indicative.

## Refresh and query on a held index

```rust
impl Kb {
    /// Walks the KB, brings the index up to date, and keeps the open index in the handle.
    pub fn refresh(&mut self) -> Result<Refresh, KbError>;

    /// Searches using the held index, without walking the files. Calls `refresh` first if the
    /// handle has no index yet.
    pub fn query(&mut self, query: &Query, now: SystemTime) -> Result<SearchOutcome, KbError>;
}
// search(query, now) == refresh() then query(query, now), as in M2.
pub struct Refresh { pub indexed: usize, pub skipped: usize, pub notices: Vec<Notice>,
                     pub duration: Duration }
```

`query` still checks that each returned entry's file exists (M2's guarantee that a deleted entry is
never returned), so a stale held index cannot show a missing file. If the held index turns out to be
corrupt, `query` rebuilds once as `search` does.

## Per-type counts and result fields

```rust
pub struct SearchOutcome { /* M2 fields */
    pub type_counts: [usize; 7],   // index = EntryType::ALL order; counts candidates for the
                                   // text, ignoring `query.types`, before paging
    pub indexed: usize,            // entries in the index
    pub skipped: usize,            // entries left out because of errors
}
pub struct SearchResult { /* M2 fields */
    pub use_count: u64,            // from state.json; 0 when never used
    pub updated: Option<String>,   // the entry's `updated:` value as written, if any
    pub modified: Option<i64>,     // file modification time, unix seconds
}
```

Behavior change from M2: the `types` filter is applied to the candidate list in Rust instead of in
SQL, so the candidate cap (1,000) applies before it. Results for a query without `type:` are
unchanged. `total` is the count after the type filter.

## Index version 2

`entries` gains `updated TEXT` (nullable); `INDEX_VERSION` becomes 2. An index of version 1 is rebuilt
automatically with M2's notice. No migration code.

## Entry check

```rust
impl Kb {
    /// The errors `kb lint` would report for this entry's file (empty when valid).
    pub fn lint_entry(&self, id: &str) -> Result<Vec<Finding>, KbError>;
}
```

Used after Ctrl-E and by `kb edit`. Reads the file; does not use the index.

## Pick output and entry JSON

```rust
pub enum PrintMode { Payload, Id, Path, Json }
impl PrintMode { pub fn parse(s: &str) -> Option<PrintMode>; }

/// Exactly the bytes `kb show --print <mode>` writes. `Payload` is the payload text for
/// sql/sumo/cw/cmd/prompt, the URL for link, `path:line` for code. No trailing newline is added
/// except after the `Json` document.
pub fn pick_output(entry: &Entry, abs_path: &Path, mode: PrintMode) -> Result<String, PickError>;

/// The document described by contracts/entry.schema.json.
pub fn entry_json(entry: &Entry, abs_path: &Path) -> serde_json::Value;

pub enum PickError { NoPayload }   // a payload type whose payload block is missing (lint error)
```

`Kb::pick(&self, id, mode) -> Result<String, KbError>` wraps `entry` + `pick_output` for the CLI and
the TUI.

## Payload pieces (variable marking)

```rust
pub enum PayloadPiece { Text(String), Var(String) }
/// Splits the payload into literal text (with `\{{` unescaped) and `{{name}}` references, using
/// M1's `scan_refs`, so what the preview marks is exactly what `kb lint` counts as a variable.
pub fn payload_pieces(entry: &Entry) -> Vec<PayloadPiece>;
```

## Config

```rust
pub struct Config { pub theme: ThemeChoice, pub mouse: bool }
pub enum ThemeChoice { Auto, Dark, Light }
pub fn read_config(path: Option<&Path>) -> Result<Config, ConfigError>;   // missing file → defaults
```

Parses the same YAML file as `root`. Unknown keys are ignored. `theme` must be `auto`, `dark` or
`light`; `mouse` must be `true` or `false`; anything else is `ConfigError` naming the key and the
file (CLI exit 2). `resolve_root` is unchanged.

## Guarantees (tested)

1. `refresh` then `query` returns what M2's `search` returns for the same inputs, apart from the
   candidate-cap note above.
2. `type_counts` equals the number of candidates of each type, independent of `query.types`; their
   sum is at least `total` of the same query without `type:`.
3. `query` is cheaper than `search` when files have not changed (no walk).
4. `pick_output` for the same entry and mode gives the same bytes in the TUI and in `kb show`.
5. `payload_pieces` joined back (with `{{name}}` for variables) equals the payload text.
6. The core prints nothing.
