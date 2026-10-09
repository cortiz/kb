//! Human and JSON rendering. kb prints no color, so output is the same in a terminal, a pipe or
//! with `NO_COLOR` set.

use serde_json::json;

/// An error that ends the command with exit code 2.
#[derive(Debug)]
pub struct CliError {
    /// `usage`, `config` or `io`.
    pub kind: &'static str,
    pub message: String,
}

impl CliError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> CliError {
        CliError {
            kind,
            message: message.into(),
        }
    }
}

/// Prints `kb: <message>` to stderr and, with `--json`, an error document to stdout.
pub fn print_error(error: &CliError, json: bool) {
    eprintln!("kb: {}", error.message);
    if json {
        println!(
            "{}",
            json!({ "error": { "kind": error.kind, "message": error.message } })
        );
    }
}
