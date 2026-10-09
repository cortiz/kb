# Data Model: Entry Format and Parser

All types live in `kb-core`. "Span" means a byte range into the source text (after the BOM).
Field rules reference the rule codes in [contracts/lint-rules.md](contracts/lint-rules.md).

## Source

The lossless representation of one file.

| Field | Type | Notes |
|-------|------|-------|
| `text` | `String` | File content after the BOM. Valid UTF-8 (otherwise KB005 and no `Source`). |
| `bom` | `bool` | Written back by `to_bytes()` |
| `line_ending` | `Lf \| CrLf \| Mixed` | `Mixed` → KB050. Inserted lines use CRLF only when the file is `CrLf`. |
| `line_starts` | `Vec<usize>` | Byte offset of each line, for line/column lookups |
| `char_to_byte` | lazy `Option<Vec<usize>>` | Built only for non-ASCII text, because the YAML parser reports character offsets (research R2) |
| `frontmatter` | `Span` | Between the delimiter lines |
| `body` | `Span` | After the closing delimiter line |

## Spanned YAML node

`Node = Scalar { value: String, style: Plain | Single | Double | Literal | Folded, span }
| Seq { items: Vec<Node>, flow: bool, span } | Map { entries: Vec<(Key, Node)>, flow: bool, span }`.
`Key` keeps its own span so findings can point at keys. Keys are unique (KB009). Anchors, aliases
and tags never reach the tree (KB008).

## EntryId

A string: the path relative to the root, without `.md`, with `/` separators. Its case-folded form
is used to detect collisions (KB053). Not stored in the file.

## Entry

| Field | Type | Source | Validation |
|-------|------|--------|------------|
| `id` | `EntryId` | path | — |
| `path` | `PathBuf` | path | — |
| `source` | `Source` | file | — |
| `tree` | `Node::Map` | frontmatter | KB006, KB007, KB008, KB009 |
| `kb_format` | `Option<u32>` | `kb_format` | required (KB010), integer ≥ 1 (KB011), ≤ `MAX_KB_FORMAT` = 1 (KB012) |
| `entry_type` | `Option<EntryType>` | `type` | required (KB013), known (KB001) |
| `title` | `Option<String>` | `title` | required (KB014), non-blank, ≤ 120 characters (KB015) |
| `tags` | `Vec<String>` | `tags` | list (KB018); strings, non-empty (KB020); normalized: lowercase, no leading `#`; duplicates (KB021) |
| `created`, `updated` | `Option<String>` | same keys | date or RFC 3339 date-time (KB022) |
| `vars` | `Vec<Variable>` | `vars` + references | see Variable |
| `fields` | `TypeFields` | type-specific keys | see below |
| `payload` | `Option<Payload>` | body | required for payload types (KB035), non-blank (KB036) |
| `notes` | `String` | body minus payload | — |
| `unknown_keys` | `Vec<(String, Span)>` | other top-level keys | KB016 (info), preserved |

`Entry` exists whenever the frontmatter parses as a mapping. Field errors leave the field `None` or
empty rather than dropping the entry.

## EntryType

| Variant | Payload | Type-specific keys | Required keys |
|---------|---------|--------------------|---------------|
| `Sql` | yes | `dialect`, `database` | — |
| `Sumo` | yes | `range` | — |
| `Cw` | yes | `log_groups`, `range`, `region` | — |
| `Cmd` | yes | `shell`, `cwd` | — |
| `Prompt` | yes | — | — |
| `Link` | no | `url` | `url` (KB030) |
| `Code` | no | `code` | `code` (KB032) |

A type-specific key on another type → KB017 (warning).

## TypeFields

| Variant | Fields |
|---------|--------|
| `Sql` | `dialect: Option<String>`, `database: Option<String>` |
| `Sumo` | `range: Option<String>` |
| `Cw` | `log_groups: Vec<String>`, `range: Option<String>`, `region: Option<String>` |
| `Cmd` | `shell: Shell` (default `Sh`; KB034), `cwd: Option<String>` |
| `Prompt` | — |
| `Link` | `url: Option<String>` (absolute http/https, KB031) |
| `Code` | `code: Option<CodeRef>` |
| `Unknown` | — (when `type` is missing or unknown) |

