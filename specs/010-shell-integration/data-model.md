# Data Model: Shell Integration

**Feature**: Shell Integration  
**Feature Branch**: `010-shell-integration`  
**Milestone**: M8  
**Status**: Completed  

---

## 1. Domain Entities & Value Types

```
  ┌────────────────────────────────────────────────────────┐
  │                     WidgetRequest                      │
  │  - shell: ShellDialect                                 │
  │  - buffer: String (current command line)               │
  │  - cursor: usize (byte offset in buffer)               │
  │  - mode: BufferMode (Query | Insert)                   │
  │  - default_type: Option<EntryType>                     │
  └───────────────────────────┬────────────────────────────┘
                              │
                              ▼
  ┌────────────────────────────────────────────────────────┐
  │                      PickerState                       │
  │  - query: String                                       │
  │  - selected_index: usize                               │
  │  - type_filter: Option<EntryType>                      │
  │  - form: Option<InlineVariableForm>                    │
  │  - preview_enabled: bool                               │
  │  - layout: InlinePickerLayout                          │
  └───────────────────────────┬────────────────────────────┘
                              │
                              ▼
  ┌────────────────────────────────────────────────────────┐
  │                      WidgetResult                      │
  │  - status: WidgetStatus (Picked | Cancelled | Refused) │
  │  - command: Option<String>                             │
  │  - cursor: Option<usize>                               │
  │  - message: Option<String>                             │
  └────────────────────────────────────────────────────────┘
```

---

## 2. Entity Definitions

### 2.1 `ShellDialect`
Identifies the executing shell environment and its syntax conventions.

- **`Zsh`**:
  - Variable reference syntax: `"$VAR"`
  - History recall: `fc -ln -1`
  - Keymap support: `emacs`, `viins`, `vicmd`
  - Multi-line buffer: native newline support in ZLE.
- **`Bash`**:
  - Variable reference syntax: `"$VAR"`
  - History recall: `fc -ln -1`
  - Keymap support: Readline `bind -x`
  - Multi-line buffer: backslash continuation / newline quotes.
- **`Fish`**:
  - Variable reference syntax: `"$VAR"`
  - History recall: `history --max=1`
  - Keymap support: `bind` (default, insert)
  - Multi-line buffer: native newline support in `commandline`.

### 2.2 `BufferMode`
Configures how existing prompt text is treated when the widget opens.

- **`Query` (default)**:
  - When command line has text, that text pre-fills the initial search query.
  - On pick, the picked resolved command **replaces** the entire command line.
- **`Insert`**:
  - Search starts with an empty query.
  - On pick, the resolved command is **inserted at the current cursor position**, preserving text before and after.

### 2.3 `WidgetRequest`
Input payload supplied to `kb pick --widget`.

| Field | Type | Description |
|-------|------|-------------|
| `shell` | `ShellDialect` | Target shell dialect (`zsh`, `bash`, `fish`) |
| `buffer` | `String` | Raw text currently in the shell command line |
| `cursor` | `usize` | 0-based byte offset of the cursor in `buffer` |
| `mode` | `BufferMode` | `Query` or `Insert` behavior |
| `default_type` | `Option<EntryType>` | Default entry filter (`Some(Cmd)` or `None` for all) |
| `height_spec` | `HeightSpec` | Configured inline picker height (e.g. 40% or lines) |

### 2.4 `WidgetResult`
Structured JSON response emitted on stdout when `kb pick --widget` finishes.

| Field | Type | Description |
|-------|------|-------------|
| `status` | `WidgetStatus` | `picked`, `cancelled`, or `refused` |
| `command` | `Option<String>` | The final command string to insert into prompt |
| `cursor` | `Option<usize>` | Target cursor position in prompt (usually end of command) |
| `message` | `Option<String>` | One-line user notice or error message to display |

### 2.5 `InlineVariableForm`
Interactive state displayed when a picked entry contains unresolved variables.

| Field | Type | Description |
|-------|------|-------------|
| `entry_id` | `String` | ID of the entry being resolved |
| `variables` | `Vec<VariableField>` | Declared variables with their current entered/prefilled values |
| `active_index` | `usize` | Currently focused variable row |
| `preview` | `String` | Live one-line preview of the partially/fully resolved command |
| `secret_warnings` | `Vec<String>` | Warnings for unset environment variables or missing sources |

### 2.6 `CompletionKind`
Represents the target domain of a dynamic tab-completion request:

- `Entries`: Matching entry IDs with description titles (`id\ttitle`).
- `Tags`: Distinct tags across all entries.
- `Types`: Fixed seven entry types (`cmd`, `sql`, `sumo`, `cw`, `prompt`, `link`, `code`).
- `Vars(id)`: Declared variable names for a specific entry (`name=`).
- `VarValues(id, var_name)`: Allowed enum values for an enum variable in an entry.

---

## 3. State Transitions

### Inline Picker Lifecycle

```
           [Widget Key Alt-K]
                    │
                    ▼
          ┌───────────────────┐
          │   Inspect Room    │
          │   & Scroll Up     │
          └─────────┬─────────┘
                    │
                    ▼
          ┌───────────────────┐        Esc / Ctrl-C
          │    Search List    ├─────────────────────────────┐
          └─────────┬─────────┘                             │
                    │ Enter (Entry with vars)               │
                    ▼                                       │
          ┌───────────────────┐        Esc                  │
          │ Inline Var Form   ├─────────────────────────────┤
          └─────────┬─────────┘                             │
                    │ Enter (Complete)                      │
                    ▼                                       │
          ┌───────────────────┐                             │
          │  Clear Lines &    │                             │
          │  Restore Cursor   │◄────────────────────────────┘
          └─────────┬─────────┘
                    │
                    ▼
          ┌───────────────────┐
          │ Emit JSON Payload │
          └───────────────────┘
```
