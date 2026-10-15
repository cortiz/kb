# Contract: Note Entry File Format

This document defines the file syntax, frontmatter schema, supported Markdown extensions, and escaping rules for `note` entry files (`kb_format: 1`).

---

## 1. File Structure

A note entry is a single UTF-8 file stored in the knowledge base root under `note/<slug>.md`.

```markdown
---
kb_format: 1
type: note
title: "Role cache TTL and Redis architecture"
tags: [auth, redis, architecture]
created: 2026-10-14
updated: 2026-10-14
---

# Role cache TTL and Redis architecture

The authorizer caches user roles in Redis for 300 seconds to protect the PostgreSQL primary database.

## Architecture

See operational telemetry in [[sumo/5xx-by-authorizer]].

### Cache Invalidation Table

| Event | Action | TTL |
|:------|:-------|:---:|
| Role Revoke | Redis DEL | Immediate |
| Role Grant | Redis SETEX | 300 s |
| Session Expiry | Evicted | Natural |

> [!WARNING]
> Never set TTL lower than 60 seconds during active incidents without team approval.

```python
# Check Redis TTL directly
import redis
r = redis.Redis(host='redis.internal', port=6379)
print(r.ttl("role:user_42"))
```
```

---

## 2. Frontmatter Schema (`YAML`)

| Key | Type | Requirement | Description |
|---|---|---|---|
| `kb_format` | Integer | Mandatory | Format revision; MUST be `1`. |
| `type` | String | Mandatory | MUST be `"note"`. |
| `title` | String | Optional* | Explicit entry title. *May be omitted if body begins with a level-1 heading (`# Title`). |
| `tags` | Array of Strings | Optional | List of classification tags (e.g. `[redis, private]`). |
| `created` | String | Optional | Timestamp (e.g. `2026-10-14` or `2026-10-14T10:00:00Z`). |
| `updated` | String | Optional | Timestamp of last modification. |
| `vars` | Any | **FORBIDDEN** | Declaring variables on a note triggers a fatal lint error (`E-NOTE-VARS`). |

---

## 3. Supported Markdown Features & Extensions

1. **Headings**:
   - Level 1 (`#`): Underlined with horizontal rule (`───`), bold text.
   - Level 2 (`##`): Underlined with horizontal rule, styled header text.
   - Level 3 (`###`): Bold header text without horizontal rule.
   - Levels 4–6: Dimmed or compact bold headers.
2. **Inline Styles**:
   - `**bold**`, `*italic*`, `~~strikethrough~~`, `` `inline code` ``.
3. **Lists**:
   - Unordered (`-`, `*`), Ordered (`1.`, `2.`), nested indentations.
   - Task lists: `- [ ]` renders as `☐ Item`; `- [x]` renders as `☑ Item` with dimmed text.
4. **Tables**:
   - GFM table syntax with column alignment (`:---`, `:---:`, `---:`).
   - Rendered using Unicode box borders (`┌─┬┐`, `│`, `├─┼┤`, `└─┴┘`). Cell text wraps to fit.
5. **Callouts (GitHub style)**:
   - `> [!NOTE]` (Blue info icon `ℹ`)
   - `> [!TIP]` (Green bulb icon `💡`)
   - `> [!IMPORTANT]` (Purple exclamation icon `❗`)
   - `> [!WARNING]` (Yellow warning icon `⚠`)
   - `> [!CAUTION]` (Red stop icon `🛑`)
6. **Fenced Code Blocks**:
   - Enclosed in box-drawn frames with language tag header (`┌─ [rust] ─┐`).
   - Highlighted via `syntect`. Languages without a syntax definition render as unhighlighted framed text.
7. **Entry Links**:
   - `[[<entry-id>]]` renders as `[<badge>] <title>` (e.g., `[sumo] 5xx by authorizer`).
   - Broken link (target ID missing): Renders as `[?] <entry-id>` in warning red text.
   - Escaped syntax `\[[` renders as literal `[[`.
8. **Footnotes**:
   - Inline reference `[^1]` renders as bracketed number `[1]`.
   - Footnote definitions `[^1]: Note text` collected and rendered at document footer.
9. **Images**:
   - `![Alt text](path/to/img.png)` renders as text fallback `[image: Alt text] (path/to/img.png)`.

---

## 4. Terminal Safety & Control Sequence Sanitization

Before emitting text to a terminal (via TUI or `kb show`):
1. **Control Characters**: All C0 and C1 characters (except `\t` and `\n`) are stripped.
2. **ANSI Sequences**: All ESC (`0x1B`), CSI, OSC, DCS, APC, and PM escape sequences are intercepted. In TUI and standard `kb show`, they are replaced by `␛`. In `kb show --raw`, they are stripped cleanly.
3. **Hyperlinks (OSC 8)**: Emitted exclusively by kb itself, restricted to `http`, `https`, and `mailto` protocols, and only when the terminal advertises OSC 8 support.
4. **HTML**: All raw HTML tags (e.g. `<script>`, `<div>`) are rendered as literal text.
