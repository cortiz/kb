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
- **Request Parameters**:
  - `id`: Target entry ID (e.g., `"note/role-cache-ttl"`).
  - `content`: New Markdown content.
  - `tags`: Optional updated tags.
- **Validation**:
  - Enforces secret scanning rules and format validity.
  - Atomically writes updated file and refreshes index.
