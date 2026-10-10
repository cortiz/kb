# Contract: `kb-core::author`

Public surface used by the CLI and TUI. Pure unless marked **I/O**. Signatures are the contract;
bodies are the implementation's business.

## Detection

```rust
pub enum Confidence { Low, Medium, High }

pub struct Detection {
    pub ty: EntryType,
    pub confidence: Confidence,
    pub fields: DraftFields,      // prefilled: link url, code repo/ref/path/line/end_line
    pub body: String,             // unwrapped from a Markdown fence, prompt markers stripped for cmd
    pub hint: Option<String>,     // fence language, if any
}

pub fn detect(text: &str) -> Detection;
pub fn normalize_clipboard(raw: &str) -> String;   // CRLF/CR -> LF, trailing blank lines trimmed
```

Guarantees:

- Deterministic; no I/O; no allocation proportional to anything but the input.
- `detect("")` and whitespace-only text → `cmd`, `Low`, empty body.
- GitHub / GitLab URL with `#L<n>` or `#L<n>-L<m>` → `code`, `High`, fields set (`repo` as `owner/name`,
  `ref` from `/blob/<ref>/`, `path`, `line`, `end_line`). A ref containing `/` is taken up to the first
  segment that looks like a file path start (documented limit; the form lets the user fix it).
- Single `http(s)` URL line → `link`, `High`, `url` set, body empty.
- A fence hint (`sql`; `bash`/`sh`/`zsh`/`shell` → cmd) → that type, `High`.
- Every other rule as in research R2.

## Suggestions

```rust
pub enum SuggestPattern { Env, Region, Time, Account, Id }

pub struct VariableSuggestion {
    pub value: String,
    pub positions: Vec<std::ops::Range<usize>>,
    pub name: String,
    pub kind: VarKind,
    pub options: Vec<String>,
    pub default: String,
    pub pattern: SuggestPattern,
}

pub fn suggest_vars(body: &str, declared: &[VarDecl]) -> Vec<VariableSuggestion>;
```

Guarantees: only the five patterns of FR-015; ignores text inside `{{…}}` / `\{{…}}`; ordered by first
position; names unique against `declared` and each other; every `value` is `[A-Za-z0-9._/-]+`.

## Clock

```rust
pub trait Clock { fn now(&self) -> SystemTime; }
pub struct SystemClock;   // the real clock; tests pass a fixed one
```

## Draft

```rust
pub struct Draft { /* fields in data-model.md */ }

impl Draft {
    pub fn from_text(text: &str, source: DraftSource, opts: &DraftOptions) -> Draft;
    pub fn empty(source: DraftSource, ty: Option<EntryType>, opts: &DraftOptions) -> Draft;

    pub fn set_type(&mut self, ty: EntryType);
    pub fn set_body(&mut self, body: String, redetect: bool);   // rebuilds vars; redetect reruns detect()
    pub fn set_var_kind(&mut self, name: &str, kind: VarKind) -> Result<(), DraftError>;
    pub fn set_var_default(&mut self, name: &str, default: Option<String>) -> Result<(), DraftError>;
    pub fn set_var_options(&mut self, name: &str, options: Vec<String>) -> Result<(), DraftError>;
    pub fn mark_literal(&mut self, name: &str);
    pub fn accept(&mut self, suggestion: usize);
    pub fn target_id(&self, taken: &dyn Fn(&str) -> bool) -> Result<String, DraftError>;

    pub fn render(&self, clock: &dyn Clock) -> Vec<u8>;           // canonical bytes (FR-018)
    pub fn check(&self, id: &str, clock: &dyn Clock) -> Check;   // `id` from target_id
    pub fn template(&self, clock: &dyn Clock) -> String;           // `kb add --editor`
}

pub struct DraftOptions {
    pub title: Option<String>,
    pub ty: Option<EntryType>,        // --type wins over detection
    pub tags: Vec<String>,
    pub path: Option<String>,         // --path
    pub path_template: PathTemplate,
}
```

Rules:

