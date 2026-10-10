//! FR-004: `kb_core::varres` is the only place that replaces `{{name}}` with a value. Outside it,
//! code may scan a template to *display* it (lint, the preview) but must not build text from it.
//! The scanner entry points and the brace literal may appear only in the files listed here; a new
//! file that needs them has to be added on purpose, with a reason.

use std::fs;
use std::path::{Path, PathBuf};

/// Files that may name the scanners or the brace literal, and why.
const ALLOWED: &[(&str, &str)] = &[
    (
        "kb-core/src/vars.rs",
        "declares the scanner; lint counts references with it",
    ),
    (
        "kb-core/src/payload.rs",
        "splits a payload for display (the preview marks variables)",
    ),
    (
        "kb-core/src/entry.rs",
        "checks a url that contains references",
    ),
    (
        "kb-core/src/body.rs",
        "turns the escaped brace into the literal one when reading a payload",
    ),
    (
        "kb/src/tui/highlight.rs",
        "draws a variable marker in the preview",
    ),
    ("kb/src/tui/preview.rs", "marks variables in the preview"),
    (
        "kb-core/src/author/draft.rs",
        "lists the references of a new entry's body to declare them, and escapes one as a literal; never substitutes a value",
    ),
    (
        "kb/src/tui/add.rs",
        "colors the body of a draft in the add form",
    ),
    (
        "kb/src/tui/add_view.rs",
        "draws the variables of a draft as {{name}}",
    ),
    (
        "kb-core/src/author/suggest.rs",
        "turns a literal value into a reference (the opposite of resolving) and skips existing references",
    ),
];

const NEEDLES: &[&str] = &[
    "scan_refs(",
    "payload_pieces(",
    "\"{{",
    "\"}}\"",
    "unescape(",
];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The part of a file before its unit tests (which sit at the end), without comment-only lines.
fn production_code(text: &str) -> String {
    let head = text.split("#[cfg(test)]").next().unwrap_or(text);
    head.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn nothing_but_varres_replaces_variables() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    sources(&crates.join("kb/src"), &mut files);
    sources(&crates.join("kb-core/src"), &mut files);
    let mut violations = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(&crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("kb-core/src/varres/") || ALLOWED.iter().any(|(a, _)| *a == rel) {
            continue;
        }
        let code = production_code(&fs::read_to_string(&f).unwrap());
        for needle in NEEDLES {
            if code.contains(needle) {
                violations.push(format!("{rel} uses {needle}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "variables must be replaced only in kb_core::varres:\n{}",
        violations.join("\n")
    );
}
