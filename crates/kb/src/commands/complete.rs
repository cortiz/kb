//! `kb __complete` command execution.

use std::process::ExitCode;

use clap::Args;
use kb_core::shell::completions::{
    complete_entries, complete_tags, complete_types, complete_vars, complete_var_values,
    CompletionKind,
};

use crate::output::CliError;
use crate::platform::Platform;

#[derive(Args, Debug, Clone)]
pub struct CompleteArgs {
    /// Completion kind: entries, tags, types, vars, var-values.
    pub kind: String,

    /// Arguments for the completion kind.
    #[arg(trailing_var_arg = true)]
    pub args: Vec<String>,
}

pub fn run_complete(
    _platform: &impl Platform,
    root: &std::path::Path,
    args: &CompleteArgs,
) -> Result<ExitCode, CliError> {
    let kind = match CompletionKind::parse(&args.kind) {
        Some(k) => k,
        None => return Ok(ExitCode::SUCCESS),
    };

    match kind {
        CompletionKind::Entries => {
            let prefix = args.args.first().map(|s| s.as_str());
            let entries = complete_entries(root, prefix);
            for (id, title) in entries {
                println!("{id}\t{title}");
            }
        }
        CompletionKind::Tags => {
            let prefix = args.args.first().map(|s| s.as_str());
            let tags = complete_tags(root, prefix);
            for tag in tags {
                println!("{tag}");
            }
        }
        CompletionKind::Types => {
            let prefix = args.args.first().map(|s| s.as_str());
            let types = complete_types(prefix);
            for t in types {
                println!("{t}");
            }
        }
        CompletionKind::Vars => {
            if let Some(id) = args.args.first() {
                let prefix = args.args.get(1).map(|s| s.as_str());
                let vars = complete_vars(root, id, prefix);
                for v in vars {
                    println!("{v}");
                }
            }
        }
        CompletionKind::VarValues => {
            if let (Some(id), Some(name)) = (args.args.first(), args.args.get(1)) {
                let prefix = args.args.get(2).map(|s| s.as_str());
                let values = complete_var_values(root, id, name, prefix);
                for val in values {
                    println!("{val}");
                }
            }
        }
    }

    Ok(ExitCode::SUCCESS)
}
