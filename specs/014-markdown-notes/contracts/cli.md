# Contract: CLI Interface (Markdown Notes)

This document defines the CLI flags, behavior, and output contracts for interacting with `note` entries in kb.

---

## 1. Authoring Notes: `kb note` & `kb add --type note`

### Synopsis
```bash
kb note [TITLE] [--tags TAG,...] [--private]
kb add --type note [TITLE] [--tags TAG,...] [--private]
```

### Behavior
1. **Title Parameter**:
   - If `TITLE` is provided, the frontmatter includes `title: "TITLE"`.
   - If `TITLE` is omitted:
     - The editor opens with a template containing empty frontmatter `type: note` and an empty body.
     - Upon saving, if the first non-empty line of the body is a level-1 heading (`# My Heading`), the canonical title is extracted from that heading with any inline Markdown syntax stripped.
     - If neither frontmatter `title` nor a level-1 heading is present, `kb` aborts the save with an error diagnostic.
2. **Template**:
   ```markdown
   ---
   kb_format: 1
   type: note
   title: "Role cache TTL"
   tags: [redis, caching]
   # Add "private" to tags to exclude this note from AI agents
   ---

   # Role cache TTL

   Explain the operational background here.
   ```
3. **Storage Path**:
   - Default path follows `note/<slug-of-title>.md` (e.g., `~/kb/note/role-cache-ttl.md`).
4. **Validation on Save**:
   - Secret scan runs across note body; if matching tokens/keys are found, editor save is blocked.
   - If `vars:` is declared in frontmatter, save is rejected.

---

## 2. Displaying Notes: `kb show`

### Synopsis
```bash
kb show <ENTRY-ID> [--raw]
```

### Behavior
1. **Standard Output (`kb show <id>`)**:
   - Renders formatted terminal Markdown identical to TUI preview rendering.
   - Terminal styles (colors, bold, underlines) are emitted only when stdout is a TTY and `NO_COLOR` is unset.
   - Dangerous ANSI sequences, C0/C1 codes are replaced with visible `␛` markers.
   - OSC 8 hyperlinks are emitted only when stdout is a terminal that supports OSC 8 and link scheme is `http`, `https`, or `mailto`.
2. **Raw Output (`kb show --raw <id>`)**:
   - Outputs the exact Markdown source of the note file.
   - Frontmatter is preserved or stripped per standard kb show flags.
   - Dangerous escape sequences (CSI, OSC, etc.) are stripped completely (no `␛` placeholder) so piping raw text to terminal or files is safe.

---

## 3. Search Filters: `in:body` & `type:note`

### Synopsis
```bash
kb search "ttl in:body"
kb search "redis type:note"
```

### Behavior
1. `type:note`: Restricts search results strictly to entries where `type = 'note'`.
2. `in:body`: Canonical alias for SQLite FTS5 `payload` column. On notes, this restricts matching to the Markdown body text, code blocks, lists, and tables (excluding frontmatter title, tags, and headings).
3. **Snippet Output (`--json`)**:
   ```json
   {
     "id": "note/role-cache-ttl",
     "type": "note",
     "title": "Role cache TTL",
     "score": 12.45,
     "snippet": {
       "field": "payload",
       "text": "The authorizer checks Redis with a cache TTL of 300 s to prevent upstream query exhaustion...",
       "ranges": [[47, 50]]
     }
   }
   ```
   Markdown syntax characters (e.g. backticks, hash markers) are stripped from the snippet string.

---

## 4. Entry Link Maintenance: `kb mv` & `kb rm`

### Synopsis
```bash
kb mv <OLD-ENTRY-ID> <NEW-ENTRY-ID>
kb rm <ENTRY-ID>
```

### Behavior
1. **`kb mv <old> <new>`**:
   - Queries `entry_links` table in SQLite index for all notes referencing `<old>`.
   - Performs atomic, format-preserving replacement of `[[<old>]]` with `[[<new>]]` in referencing note files.
   - Commits all modified files in a single atomic git commit (matching R1 runbook refactoring rules).
2. **`kb rm <id>`**:
   - Checks if any notes reference `<id>` via `entry_links`.
   - If referenced, prints a warning listing all referencing notes and prompts:
     `"Entry 'sumo/5xx' is referenced by 2 notes (note/auth-guide, note/incident-42). Delete anyway? [y/N]"`
   - If cancelled, command exits 0 without modifying the file.
