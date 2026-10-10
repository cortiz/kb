# Contract: TUI Screens and Keybindings

**Branch**: `008-actions-code-links` | **Date**: 2026-10-09 | **Spec**: [spec.md](../spec.md)

This contract defines the visual layout, interactive states, and keyboard bindings for the TUI Action Menu, Code View (screen 3), and Command Run modals.

---

## 1. Main List Keybindings & Key Bar

### New Keybindings

| Key | Context | Action |
|---|---|---|
| `Ctrl-O` | Main List | Primary open action for selected entry (`code`: Code View; `link`/`sumo`/`cw`: Open URL) |
| `Ctrl-G` | Main List | Run action for selected `cmd` entry (opens confirmation modal) |
| `Alt-A` / `F2` | Main List | Open Action Menu modal for selected entry |
| `Enter` | Main List | If `tui.enter = "primary"`, runs primary open. If `tui.enter = "copy"`, copies payload. **Enter never runs a command.** |

### Dynamic Key Bar
The footer key bar dynamically adapts to the selected entry type:
- `link`: `Enter:copy  Ctrl-O:open  Ctrl-E:edit  Alt-A:actions`
- `sumo` / `cw`: `Enter:copy  Ctrl-O:console  Ctrl-E:edit  Alt-A:actions`
- `cmd`: `Enter:copy  Ctrl-G:run  Ctrl-E:edit  Alt-A:actions`
- `code`: `Enter:copy  Ctrl-O:code view  Ctrl-E:edit  Alt-A:actions`

---

## 2. Action Menu Modal (`Alt-A` / `F2`)

Renders as a centered popup overlay modal on top of the list view.

```text
┌─ Actions: 5xx by authorizer (sumo) ─────────────────────────┐
│                                                             │
│  [o] Open in Sumo Logic (primary)                           │
│  [c] Copy resolved query                                    │
│  [p] Run in psql (custom)                                   │
│                                                             │
│  Press letter to run • Esc to close                         │
└─────────────────────────────────────────────────────────────┘
```

### Interactions
- Pressing any listed action key runs that action.
- If the selected action requires variables (M5), the Variable Form opens first.
- Pressing `Esc` dismisses the menu without action.

---

## 3. Code View (Screen 3)

Opens when the user presses `Ctrl-O` on a `code` entry (or Enter when `tui.enter = "primary"`).

```text
┌─ AuthorizerHandler.handleRequest (code) ────────────────────────────────────────────────────────┐
│ Repo: auth0/auth0-java  •  Path: src/main/java/AuthorizerHandler.java:147-155  •  Ref: 4a2b1c8   │
│ Status: moved (now at line 151 on origin/main)                                                  │
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 144 │     public void configure(Context ctx) {                                                  │
│ 145 │         this.ctx = ctx;                                                                   │
│ 146 │     }                                                                                     │
│ 147 │ ▶   public Response handleRequest(Request req) {                                          │
│ 148 │         if (req.token == null) {                                                          │
│ 149 │             throw new UnauthorizedException("Missing token");                             │
│ 150 │         }                                                                                 │
│ 151 │         return authService.authorize(req);                                                │
│ 152 │     }                                                                                     │
│ 153 │                                                                                           │
│ 154 │     public void shutdown() {                                                              │
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│ [Enter] open (web)  [g] web  [v] vscode  [i] idea  [y] path:line  [Y] permalink  [u] repin  ... │
│ [F] fetch  [Ctrl-E] edit  [Esc] back                                                            │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### Key Controls

| Key | Action | Behavior |
|---|---|---|
| `Enter` | Default Open | Launches default target (`code.default_open`, defaults to `web`) |
| `g` | Web Permalink | Opens browser permalink for pinned commit SHA |
| `v` | VS Code | Opens `vscode://file/<abs_path>:<line>` |
| `i` | IntelliJ | Opens `idea://open?file=<abs_path>&line=<line>` |
| `y` | Copy Location | Copies `path:line` to clipboard |
| `Y` | Copy Permalink| Copies full web permalink to clipboard |
| `u` | Re-pin | Proposes new commit ref and lines; updates file on confirmation |
| `F` | Fetch & Recheck | Runs `git fetch` on clone and refreshes drift status |
| `Ctrl-E` | Edit Entry | Opens entry Markdown file in external editor |
| `Esc` | Back | Returns to main list view |

### Empty & Error States
- **No Local Clone Found**:
  - Code panel displays instructions:
    `No local clone found for repo 'owner/name'. Configure [code.repos] or code.search_paths.`
  - Keys `v` and `i` are disabled and grayed out. `g`, `y`, and `Y` remain functional.
- **Over SSH**:
  - Editor keys `v` and `i` disabled with notice `(Editor launch disabled over SSH)`.

---

## 4. Run Confirmation & Execution Flow

### Standard Command Prompt
```text
┌─ Run Command ───────────────────────────────────────────────┐
│ Command: tail -f /var/log/authorizer.log                    │
│ Shell:   /bin/sh                                            │
│ Cwd:     /Users/carlos/deploy                               │
│ Secrets: KB_SECRET_AUTH_TOKEN                               │
│                                                             │
│ Execute command? [y/N]                                      │
└─────────────────────────────────────────────────────────────┘
```

### Destructive Command Prompt
```text
┌─ WARNING: Destructive Command ──────────────────────────────┐
│ Command: terraform destroy -auto-approve                    │
│ Warning: Matches destructive pattern 'terraform destroy'    │
│ Shell:   /bin/bash                                          │
│ Cwd:     /Users/carlos/infra                                │
│                                                             │
│ Type 'run' to execute, or Esc to cancel: [_______]          │
└─────────────────────────────────────────────────────────────┘
```

### Post-Execution Pause Screen
After process exits in foreground:
```text
[Command exited with code 0 in 1.4s] Press any key to return to kb...
```
- If `run.pause_after = false` and exit code is 0, skips wait screen and returns to TUI immediately.
- If exit code $\ne$ 0, always pauses.
