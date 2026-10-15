# Quickstart: Markdown Notes Validation Guide

This guide outlines runnable end-to-end scenarios to validate that `note` entries, Markdown rendering, terminal sanitization, and search indexing function properly.

---

## Prerequisites

1. Working Rust toolchain (Rust 1.88+).
2. Clean fixture knowledge base or temporary test repository (`KB_DIR=$(mktemp -d)`).
3. Built `kb` binary:
   ```bash
   cargo build --bin kb
   ```

---

## Scenario 1: Author and Search a Note

### Step 1.1: Create Note via CLI
```bash
# Author note with title
kb note "Redis Role Cache TTL" --tags "redis,architecture"
```
In your editor, paste:
```markdown
# Redis Role Cache TTL

The authorizer caches user permissions in Redis for 300 seconds to protect the PostgreSQL primary database.

## Invalidation Rules

- Roles revoked: immediate DEL
- Session timeout: natural eviction
```
Save and exit.

### Step 1.2: Validate Search Retrieval & Ranking
```bash
# Query words from the note's body
kb search "postgresql eviction" --json
```
**Expected Outcome**:
- The note `note/redis-role-cache-ttl` is returned as the top result.
- A snippet is extracted centered around the matching paragraph.

---

## Scenario 2: Terminal Markdown Rendering & TUI Preview

### Step 2.1: Render via `kb show`
```bash
kb show note/redis-role-cache-ttl
```
**Expected Outcome**:
- Level-1 heading is bold with an underline rule beneath it.
- Bullet points render with indentation.
- No raw YAML frontmatter is shown.

### Step 2.2: Raw Source Output
```bash
kb show --raw note/redis-role-cache-ttl
```
**Expected Outcome**:
- Emits exact Markdown source text to standard output without terminal escape styling.

---

## Scenario 3: Terminal Safety & Control Sequence Sanitization

### Step 3.1: Create Note with Malicious Escapes
Create a test note file directly in the knowledge base:
```bash
cat << 'EOF' > "$KB_DIR/note/security-test.md"
---
kb_format: 1
type: note
title: "Security Test"
---

# Malicious Note

Here is a dangerous terminal sequence: \x1b[2J\x1b[H\x1b]0;Pwned\x07

Here is raw HTML: <script>alert("evil")</script>

Here is a bad link: [Click me](javascript:alert(1))
EOF
```

### Step 3.2: Verify Safe Rendering
```bash
kb show note/security-test
```
**Expected Outcome**:
- Screen is NOT cleared. Window title is NOT changed.
- Escape codes are replaced with visible `␛` placeholders.
- Raw HTML `<script>...</script>` is rendered as literal text.
- `javascript:` link is NOT emitted as an OSC 8 terminal hyperlink.

---

## Scenario 4: Internal Entry Links & Link Maintenance

### Step 4.1: Author Note with Entry Links
```markdown
# Incident Playbook

1. Check operational telemetry in [[sumo/5xx-by-authorizer]].
2. Run database health check in [[cmd/pg-stat-activity]].
3. Non-existent link: [[cw/missing-log-group]].
```

### Step 4.2: Verify Preview Rendering
```bash
kb show note/incident-playbook
```
**Expected Outcome**:
- `[[sumo/5xx-by-authorizer]]` renders as `[sumo] 5xx by authorizer`.
- `[[cmd/pg-stat-activity]]` renders as `[cmd] Postgres active queries`.
- Broken link renders with fallback warning: `[?] cw/missing-log-group`.

### Step 4.3: Validate Atomic Refactoring with `kb mv`
```bash
kb mv sumo/5xx-by-authorizer sumo/5xx-authorizer-v2
```
**Expected Outcome**:
- Note file `note/incident-playbook.md` is automatically updated to reference `[[sumo/5xx-authorizer-v2]]`.
- Git history shows a single atomic commit with the move and reference update.

---

## Scenario 5: MCP Agent Integration & Privacy

### Step 5.1: Agent Search (Excluding Private)
Tag a sensitive note with `private`:
```bash
kb note "Internal Credentials Runbook" --tags "private,security"
```
Run MCP `search` tool:
**Expected Outcome**:
- Private note is completely excluded from search results returned to the agent.
