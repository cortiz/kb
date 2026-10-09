//! Commands that do what a TUI action does, so every TUI action is scriptable.

use std::process::ExitCode;

use kb_core::entry_json::PrintMode;
use kb_core::search::Kb;

use crate::output::CliError;
use crate::platform::Platform;

fn io_error(e: impl std::fmt::Display) -> CliError {
    CliError::new("io", e.to_string())
}

/// `kb show`: what picking `id` in the TUI prints, without the UI and without recording a use.
pub fn run_show(root: &std::path::Path, id: &str, mode: PrintMode) -> Result<String, CliError> {
    Kb::open(root)
        .map_err(io_error)?
        .pick(id, mode)
        .map_err(io_error)
}

/// `kb edit`: Ctrl-E without the UI.
pub fn run_edit(
    platform: &impl Platform,
    root: &std::path::Path,
    id: &str,
) -> Result<ExitCode, CliError> {
    let kb = Kb::open(root).map_err(io_error)?;
    let path = kb.entry_path(id).map_err(io_error)?;
    let tty = platform
        .open_tty()
        .map_err(|e| CliError::new("io", format!("no terminal to run the editor on: {e}")))?;
    let status = platform
        .run_editor(&path, &tty, &|| false)
        .map_err(|e| CliError::new("io", format!("could not start the editor: {e}")))?
        .ok_or_else(|| CliError::new("io", "the editor was interrupted"))?;
    if let Ok(findings) = kb.lint_entry(id)
        && let Some(f) = findings.first()
    {
        eprintln!("{}:{}: error[{}]: {}", f.path, f.line, f.code, f.message);
    }
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