- `from_text` runs `detect`, the title rule (FR-007), `vars` from the body (FR-011, FR-013) and
  `suggest_vars`. `opts.ty` overrides the type and sets confidence `High`.
- `set_var_default` on a `secret` → `DraftError::SecretDefault`; an `enum` default not in `options` →
  `DraftError::DefaultNotInOptions` (the same rules lint enforces, caught at edit time).
- `target_id(taken)` applies the path rule: `Fixed` as typed (validated with `valid_id` rules, `.md`
  suffix removed); `Auto` from the template, with `-2`, `-3` … until `taken` is false (bounded at 999);
  `DraftError::NeedsPath` when the slug is empty.
- `render` is total: any draft renders; `check` reports what is wrong with the bytes.
- `render` never touches `created`/`updated` of an existing file (Draft is for new entries only).

## Check and lint hook

```rust
pub struct Check { pub id: String, pub bytes: Vec<u8>, pub findings: Vec<Finding>, pub too_large: bool }
impl Check {
    pub fn secrets(&self) -> Vec<&Finding>;
    pub fn errors(&self) -> Vec<&Finding>;                // error severity, not secret rules
    pub fn writable(&self, save_anyway: bool) -> bool;
}
// kb-core::lint
pub fn check_text(id: &str, bytes: Vec<u8>) -> Vec<Finding>;   // same checks as `kb lint` on one file
```

`writable(false)` = no findings of error severity and not too large. `writable(true)` = no secret
findings and not too large. A secret finding is any of `KB060`–`KB065` (not suppressed by
`kb:allow-secret`).

## File operations (**I/O**, on `Kb`)

```rust
impl Kb {
    pub fn create_entry(&mut self, check: &Check) -> Result<PathBuf, FsError>;
    pub fn rename_entry(&mut self, old: &str, new: &str,
                        mover: &dyn Fn(&Path, &Path) -> io::Result<()>) -> Result<Renamed, FsError>;
    pub fn remove_entry(&mut self, id: &str,
                        remover: &dyn Fn(&Path) -> io::Result<()>) -> Result<(), FsError>;
    pub fn save_edit(&mut self, id: &str, based_on: &Version, clock: &dyn Clock)
                        -> Result<Saved, FsError>;          // bump `updated`, atomic, conflict-checked
    pub fn restore(&mut self, id: &str, bytes: &[u8]) -> Result<(), FsError>;   // "discard" in kb edit
    pub fn version(&self, id: &str) -> Result<Version, FsError>;
    pub fn tag_counts(&self) -> Vec<(String, u32)>;
    pub fn id_taken(&self, id: &str) -> bool;                // file exists, case-insensitively
}

pub enum FsError {
    BadId(String), NotFound(String), Exists(String), Conflict(String),
    Secret(Vec<Finding>), TooLarge, Io { path: PathBuf, message: String },
}
```

Guarantees:

- `create_entry` refuses unless `check.writable(…)` was satisfied by the caller **and** re-scans the bytes
  for secrets itself (defense in depth: a caller bug cannot write a secret). Atomic, no-clobber, creates
  folders. Calls `refresh` before returning.
- `rename_entry` refuses an existing target (`Exists`), moves state (R7), removes empty folders,
  refreshes. If `mover` fails nothing else happens.
- `remove_entry` calls `remover`, forgets state, removes empty folders, refreshes.
- `save_edit` re-reads, compares `based_on`, sets `updated` with `Entry::set_field(["updated"], …)`, and
  writes through `write_entry` (so the secret scan applies). `Conflict` if the file changed since.
- None of them write outside `<root>/**.md` and `<root>/.kb/`.

## Config (`kb-core::config`)

```rust
impl Settings { pub fn path_template(&self) -> PathTemplate; }
pub struct PathTemplate { /* parsed */ }
impl PathTemplate { pub fn parse(s: &str) -> Result<PathTemplate, String>; pub fn render(&self, ty: EntryType, slug: &str, tags: &[String]) -> String; }
```
