//! `kb keys`, `kb keys check` and `kb keys defaults`: the effective key bindings, and whether
//! the `[tui.keys.*]` config is sound.

use std::process::ExitCode;

use clap::{Args, Subcommand};
use kb_core::config::Loaded;
use kb_core::keys::{Command, Context, EffectiveKeymap, Keymap, Origin, rows, valid};
use serde_json::json;

use crate::output::CliError;

#[derive(Args, Debug, Clone)]
pub struct KeysArgs {
    #[command(subcommand)]
    pub sub: Option<KeysCommand>,
    /// Show one context only: search, normal, action-menu, code-view, history, form, add or help.
    #[arg(long, value_name = "NAME")]
    pub context: Option<String>,
    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum KeysCommand {
    /// Check the key bindings in the config file without opening the UI (exit 1 on any problem).
    Check {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print a commented `[tui.keys.*]` block with every command and its default keys. Use
    /// `--keymap vim` to see the vim defaults.
    Defaults,
}

impl KeysArgs {
    pub fn json(&self) -> bool {
        self.json || matches!(self.sub, Some(KeysCommand::Check { json: true }))
    }
}

/// The keymap the UI runs with: `--keymap` / `KB_TUI_KEYMAP` / `tui.keymap`, plus the overrides.
pub fn effective(loaded: &Loaded) -> EffectiveKeymap {
    EffectiveKeymap::build(loaded.settings.keymap(), loaded.settings.key_overrides())
}

/// Every problem with the key config: the overrides that were ignored, and `tui.keys` /
/// `tui.keymap` lines the config loader could not read.
pub fn problems(loaded: &Loaded, map: &EffectiveKeymap) -> Vec<String> {
    let mut out: Vec<String> = loaded
        .warnings
        .iter()
        .filter(|w| w.message.starts_with("tui.key"))
        .map(|w| w.to_string())
        .collect();
    out.extend(map.warnings.iter().map(|w| w.to_string()));
    out
}

const SHOWN: [Context; 8] = [
    Context::Search,
    Context::Normal,
    Context::ActionMenu,
    Context::CodeView,
    Context::History,
    Context::Form,
    Context::Add,
    Context::Help,
];

pub fn run_keys(loaded: &Loaded, args: &KeysArgs) -> Result<ExitCode, CliError> {
    let map = effective(loaded);
    match &args.sub {
        Some(KeysCommand::Check { json }) => Ok(check(loaded, &map, *json)),
        Some(KeysCommand::Defaults) => {
            print!("{}", defaults(map.keymap));
            Ok(ExitCode::SUCCESS)
        }
        None => list(loaded, &map, args),
    }
}

fn contexts(map: &EffectiveKeymap, only: Option<&str>) -> Result<Vec<Context>, CliError> {
    let usable: Vec<Context> = SHOWN
        .into_iter()
        .filter(|c| *c != Context::Normal || map.keymap == Keymap::Vim)
        .collect();
    let Some(name) = only else {
        return Ok(usable);
    };
    match Context::from_name(name) {
        Some(c) if usable.contains(&c) => Ok(vec![c]),
        _ => Err(CliError::new(
            "usage",
            format!(
                "no keys for context {name:?} (accepted: {})",
                usable
                    .iter()
                    .map(|c| c.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

fn source_text(b: &kb_core::keys::Binding) -> String {
    match b.origin {
        Origin::Preset => "[preset]".into(),
        Origin::Config => {
            let file = b.file.as_ref().map(|p| p.display().to_string());
            match (file, b.line) {
                (Some(f), Some(l)) => format!("[config: {f}:{l}]"),
                (Some(f), None) => format!("[config: {f}]"),
                _ => "[config]".into(),
            }
        }
    }
}

fn list(loaded: &Loaded, map: &EffectiveKeymap, args: &KeysArgs) -> Result<ExitCode, CliError> {
    let contexts = contexts(map, args.context.as_deref())?;
    let warnings = problems(loaded, map);
    if args.json {
        let mut ctxs = serde_json::Map::new();
        for ctx in &contexts {
            let rows: Vec<_> = map
                .context(*ctx)
                .map(|(cmd, b)| {
                    json!({
                        "command": cmd.name(),
                        "keys": b.keys.iter().map(|k| k.name()).collect::<Vec<_>>(),
                        "origin": match b.origin { Origin::Preset => "preset", Origin::Config => "config" },
                        "source": match (&b.file, b.line) {
                            (Some(f), line) => json!({ "file": f.display().to_string(), "line": line }),
                            _ => serde_json::Value::Null,
                        },
                    })
                })
                .collect();
            ctxs.insert(ctx.name().into(), rows.into());
        }
        let doc = json!({ "keymap": map.keymap.name(), "contexts": ctxs, "warnings": warnings });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
        return Ok(ExitCode::SUCCESS);
    }
    for ctx in contexts {
        println!("Context: {} (keymap: {})", ctx.name(), map.keymap.name());
        for (cmd, b) in map.context(ctx) {
            let keys = if b.keys.is_empty() {
                "(none)".to_string()
            } else {
                b.keys
                    .iter()
                    .map(|k| k.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            println!("  {keys:<30} {:<26} {}", cmd.name(), source_text(b));
        }
        println!();
    }
    for w in warnings {
        eprintln!("kb: {w}");
    }
    Ok(ExitCode::SUCCESS)
}

fn check(loaded: &Loaded, map: &EffectiveKeymap, json: bool) -> ExitCode {
    let problems = problems(loaded, map);
    if json {
        let warnings: Vec<_> = map
            .warnings
            .iter()
            .map(|w| {
                json!({
                    "file": w.file.as_ref().map(|p| p.display().to_string()),
                    "line": w.line,
                    "context": w.context,
                    "message": w.message,
                })
            })
            .chain(
                loaded
                    .warnings
                    .iter()
                    .filter(|w| w.message.starts_with("tui.key"))
                    .map(|w| {
                        json!({
                            "file": w.path.as_ref().map(|p| p.display().to_string()),
                            "line": w.line,
                            "context": serde_json::Value::Null,
                            "message": w.message,
                        })
                    }),
            )
            .collect();
        let doc = json!({
            "ok": problems.is_empty(),
            "problems_count": problems.len(),
            "warnings": warnings,
        });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
    } else if problems.is_empty() {
        println!(
            "Key configuration valid (preset: {}, {} override{} active, 0 warnings).",
            map.keymap.name(),
            map.active_overrides,
            if map.active_overrides == 1 { "" } else { "s" }
        );
    } else {
        for p in &problems {
            println!("{p}");
        }
        println!(
            "Validation failed: {} problem{} found.",
            problems.len(),
            if problems.len() == 1 { "" } else { "s" }
        );
    }
    if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// A commented `[tui.keys.*]` block with every command and its default keys, ready to copy.
pub fn defaults(keymap: Keymap) -> String {
    let preset = EffectiveKeymap::preset(keymap);
    let mut out = format!(
        "# Default key bindings for kb (keymap = \"{}\")\n\
         # Copy sections below to your config file to customize. A list replaces the command's\n\
         # keys in that context; an empty list unbinds it. [tui.keys.global] sets a command in\n\
         # both list modes (search and normal) at once.\n",
        keymap.name()
    );
    for ctx in SHOWN {
        if ctx == Context::Normal && keymap != Keymap::Vim {
            continue;
        }
        let mut lines: Vec<(Command, Vec<String>)> = rows(keymap, ctx)
            .into_iter()
            .map(|(c, _)| {
                (
                    c,
                    preset
                        .keys(ctx, c)
                        .iter()
                        .map(|k| format!("{:?}", k.name()))
                        .collect(),
                )
            })
            .collect();
        if ctx.is_list() {
            for c in Command::ALL {
                if valid(keymap, ctx, *c) && !lines.iter().any(|(x, _)| x == c) {
                    lines.push((*c, Vec::new()));
                }
            }
        }
        out.push_str(&format!("\n# [tui.keys.{}]\n", ctx.name()));
        for (c, keys) in lines {
            out.push_str(&format!("# {} = [{}]\n", c.name(), keys.join(", ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_block_is_valid_config_when_uncommented() {
        for keymap in [Keymap::Emacs, Keymap::Vim] {
            let text: String = defaults(keymap)
                .lines()
                .map(|l| {
                    l.strip_prefix("# ")
                        .filter(|r| r.starts_with('[') || r.contains(" = ["))
                        .unwrap_or("")
                })
                .map(|l| format!("{l}\n"))
                .collect::<String>();
            let d = tempfile::tempdir().unwrap();
            let p = d.path().join("config.toml");
            std::fs::write(
                &p,
                format!("[tui]\nkeymap = \"{}\"\n\n{text}", keymap.name()),
            )
            .unwrap();
            let loaded = kb_core::config::load(&[p], &|_| None, &kb_core::config::Flags::default());
            let map = effective(&loaded);
            assert_eq!(
                problems(&loaded, &map),
                Vec::<String>::new(),
                "{keymap:?}\n{text}"
            );
            // and it changes nothing
            let preset = EffectiveKeymap::preset(keymap);
            for ctx in SHOWN {
                for (cmd, b) in preset.context(ctx) {
                    assert_eq!(map.keys(ctx, cmd), b.keys.as_slice(), "{ctx:?} {cmd:?}");
                }
            }
        }
    }
}
