# Inline Picker UI Contract: Shell Integration

**Feature**: Shell Integration  
**Feature Branch**: `010-shell-integration`  
**Milestone**: M8  
**Status**: Completed  

---

## 1. Terminal Layout

The inline picker renders in the standard terminal screen buffer directly below the prompt.

```
$ aws logs tail █                                   <-- Prompt line
┌── Search: tail ───────────────────────[cmd]───────┐ <-- Line 1
│ > cmd/tail-authorizer-logs  Tail authorizer logs  │ <-- Line 2
│   cmd/tail-gateway-errors   Tail API GW errors    │ <-- Line 3
│   cmd/follow-worker-logs    Follow queue worker   │ <-- Line 4
└───────────────────────────────────────────────────┘ <-- Status / Key hints
```

### Constraints
- Height is clamped between 10 lines and 40% of terminal height.
- If terminal lines $< 8$, fallback to full alternate screen buffer (`EnterAlternateScreen`).
- When width $\ge 120$ columns and preview is enabled, layout splits 50/50 horizontally with syntax-highlighted entry preview on the right.

---

## 2. Inline Variable Form (FR-013)

When an entry containing variables is selected, the list is replaced by the compact inline variable form:

```
$ aws logs tail █
┌── Variables for cmd/tail-authorizer-logs ─────────┐
│   env: [prod              ]                       │ <-- Active input field
│   stage: [v1]                                     │
│ ───────────────────────────────────────────────── │
│ Preview: aws logs tail /aws/lambda/authorizer-prod│ <-- Live preview
└──────────────────────────────────────[Enter] Pick ┘
```

### Key Bindings
- `Tab` / `Shift-Tab`: Move focus between variable fields.
- `←` / `→`: Cycle options for enum variables.
- `Enter`: Complete variable resolution and submit.
- `Esc` / `Ctrl-C`: Cancel and return to search list.