## CodeRef

| Field | Type | Validation (KB033) |
|-------|------|--------------------|
| `repo` | `String` | required; `owner/name`, or a git remote starting with `https://`, `http://`, `ssh://`, `git://`, `git@` or `file://` |
| `path` | `String` | required; relative, no `..` component, `/` separators |
| `line` | `u32` | required; ≥ 1 |
| `ref` | `String` | required; non-empty |
| `symbol` | `Option<String>` | — |
| `end_line` | `Option<u32>` | ≥ `line` |

## Variable

| Field | Type | Notes |
|-------|------|-------|
| `name` | `String` | `[a-z][a-z0-9_]*` (KB040) |
| `kind` | `VarKind` | `Text`, `Number`, `Enum`, `Secret`; required when declared (KB041, KB042); `Text` when undeclared |
| `declared` | `bool` | false → KB047 (info) |
| `default` | `Option<Scalar>` | forbidden for `Secret` (KB043); in `options` for `Enum` (KB045); numeric for `Number` (KB046) |
| `options` | `Vec<String>` | required and non-empty for `Enum` (KB044) |
| `description` | `Option<String>` | — |
| `env` | `Option<String>` | environment variable name |
| `references` | `Vec<VarRef>` | empty and declared → KB048 (warning) |

`VarRef { location: Payload | Field("range" | "url" | "cwd" | "log_groups[i]"), span }`.
Scanning: `\{{` is skipped and emitted as a literal. `{{name}}` with a valid name is a reference.
Any other `{{…}}` on one line is KB049 (info) and stays literal.

## Payload

| Field | Type | Notes |
|-------|------|-------|
| `text` | `String` | Block content with `\{{` turned into `{{`, always with LF line endings (the file keeps its own) |
| `raw_span` | `Span` | Content bytes in the file |
| `fence_span` | `Span` | Whole block including fences |
| `info` | `Option<String>` | Info string, for example `sql` (kept, not validated) |

## Finding

| Field | Type | Notes |
|-------|------|-------|
| `path` | `String` | Relative to the root |
| `id` | `Option<EntryId>` | — |
| `line`, `column` | `u32` | 1-based; column counts Unicode characters |
| `severity` | `Error \| Warning \| Info` | — |
| `code` | `RuleCode` | Enum whose display form is `KB001-unknown-type` |
| `message` | `String` | Never contains more than 4 characters of a secret |
| `fix` | `Option<String>` | — |

## Suppression

`{ path, line, code }`: a secret finding silenced by a trailing `kb:allow-secret`.

## LintReport

| Field | Type | Notes |
|-------|------|-------|
| `root` | `PathBuf` | — |
| `entries` | `usize` | `.md` files examined (including ones with errors) |
| `findings` | `Vec<Finding>` | sorted by path, line, column, code |
| `suppressions` | `Vec<Suppression>` | — |
| counts | derived | errors, warnings, info, suppressed |

Exit policy: `exit_code(strict) = 1 if errors > 0 || (strict && warnings > 0) else 0`.

## InitPlan and InitReport

`InitPlan { root, actions: Vec<InitAction>, examples: Created | SkippedFlag | SkippedHasEntries }`
where `InitAction = CreateDir(path) | CreateFile(path, contents) | Exists(path)`. `apply_init`
runs `CreateDir` and `CreateFile` with exclusive create and returns an `InitReport` (created and
existing paths, plus every step in plan order for display) matching
[contracts/init-report.schema.json](contracts/init-report.schema.json).

## State transitions

None in M1. Entries are read-only except through `Entry::set_field`, which goes from
**parsed** to **modified** (in memory) to **written** (`write_entry`). Entries with an unsupported
`kb_format` cannot leave **parsed**.
