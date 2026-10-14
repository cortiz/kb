//! `kb pick` command execution.

use std::process::ExitCode;
use std::time::SystemTime;

use clap::Args;
use kb_core::config::Loaded;
use kb_core::entry::EntryType;
use kb_core::search::Kb;
use kb_core::shell::{BufferMode, ShellDialect, WidgetResult};

use crate::output::CliError;
use crate::platform::Platform;
use crate::tui::inline_picker::{self, InlinePickerOptions, InlinePickerResult};

#[derive(Args, Debug, Clone)]
pub struct PickArgs {
    /// Initial query text.
    #[arg(value_name = "QUERY")]
    pub query: Vec<String>,

    /// Filter by entry type.
    #[arg(long = "type", value_name = "TYPE")]
    pub entry_type: Option<String>,

    /// Filter by tag.
    #[arg(long = "tag", value_name = "TAG")]
    pub tag: Option<String>,

    /// Picker height (`<N>` lines or `<N>%`).
    #[arg(long, default_value = "40%")]
    pub height: String,

    /// Disable preview pane even on wide terminals.
    #[arg(long)]
    pub no_preview: bool,

    /// Output mode on pick (resolved, raw, id, payload).
    #[arg(long, default_value = "resolved")]
    pub print: String,

    /// Run inside tmux display-popup if tmux >= 3.2.
    #[arg(long)]
    pub tmux: bool,

    /// Do not run inside tmux display-popup even if --tmux is passed.
    #[arg(long)]
    pub no_tmux: bool,

    /// Internal flag for shell widgets (outputs JSON on stdout).
    #[arg(long)]
    pub widget: bool,

    /// Target shell dialect (zsh, bash, fish) when --widget is used.
    #[arg(long, value_name = "SHELL")]
    pub shell: Option<String>,

    /// Initial prompt buffer text.
    #[arg(long, value_name = "TEXT")]
    pub buffer: Option<String>,

    /// Current cursor byte offset in prompt buffer.
    #[arg(long, value_name = "N")]
    pub cursor: Option<usize>,
}

