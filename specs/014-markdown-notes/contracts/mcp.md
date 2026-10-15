# Contract: MCP Server Interface (Markdown Notes)

This document specifies the Model Context Protocol (MCP) behavior for `note` entries, implementing spec 012 integration requirements.

---

## 1. Tools Overview

| Tool Name | Behavior for Notes |
|---|---|
| `search` | Returns matching note entries alongside queries/commands, ranked by relevance. Automatically excludes notes tagged `private`. |
| `get` | Returns the complete Markdown body of the note (excluding YAML frontmatter). Sanitizes titles and metadata. |
| `create` | Validates and creates a new note entry under `note/<slug>.md`. Enforces secret scanning and schema checks. |
| `update` | Validates and updates an existing note entry. Enforces secret scanning and retains/increments timestamps. |

---

## 2. Tool Details

### 2.1 `search`
- **Request Parameters**:
  - `query`: Text query string (e.g. `ttl redis`).
  - `type`: Optional filter (e.g. `"note"`).
  - `tags`: Optional tag filters.
- **Filtering Invariant**:
  - Notes containing the tag `"private"` are automatically excluded from search results unless the MCP client explicitly passes an administrative override.
- **Response Format**:
  ```json
  {
    "entries": [
      {
        "id": "note/role-cache-ttl",
        "type": "note",
        "title": "Role cache TTL and Redis architecture",
        "tags": ["redis", "architecture"],
        "snippet": "The authorizer caches user roles in Redis for 300 seconds..."
      }
    ]
  }
  ```

### 2.2 `get`
- **Request Parameters**:
  - `id`: Canonical entry ID (e.g., `"note/role-cache-ttl"`).
- **Response Format**:
  ```json
  {
    "id": "note/role-cache-ttl",
    "type": "note",
    "title": "Role cache TTL and Redis architecture",
    "tags": ["redis", "architecture"],
    "created": "2026-10-14",
    "updated": "2026-10-14",
    "content": "# Role cache TTL and Redis architecture\n\nThe authorizer caches user roles in Redis for 300 seconds...\n\nSee [[sumo/5xx-by-authorizer]]."
  }
  ```
  Note: `content` returns the raw Markdown body. The frontmatter is parsed into structured JSON fields.

### 2.3 `create`
- **Request Parameters**:
  - `type`: Must be `"note"`.
  - `title`: String (optional if `content` starts with `# Title`).
  - `content`: String (the Markdown body).
  - `tags`: Array of strings (optional).
- **Validation**:
  - If `content` triggers secret scanning rules, `create` returns a protocol error:
    `"Error: SecretDetected - Note content contains sensitive credentials."`
  - Creates file at `~/kb/note/<slug>.md`.

### 2.4 `update`
- **Request Parameters** (the shape of spec 012's `update`, with `content` added to `changes`):
  - `id`: Target entry ID (e.g., `"note/role-cache-ttl"`).
  - `version`: The `version` of the note as `get` returned it.
  - `changes.content`: New Markdown body (`changes.notes` is accepted as a synonym).
  - `changes.tags` / `add_tags` / `remove_tags`: Optional tag changes. `changes.title` sets an explicit title.
  - `payload`, `vars`, `set_vars` and `remove_vars` are errors for a note.
- **Validation**:
  - Enforces secret scanning rules (`SecretDetected`) and format validity.
  - Atomically writes the updated file and refreshes the index. `created` is kept; `updated` moves.
  - A note whose title comes from its first `# heading` keeps doing so.

### 2.5 Implementation notes (as built)
- `search` also takes `types` (array) as spec 012 defined it; `type` (string) is the same filter for one type.
- Private notes are hidden through `mcp.exclude_tags`, whose default is `["private"]`. Removing `private` from that setting is the administrative override; the tool call itself has no such parameter.
- `get` returns the Markdown in `content` (without escape sequences), `notes` is empty for a note, and
  `created` / `updated` are included for every type.
- `create` takes `content` (or `notes`, or `payload`) as the body; `type: "note"` is implied when `content` is given.
