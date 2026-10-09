//! Human and JSON rendering. kb prints no color, so output is the same in a terminal, a pipe or
//! with `NO_COLOR` set.

use kb_core::lint::LintReport;
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

/// Human lint output: one block per finding, one line per suppression, then the summary.
pub fn print_lint(report: &LintReport, quiet: bool) {
    let mut out = String::new();
    if !quiet {
        for f in &report.findings {
            let severity = match f.severity {
                kb_core::lint::Severity::Error => "error",
                kb_core::lint::Severity::Warning => "warning",
                kb_core::lint::Severity::Info => "info",
            };
            out.push_str(&format!(
                "{}:{}:{}: {severity}[{}]: {}\n",
                f.path, f.line, f.column, f.code, f.message
            ));
            if let Some(fix) = &f.fix {
                out.push_str(&format!("  fix: {fix}\n"));
            }
        }
        for s in &report.suppressions {
            out.push_str(&format!("{}:{}: suppressed[{}]\n", s.path, s.line, s.code));
        }
        if !out.is_empty() {
            out.push('\n');
        }
    }
    out.push_str(&report.summary_line());
    println!("{out}");
}
