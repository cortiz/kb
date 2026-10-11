# TUI Views & Keybinding Contracts: Git Sync

This document specifies the visual components, layouts, and keybindings added to the terminal interface in Milestone 9 (`009-git-sync`).

---

## 1. Main List Header Sync Indicator (FR-028)

When the KB root is a git repository, the main list header displays a persistent sync status badge next to the entry count.

### 1.1 Status Badges

| Condition | Indicator Rendering | Description |
|---|---|---|
| Clean & in sync | `git ✓ in sync (12m ago)` | Repository is up to date with upstream tracking branch |
| Local commits unpushed | `git ↑2` | 2 local commits ahead of upstream |
| Remote commits unpulled | `git ↓3` | 3 remote commits behind upstream (from last fetch) |
| Diverged | `git ↑2 ↓3` | Ahead by 2 and behind by 3 |
| Uncommitted hand edits | `git ● 4 changed` | 4 entry files modified, added, or deleted outside kb |
| Unresolved conflict | `git ⚠ conflict` | Sync was interrupted or paused on unresolved conflicts |
| Fetch failed (auth/timeout) | `git ⚠ fetch auth` | Background fetch failed or required credentials |

---

## 2. Main List Keybindings (FR-029)

Added to the M3/M6/M7 keymap without collisions:

| Keybinding | Action | Behavior |
|---|---|---|
| `Alt-S` | Trigger Sync | Starts foreground sync; header shows `syncing...`; status bar reports results or hands terminal to conflict screen |
| `Alt-H` | Entry History | Opens the History modal overlay for the selected entry |

---

## 3. History Modal Overlay (Screen / Overlay) (FR-027, FR-029)

Opens when the user presses `Alt-H` on an entry.

### 3.1 Visual Layout

```text
┌─ History: sumo/5xx-authorizer ────────────────────────────────────────┐
│ Date         Author         Commit   Categories      Message          │
│ 2026-10-10   Carlos Ortiz   a1b2c3d  payload, tags   kb: edit sumo... │
│ 2026-10-09   Carlos Ortiz   e4f5a6b  variables       kb: add sumo...  │
│ 2026-10-08   Carlos Ortiz   7c8d9e0  import          kb: initial i... │
│                                                                       │
│ [Enter] View Diff   [r] Restore Revision   [Esc] Close                │
└───────────────────────────────────────────────────────────────────────┘
```

### 3.2 Keybindings
- `↑` / `↓` / `k` / `j`: Select historical commit
- `Enter`: Open full diff view for the selected commit
- `r`: Prompt to restore entry from this revision (`kb: restore <id> from <rev>`)
- `Esc`: Close history overlay and return to the main list

---

## 4. Conflict Resolution Screen (FR-016, FR-030)

Dedicated full-screen modal displayed when `kb sync` encounters integration conflicts.

### 4.1 Visual Layout ($\ge 120$ Columns: Side-by-Side)

```text
┌─ Conflict 1 of 2: sumo/5xx-authorizer (both modified) ────────────────┐
│ MINE (Local)                            │ THEIRS (Remote)             │
│ ---                                     │ ---                         │
│ title: 5xx by authorizer                │ title: 5xx Authorizer Sumo  │
│ type: sumo                              │ type: sumo                  │
│ tags: [latency, lambda]                 │ tags: [latency, prod, p99]  │
│ ---                                     │ ---                         │
│ _sourceCategory=prod/lambda             │ _sourceCategory=prod/lambda │
│ | parse "error: *" as err               │ | parse "error: *" as err   │
│ | count by err                          │ | count by err, env         │
├─────────────────────────────────────────┴─────────────────────────────┤
│ Frontmatter differences:                                              │
│ - title: "5xx by authorizer" vs "5xx Authorizer Sumo"                 │
│ - tags: [latency, lambda] vs [latency, prod, p99]                     │
├───────────────────────────────────────────────────────────────────────┤
│ [m] Mine  [t] Theirs  [e] Edit Merged  [b] Keep Both  [Esc] Abort     │
└───────────────────────────────────────────────────────────────────────┘
```

### 4.2 Visual Layout ($< 120$ Columns: Stacked)
- Top pane: "Mine (Local)"
- Bottom pane: "Theirs (Remote)"
- Diff and frontmatter summary placed below panes.

### 4.3 Keybindings
- `m`: Keep local version ("mine")
- `t`: Keep remote version ("theirs")
- `e`: Suspend TUI and launch editor (`$EDITOR`) with conflict markers
- `b`: Keep both versions (local as original ID; remote saved as `<id>-conflict-<short-hostname>`). If that file already exists, prompts interactively for a unique ID before saving (clarified in Session 2026-10-10).
- `d`: Delete entry (for edit/delete conflicts)
- `n` / `p` or `Tab`: Navigate to next / previous conflict
- `Esc`: Abort sync with confirmation dialog (`Abort sync and restore repository? (y/n)`)
