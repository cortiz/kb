//! FR-003: nothing outside `crates/kb/src/platform/` may name an OS-specific program, read an
//! OS-specific environment variable, or branch on the operating system. Comments and doc strings
//! are ignored; string literals are not. Test code (`#[cfg(test)]` modules, which sit at the end
//! of a file, and the `tests/` directories) is exempt: a test has to name the fake it builds.

use std::fs;
use std::path::{Path, PathBuf};

/// String literals that name a clipboard or opener program.
const TOOL_LITERALS: &[&str] = &[
    "\"pbcopy\"",
    "\"pbpaste\"",
    "\"wl-copy\"",
    "\"xclip\"",
    "\"xsel\"",
    "\"xdg-open\"",
    "\"osascript\"",
];

/// Environment variables that describe the machine or terminal.
const ENV_LITERALS: &[&str] = &[
    "WAYLAND_DISPLAY",
    "\"DISPLAY\"",
    "SSH_CONNECTION",
    "SSH_TTY",
    "SSH_CLIENT",
    "\"TMUX\"",
    "\"STY\"",
    "COLORTERM",
    "XDG_",
    "TERM_PROGRAM",
];

const OTHER: &[&str] = &["Command::new(\"open\")", "cfg(target_os"];

#[derive(Debug, PartialEq, Eq)]
struct Violation {
    file: PathBuf,
    line: usize,
    what: &'static str,
}

/// `text` with every comment replaced by spaces (newlines kept), so line numbers still match.
/// String and char literals are skipped over, so `"https://x"` is not mistaken for a comment.
fn strip_comments(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < b.len() {
                if b[i] == '/' && b.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(blank(b[i]));
                    i += 1;
                }
            }
        } else if c == 'r' && matches!(next, Some('"' | '#')) && is_raw_string_start(&b, i) {
            let mut hashes = 0;
            let mut j = i + 1;
            while b.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            // b[j] is the opening quote
            out.extend(&b[i..=j]);
            j += 1;
            while j < b.len() {
                if b[j] == '"' && (0..hashes).all(|k| b.get(j + 1 + k) == Some(&'#')) {
                    out.extend(&b[j..=j + hashes]);
                    j += hashes + 1;
                    break;
                }
                out.push(b[j]);
                j += 1;
            }
            i = j;
        } else if c == '"' {
            out.push(c);
            i += 1;
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' && i + 1 < b.len() {
                    out.push(b[i]);
                    i += 1;
                }
                out.push(b[i]);
                i += 1;
            }
            if i < b.len() {
                out.push('"');
                i += 1;
            }
        } else if c == '\'' && is_char_literal(&b, i) {
            let end = if next == Some('\\') {
                b[i + 2..]
                    .iter()
                    .position(|&x| x == '\'')
                    .map_or(i + 1, |p| i + 2 + p)
            } else {
                i + 2
            };
            out.extend(&b[i..=end.min(b.len() - 1)]);
            i = end + 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn is_raw_string_start(b: &[char], i: usize) -> bool {
    // `r"`, `r#"`, `r##"` ... and not the end of an identifier such as `bar"`
    if i > 0 && (b[i - 1].is_alphanumeric() || b[i - 1] == '_') {
        return false;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&'#') {
        j += 1;
    }
    b.get(j) == Some(&'"')
}

fn is_char_literal(b: &[char], i: usize) -> bool {
    match (b.get(i + 1), b.get(i + 2)) {
        (Some('\\'), _) => true,
        (Some(_), Some('\'')) => true,
        _ => false, // a lifetime such as 'a
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every rule broken under `workspace/crates/*/src`, except inside `crates/kb/src/platform/`.
fn scan(workspace: &Path) -> Vec<Violation> {
    let allowed = workspace.join("crates/kb/src/platform");
    let mut files = Vec::new();
    if let Ok(crates) = fs::read_dir(workspace.join("crates")) {
        for c in crates.flatten() {
            rust_files(&c.path().join("src"), &mut files);
        }
    }
    files.sort();
    let mut found = Vec::new();
    for file in files.into_iter().filter(|f| !f.starts_with(&allowed)) {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let code = strip_comments(&text);
        // unit tests sit in a `#[cfg(test)]` module at the end of the file
        let code = code.split("#[cfg(test)]").next().unwrap_or_default();
        for (n, line) in code.lines().enumerate() {
            for what in TOOL_LITERALS.iter().chain(ENV_LITERALS).chain(OTHER) {
                if line.contains(what) {
                    found.push(Violation {
                        file: file.strip_prefix(workspace).unwrap_or(&file).to_path_buf(),
                        line: n + 1,
                        what,
                    });
                }
            }
        }
    }
    found
}

#[test]
fn nothing_outside_the_platform_layer_is_os_specific() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let found = scan(&root);
    assert!(
        found.is_empty(),
        "OS-specific code outside crates/kb/src/platform/:\n{}",
        found
            .iter()
            .map(|v| format!("  {}:{}: {}", v.file.display(), v.line, v.what))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_checker_finds_violations_and_ignores_comments_and_the_platform_layer() {
    let dir = tempfile::tempdir().unwrap();
    let write = |rel: &str, text: &str| {
        let p = dir.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    };
    write(
        "crates/a/src/bad.rs",
        "fn f() {\n    let _ = std::process::Command::new(\"xclip\");\n    let _ = std::env::var(\"SSH_TTY\");\n    #[cfg(target_os = \"linux\")]\n    fn g() {}\n    let _ = Command::new(\"open\");\n}\n",
    );
    write(
        "crates/a/src/fine.rs",
        "// pbcopy is mentioned in a comment\n/// and in \"xclip\" a doc string\n/* WAYLAND_DISPLAY in a block comment */\nfn f() {\n    let url = \"https://example.com/xclip\";\n    let c = '\"';\n    let _ = (url, c, \"xclip -selection\");\n}\n",
    );
    write(
        "crates/kb/src/platform/mac.rs",
        "fn f() { let _ = std::process::Command::new(\"pbcopy\"); }\n#[cfg(target_os = \"macos\")]\nfn g() {}\n",
    );
    write(
        "crates/kb/src/other.rs",
        "fn f() { let _ = std::env::var(\"TMUX\"); }\n",
    );
    let found = scan(dir.path());
    let summary: Vec<(String, usize, &str)> = found
        .iter()
        .map(|v| (v.file.display().to_string(), v.line, v.what))
        .collect();
    assert_eq!(
        summary,
        [
            ("crates/a/src/bad.rs".to_string(), 2, "\"xclip\""),
            ("crates/a/src/bad.rs".to_string(), 3, "SSH_TTY"),
            ("crates/a/src/bad.rs".to_string(), 4, "cfg(target_os"),
            (
                "crates/a/src/bad.rs".to_string(),
                6,
                "Command::new(\"open\")"
            ),
            ("crates/kb/src/other.rs".to_string(), 1, "\"TMUX\""),
        ]
    );
}

#[test]
fn comments_are_blanked_but_line_numbers_survive() {
    let s = strip_comments("a // x\n/* y\nz */ b\n\"//not\" r#\"/*not\"# c\n");
    assert_eq!(s.lines().count(), 4);
    assert!(s.contains("\"//not\"") && s.contains("r#\"/*not\"#"));
    assert!(!s.contains('x') && !s.contains('y') && !s.contains('z'));
}
