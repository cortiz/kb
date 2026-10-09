# Contract: the platform layer

Module `kb::platform`. All OS-specific behavior is here (FR-001, FR-003).

```rust
pub trait Platform {
    // paths
    fn home_dir(&self) -> Option<PathBuf>;
    fn config_files(&self) -> Vec<PathBuf>;        // read order; toml first, legacy yml last
    fn config_write_path(&self) -> Option<PathBuf>;
    fn data_dir(&self) -> Option<PathBuf>;
    fn cache_dir(&self) -> Option<PathBuf>;
    // environment
    fn env(&self) -> Environment;
    fn find_tool(&self, name: &str) -> Option<PathBuf>;     // PATH lookup
    fn tmux_allows_passthrough(&self) -> Option<bool>;      // None: not in tmux
    // terminal
    fn has_terminal(&self) -> bool;
    fn open_tty(&self) -> io::Result<File>;
    fn write_tty(&self, bytes: &[u8]) -> io::Result<()>;    // OSC 52 target
    // clipboard
    fn native_tools(&self, env: &Environment) -> Vec<NativeTool>;
    fn run_copy_tool(&self, tool: &ToolCommand, text: &str, timeout: Duration) -> Result<(), ToolError>;
    // opening
    fn opener(&self, override_cmd: Option<&str>) -> OpenerCommand;
    fn open(&self, target: &str, opener: &OpenerCommand, env: &Environment) -> io::Result<Opened>;
    // editor
    fn resolve_editor(&self, configured: Option<&str>) -> Editor;   // R3 order; reads $VISUAL/$EDITOR
    fn run_editor(&self, editor: &Editor, path: &Path, tty: &File, interrupt: &dyn Fn() -> bool)
        -> io::Result<Option<ExitStatus>>;
}
```

No method has a default body. Implementations: `MacOs`, `Linux`, `FakePlatform`.

## Pure functions (no `Platform` needed, unit-tested on any host)

| Function | Spec |
|---|---|
| `Environment::detect(get, os)` | FR-002, research R8 |
| `chain(env, settings, tools)` | FR-005, FR-006, clarification 1; research R4 |
| `osc52::sequence(text, wrap, max_bytes)` | FR-007; `Wrap::{None, Tmux, Screen}` |
| `clipboard::copy(platform, settings, text, id) -> CopyOutcome` | FR-008, FR-009 |
| `config_paths(os, xdg, home)` | FR-011 |

## FakePlatform

Constructed with an `Environment` and a table of fake tools (`name → behaviour`:
`Ok`, `Fail(code)`, `Hang`). It records every call (`Vec<Call>`) so tests assert what happened and
in which order. Its `run_copy_tool` returns the scripted result immediately (a `Hang` returns
`ToolError::TimedOut` without waiting).

## Invariants

- `copy` returns a `CopyOutcome`, never an `Err`. The chain ends at `Stdout`; `used` is `None`
  only for the size refusal (research R7). `used = Stdout` means the caller prints the text.
- A copy attempt is bounded by 2 s (FR-009), enforced in the real `run_copy_tool`.
- Nothing outside `platform/` names a tool, display variable or `cfg(target_os)` (checked by
  `tests/platform_boundary.rs`).