pub fn run_pick(
    platform: &impl Platform,
    root: &std::path::Path,
    loaded: &Loaded,
    args: &PickArgs,
) -> Result<ExitCode, CliError> {
    if args.tmux && !args.no_tmux && platform.is_tmux_popup_supported() {
        return run_tmux_popup(args, platform);
    }

    let mut kb = Kb::open(root).map_err(|e| CliError::new("search", e.to_string()))?;

    let dialect = args
        .shell
        .as_deref()
        .and_then(ShellDialect::from_str)
        .unwrap_or(ShellDialect::Zsh);

    let default_type = if let Some(t_str) = &args.entry_type {
        EntryType::from_name(t_str)
    } else {
        match loaded.settings.widget_default_type() {
            "all" => None,
            _ => Some(EntryType::Cmd),
        }
    };

    let buffer_mode = match loaded.settings.widget_buffer() {
        "insert" => BufferMode::Insert,
        _ => BufferMode::Query,
    };

    let prompt_buffer = args.buffer.clone().unwrap_or_default();
    let prompt_cursor = args.cursor.unwrap_or(prompt_buffer.len());

    let initial_query = if !args.query.is_empty() {
        args.query.join(" ")
    } else if buffer_mode == BufferMode::Query && !prompt_buffer.is_empty() {
        prompt_buffer.clone()
    } else {
        String::new()
    };

    let picker_options = InlinePickerOptions {
        initial_query,
        default_type,
        tag: args.tag.clone(),
        height: args.height.clone(),
        preview: !args.no_preview && loaded.settings.widget_preview(),
        dialect: Some(dialect),
    };

    let res = inline_picker::run_inline_picker(&mut kb, picker_options)
        .map_err(|e| CliError::new("tui", e.to_string()))?;

    if args.widget {
        match res {
            InlinePickerResult::Picked {
                id,
                resolved_command,
                variables,
            } => {
                if !variables.is_empty() && loaded.settings.vars_remember() {
                    let pairs: Vec<(String, String)> = variables.into_iter().collect();
                    let _ = kb.record_resolved_use(&id, SystemTime::now(), &pairs);
                } else {
                    let _ = kb.record_use(&id, SystemTime::now());
                }

                let (final_command, final_cursor) = match buffer_mode {
                    BufferMode::Insert => {
                        let at = prompt_cursor.min(prompt_buffer.len());
                        let before = &prompt_buffer[..at];
                        let after = &prompt_buffer[at..];
                        let cmd = format!("{before}{resolved_command}{after}");
                        let cur = at + resolved_command.len();
                        (cmd, cur)
                    }
                    BufferMode::Query => {
                        let cur = resolved_command.len();
                        (resolved_command, cur)
                    }
                };

                let output_payload = WidgetResult::picked(final_command, final_cursor);
                let json_str = serde_json::to_string(&output_payload)
                    .map_err(|e| CliError::new("json", e.to_string()))?;
                println!("{json_str}");
                Ok(ExitCode::SUCCESS)
            }
            InlinePickerResult::Cancelled => {
                let output_payload = WidgetResult::cancelled();
                let json_str = serde_json::to_string(&output_payload)
                    .map_err(|e| CliError::new("json", e.to_string()))?;
                println!("{json_str}");
                Ok(ExitCode::from(130))
            }
            InlinePickerResult::Refused(msg) => {
                let output_payload = WidgetResult::refused(msg);
                let json_str = serde_json::to_string(&output_payload)
                    .map_err(|e| CliError::new("json", e.to_string()))?;
                println!("{json_str}");
                Ok(ExitCode::from(1))
            }
        }
    } else {
        match res {
            InlinePickerResult::Picked {
                id,
                resolved_command,
                variables,
            } => {
                if !variables.is_empty() && loaded.settings.vars_remember() {
                    let pairs: Vec<(String, String)> = variables.into_iter().collect();
                    let _ = kb.record_resolved_use(&id, SystemTime::now(), &pairs);
                } else {
                    let _ = kb.record_use(&id, SystemTime::now());
                }

                match args.print.as_str() {
                    "id" => println!("{id}"),
                    "raw" => {
                        let entry = kb
                            .entry(&id)
                            .map_err(|e| CliError::new("entry", e.to_string()))?;
                        let raw = entry
                            .payload()
                            .map(|p| p.text.as_str())
                            .unwrap_or_else(|| entry.notes());
                        println!("{}", raw.trim_end_matches('\n'));
                    }
                    "payload" => {
                        let entry = kb
                            .entry(&id)
                            .map_err(|e| CliError::new("entry", e.to_string()))?;
                        let abs_path = root.join(entry.path());
                        match kb_core::entry_json::pick_output(
                            &entry,
                            &abs_path,
                            kb_core::entry_json::PrintMode::Payload,
                        ) {
                            Ok(out) => println!("{}", out.trim_end_matches('\n')),
                            Err(_) => {
                                let p = entry.payload().map(|p| p.text.clone()).unwrap_or_default();
                                println!("{}", p.trim_end_matches('\n'));
                            }
                        }
                    }
                    "path" => {
                        let entry = kb
                            .entry(&id)
                            .map_err(|e| CliError::new("entry", e.to_string()))?;
                        let abs_path = root.join(entry.path());
                        println!("{}", abs_path.display());
                    }
                    "json" => {
                        let entry = kb
                            .entry(&id)
                            .map_err(|e| CliError::new("entry", e.to_string()))?;
                        let abs_path = root.join(entry.path());
                        let doc = kb_core::entry_json::entry_json(&entry, &abs_path);
                        println!("{doc}");
                    }
                    _ => println!("{}", resolved_command.trim_end_matches('\n')),
                }
                Ok(ExitCode::SUCCESS)
            }
            InlinePickerResult::Cancelled => Ok(ExitCode::from(130)),
            InlinePickerResult::Refused(msg) => {
                eprintln!("{msg}");
                Ok(ExitCode::from(1))
            }
        }
    }
}

fn run_tmux_popup(args: &PickArgs, platform: &impl Platform) -> Result<ExitCode, CliError> {
    let mut child_args = vec!["pick".to_string(), "--no-tmux".to_string()];
    if let Some(t) = &args.entry_type {
        child_args.push("--type".to_string());
        child_args.push(t.clone());
    }
    if let Some(t) = &args.tag {
        child_args.push("--tag".to_string());
        child_args.push(t.clone());
    }
    child_args.push("--height".to_string());
    child_args.push(args.height.clone());
    if args.no_preview {
        child_args.push("--no-preview".to_string());
    }
    child_args.push("--print".to_string());
    child_args.push(args.print.clone());
    if !args.query.is_empty() {
        child_args.extend(args.query.clone());
    }

    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "kb".to_string());

    let quoted_args = child_args
        .iter()
        .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(" ");

    let child_cmd = format!("\"{}\" {}", exe.replace('"', "\\\""), quoted_args);

    let (code, output) = platform
        .run_tmux_popup(" kb pick ", "85%", "50%", &child_cmd)
        .map_err(|e| CliError::new("tmux", e.to_string()))?;

    let code = code.unwrap_or(0);
    if code == 0 {
        print!("{output}");
        return Ok(ExitCode::SUCCESS);
    }
    if code == 130 {
        return Ok(ExitCode::from(130));
    }
    Ok(ExitCode::from(code as u8))
}
