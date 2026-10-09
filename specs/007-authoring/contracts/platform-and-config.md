# Contract: platform additions, doctor, config

## Platform trait (`crates/kb/src/platform/mod.rs`)

```rust
// clipboard
fn run_read_tool(&self, tool: &NativeTool, timeout: Duration) -> Result<Vec<u8>, ToolError>;
// processes and files that authoring needs
fn run_git(&self, root: &Path, args: &[&str], timeout: Duration) -> Result<GitOutput, ToolError>;
fn private_temp_file(&self, extension: &str, contents: &[u8]) -> io::Result<TempFile>;
```

- `run_read_tool`: runs the tool with no stdin, captures stdout up to 2 MiB (`ToolError::Io("too large")`
  beyond), kills it after `timeout` (`ToolError::TimedOut`). Same process handling as `run_copy_tool`.
- `run_git`: `git -C <root> <args>`, stdout and stderr captured, 10 s default. Never inherits the
  terminal. Returns exit status and both streams.
- `private_temp_file`: system temp dir, name `kb-<pid>-<random>.<ext>`, `create_new`, mode `0600`.
  `TempFile` deletes the file on `Drop`; `path()` and `read_to_string()` for the caller. It is
  never created inside the KB root (checked by a boundary test that the returned path is not under the
  root passed to the caller).
- `FakePlatform` gains `with_read(tool_name, FakeRead::{Text, Bytes, Missing, Timeout})`,
  `with_git(script)` and an in-memory temp file store. The M4 `platform_boundary` test is extended:
  `Command::new` and `OpenOptions` for temp files appear only under `platform/`.

## Clipboard read (`platform/clipboard.rs`)

| Constant | Program | Arguments |
|---|---|---|
| `PBPASTE` | `pbpaste` | (none) |
| `WL_PASTE` | `wl-paste` | (none) |
| `XCLIP_OUT` | `xclip` | `-selection clipboard -out` |
| `XSEL_OUT` | `xsel` | `--clipboard --output` |

```rust
pub fn read_tools_for(env: &Environment) -> Vec<NativeTool>;   // macOS: pbpaste; Linux: wl-paste (Wayland), xclip, xsel (X11)
pub fn read(platform: &impl Platform) -> ReadOutcome;
```

`read` order: SSH → `Unavailable(OverSsh)`; no tools for this environment → `Unavailable(NoTool)`;
try each tool (2 s each): missing → next, failure → next, timeout → next; first success decides.
Result rules (FR-003): invalid UTF-8 or NUL → `Empty`; normalize line endings; whitespace-only →
`Empty`. A tool that exits non-zero with empty output (Wayland "nothing is copied") is `Empty`, not a
failure, when it is the last tool tried.

## `kb doctor` check

| Field | Value |
|---|---|
| id | `clipboard-read` |
| pass | `reads with pbpaste` (tool name) |
| warn | `cannot read the clipboard: <reason>`; hint = install hint (`install wl-clipboard`, …) or "over SSH kb add uses stdin or an empty form" |
| JSON | same `Check` shape as the other checks; `details` lists tools tried and found |

The check only looks for the tools (`find_tool`); it never reads the user's clipboard (privacy: doctor
output may be pasted into bug reports).

## Config

New setting in `config::SETTINGS` (twelfth):

| Key | Kind | Default | Env | Doc |
|---|---|---|---|---|
| `path_template` | Text | (empty = `{type}/{slug}`) | `KB_PATH_TEMPLATE` | Where `kb add` puts new entries: placeholders `{type}`, `{slug}`, `{tag}` (first tag, else the type). Must contain `{slug}`. |

Behavior: parsed by `PathTemplate::parse`; an invalid value is a `ConfigWarning` and the default is
used. `kb config list|get|set|init`, the `config init` template and the settings-count tests are
updated ("eleven" becomes "twelve").

## State (`.kb/state.json`)

No new keys, no version bump. `State::move_entry` and `State::forget` as in data-model.md. Both leave
unknown top-level keys untouched (M2 guarantee).
