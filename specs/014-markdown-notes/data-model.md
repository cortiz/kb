# Data Model: Markdown Notes

This document specifies the logical data model, in-memory structures, index schemas, and validation rules for `note` entries in kb.

---

## 1. Domain Entities

### 1.1 Note Entry (`kb_core::entry::Entry`)

A `Note` is an entry of type `note`. Unlike command or query entries, it has no separate payload block; its entire body represents free-form Markdown text.

```rust
pub struct NoteEntry {
    /// Canonical entry ID (e.g., "note/role-cache-ttl").
    pub id: String,
    /// Constant EntryType::Note.
    pub entry_type: EntryType,
    /// Title from frontmatter or extracted from first level-1 heading.
    pub title: String,
    /// User-assigned tags (e.g., ["redis", "architecture", "private"]).
    pub tags: Vec<String>,
    /// Creation timestamp (ISO 8601 / RFC 3339 or YYYY-MM-DD).
    pub created: Option<String>,
    /// Last update timestamp.
    pub updated: Option<String>,
    /// Full Markdown body (excluding frontmatter).
    pub body: String,
}
```

#### Validation Rules & Lint Invariants
| Code | Severity | Condition | Suggested Fix / Action |
|---|---|---|---|
| `E-NOTE-TITLE` | Error | Note lacks frontmatter `title` AND body has no level-1 heading (`# ...`) | Add `title: "..."` in frontmatter or `# Title` at start of body |
| `E-NOTE-VARS` | Error | Note frontmatter declares a `vars:` key | Remove `vars:` (notes do not support variable interpolation) |
| `E-SECRET-DETECTED` | Error | Note body matches known secret pattern (AWS key, private key, token assignment) | Remove secret credentials from note body |
| `W-NOTE-OVERSIZED` | Warning | Note body byte length exceeds 64 KB ($65,536$ bytes) | Split large note into separate linked entries |
| `W-ENTRY-LINK-BROKEN` | Warning | Entry reference `[[<target-id>]]` does not exist in the knowledge base | Fix target ID or create missing entry (fuzzy match suggestion) |
| `I-NOTE-EMPTY` | Info | Note has valid frontmatter but completely empty body | Add content or documentation to note body |

---

### 1.2 Entry Link (`kb_core::entry::link::EntryLink`)

Represents an internal reference from a note to another knowledge base entry.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryLink {
    /// Canonical target entry ID referenced in the markup (e.g., "sumo/5xx-by-authorizer").
    pub target_id: String,
    /// Absolute byte range of the `[[...]]` markup in the source file.
    pub span: std::ops::Range<usize>,
    /// Cached resolved entry type (None if link is broken).
    pub resolved_type: Option<EntryType>,
    /// Cached resolved title (None if link is broken).
    pub resolved_title: Option<String>,
}

impl EntryLink {
    pub fn is_broken(&self) -> bool {
        self.resolved_type.is_none()
    }
}
```

---

### 1.3 Rendered Note (`kb::tui::markdown::RenderedNote`)

An in-memory cached visual representation of a note formatted for a specific terminal display width and color theme.

```rust
pub struct RenderedNote {
    /// 64-bit content hash of the note file.
    pub content_hash: u64,
    /// Display column width used for text wrapping.
    pub width: u16,
    /// Theme identifier used for syntax and UI coloring.
    pub theme_id: String,
    /// Cached parsed AST blocks for zero-reparse width reflowing (FR-016).
    pub blocks: Vec<Block>,
    /// Layout-wrapped lines ready for ratatui rendering.
    pub lines: Vec<RenderedLine>,
    /// Internal entry links detected in the document.
    pub links: Vec<EntryLink>,
    /// Code block highlighting state.
    pub code_blocks: Vec<CodeBlockState>,
    /// Timestamp of last access (for LRU eviction).
    pub last_accessed: std::time::Instant,
}

pub struct CodeBlockState {
    /// Line range within `lines` occupied by this code block.
    pub line_range: std::ops::Range<usize>,
    /// Syntax language tag (e.g., "rust", "sql", "mermaid").
    pub language: String,
    /// Whether syntect syntax highlighting has been computed.
    pub is_highlighted: bool,
    /// Raw lines stored for lazy evaluation.
    pub raw_lines: Vec<String>,
}

pub struct RenderedLine {
    /// Styled text spans making up the visible line.
    pub spans: Vec<StyledSpan>,
    /// Visual display width in columns (handling wide chars/emoji).
    pub display_width: usize,
}
```

---

## 2. Storage & SQLite Index Schema

The disposable search index (`.kb/index.db`) schema is incremented from `INDEX_VERSION = 2` to `INDEX_VERSION = 3`.

```sql
-- Existing metadata table
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

-- Existing entries table
CREATE TABLE IF NOT EXISTS entries (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    title TEXT NOT NULL,
    title_fold TEXT NOT NULL,
    tags TEXT NOT NULL,
    code_repo TEXT,
    updated TEXT
);

-- NEW: Tracking internal entry link relationships for kb mv / kb rm
CREATE TABLE IF NOT EXISTS entry_links (
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    PRIMARY KEY(source_id, target_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS entry_links_target ON entry_links(target_id);

-- FTS5 search table (existing schema, new column mapping for notes)
-- Columns: title, tags, id, meta, payload, notes
CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5 (
    title,
    tags,
    id,
    meta,
    payload,
    notes,
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);

-- Typo expansion vocabulary table
CREATE TABLE IF NOT EXISTS vocab (
    term TEXT PRIMARY KEY,
    len INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS vocab_len ON vocab(len);
```

### Column Mapping Matrix

| Entry Type | `title` | `tags` | `id` | `meta` | `payload` | `notes` |
|---|---|---|---|---|---|---|
| **Query / Command** (`sql`, `sumo`, `cw`, `cmd`) | Title | Tags | ID | Vars, Range, Shell, Log Groups | Code Block Query/Command | Body prose |
| **Prompt** (`prompt`) | Title | Tags | ID | Vars | Prompt text block | Body prose |
| **Link** (`link`) | Title | Tags | ID | Vars, URL host/path | *(empty)* | Body prose |
| **Code** (`code`) | Title | Tags | ID | Repo, Path, Symbol, Vars | *(empty)* | Body prose |
| **Note** (`note`) | Title (frontmatter or stripped `# Title`) | Tags | ID | Headings 1–3, Entry link titles, Link URLs | Body text, tables, lists, code blocks ($\le 256$ KB) | *(empty)* |

---

## 3. Typo Vocabulary Isolation Rules

1. **Incremental Update (`insert_doc`)**:
   - For entries where `type != "note"`, all non-empty columns (`cols[0..=5]`) contribute words to `vocab`.
   - For entries where `type == "note"`, ONLY `cols[0..=3]` (`title`, `tags`, `id`, `meta`) contribute words to `vocab`. `cols[4]` (`payload` = note body) is strictly excluded from `vocab`.
2. **Full Rebuild (`apply_reset`)**:
   - During full reindex, vocabulary is populated either by iterating through scanned `Doc` objects applying the rule above, or via selective SQL queries that exclude note rows for the `payload` column.
