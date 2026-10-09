# Contract: `kb-core` Public API (M1)

The surface the `kb` binary uses in M1, and that M2 (index), M6 (authoring) and M8 (re-pinning)
build on. Signatures are indicative. Names may change during implementation, but the
capabilities and guarantees may not.

## Parsing

```rust
pub fn parse_entry(id: EntryId, path: &Path, bytes: Vec<u8>) -> ParseOutcome;

pub struct ParseOutcome {
    pub entry: Option<Entry>,      // None when the file is structurally unreadable
    pub findings: Vec<Finding>,    // structural and field findings (no secrets, no KB-wide checks)
}
```

- Never panics on any input (checked by a test that feeds it seeded random and truncated bytes;
  no extra dependency).
- `entry` is `Some` whenever the frontmatter parses as a mapping, even with field errors, so
  callers can still show partial data.

## Entry

```rust
impl Entry {
    pub fn id(&self) -> &EntryId;
    pub fn entry_type(&self) -> Option<EntryType>;   // None when the type is unknown
    pub fn title(&self) -> Option<&str>;
    pub fn tags(&self) -> Vec<String>;               // normalized
    pub fn vars(&self) -> &[Variable];               // declared + undeclared references
    pub fn payload(&self) -> Option<Payload>;        // text with \{{ unescaped, plus its span
    pub fn notes(&self) -> String;
    pub fn fields(&self) -> &TypeFields;             // per-type struct, see data-model.md
    pub fn kb_format(&self) -> Option<u32>;
    pub fn to_bytes(&self) -> Vec<u8>;               // FR-016: identical to the input when unmodified
    pub fn set_field(&mut self, path: &[&str], value: FieldValue) -> Result<(), EditError>;
}
```

### `set_field` guarantees (FR-017, FR-018)

- Existing scalar: only the bytes of that value change. The original quoting style is kept when
  the new value can be written in it; otherwise the value is written double-quoted.
- Missing top-level key: a `key: value` line is inserted just before the closing `---`, using the
  file's line ending. Missing nested keys inside a block mapping are inserted after the last
  sibling with the same indentation. Missing keys inside a flow mapping return
  `EditError::Unsupported`.
- The result is re-parsed. If it would be invalid, the entry is unchanged and `EditError::Invalid`
  is returned.
- Returns `EditError::UnsupportedFormat` without changing anything when `kb_format` is greater
  than `MAX_KB_FORMAT` or invalid. Writing to disk is the caller's job: `write_entry` refuses the
  same case.

```rust
pub fn write_entry(path: &Path, entry: &Entry) -> Result<(), WriteError>;
// WriteError::{UnsupportedFormat, Secret(Vec<Finding>), Io(io::Error)}
// Runs scan_secrets on the full bytes first; any unsuppressed finding → Secret, nothing written
// (constitution VI). Atomic: temp file + rename.
```

## KB walk and lint

```rust
pub fn walk(root: &Path, scope: &[PathBuf]) -> Result<Walk, WalkError>; // files + walk findings
pub fn lint(root: &Path, scope: &[PathBuf]) -> Result<LintReport, WalkError>;
pub fn scan_secrets(text: &str) -> SecretScan;  // findings + suppressions; reused by M6 save paths

impl LintReport {
    pub fn exit_code(&self, strict: bool) -> i32;   // 0 or 1
    pub fn summary_line(&self) -> String;
    // Serialize matches contracts/lint-report.schema.json
}
```

## Root resolution and init

```rust
pub struct RootInputs { pub flag: Option<PathBuf>, pub env: Option<OsString>,
                        pub config_file: Option<PathBuf>, pub home: Option<PathBuf> }
pub fn resolve_root(inputs: &RootInputs) -> Result<PathBuf, RootError>;

pub fn plan_init(path: &Path, examples: bool) -> Result<InitPlan, InitError>;  // pure inspection
pub fn apply_init(plan: &InitPlan) -> Result<InitReport, InitError>;           // create-only
```

The binary provides `home` and `config_file` through its `Platform` trait. Core never reads
environment variables or OS directories itself, except where the binary passes them in.
