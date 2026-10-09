//! Quoting for the place a value lands: a shell command, an SQL string, a URL.

use crate::entry::Shell;

/// Characters that need no quoting in a shell word.
fn is_safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c)
}

/// Where the template is, at a given point, with respect to shell quoting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quote {
    #[default]
    None,
    Single,
    Double,
}

/// Follows quotes through the literal text of a shell template. A value is never fed to it, so
/// a value cannot change what comes after it.
// ponytail: no heredocs, comments, `$'...'`, `$(...)` nesting or backticks. Text inside those is
// treated by the plain quote rules, which still quote conservatively; `escape:` pins the choice.
pub fn advance(mut state: Quote, literal: &str) -> Quote {
    let mut chars = literal.chars();
    while let Some(c) = chars.next() {
        match (state, c) {
            (Quote::None, '\\') | (Quote::Double, '\\') => {
                chars.next();
            }
            (Quote::None, '\'') => state = Quote::Single,
            (Quote::None, '"') => state = Quote::Double,
            (Quote::Single, '\'') | (Quote::Double, '"') => state = Quote::None,
            _ => {}
        }
    }
    state
}

/// The value as one shell word: unchanged when it is plainly safe, else single-quoted.
pub fn unquoted(value: &str, shell: Shell) -> String {
    if !value.is_empty() && value.chars().all(is_safe) {
        return value.to_string();
    }
    format!("'{}'", in_single(value, shell))
}

/// The value escaped for the inside of `'...'`.
pub fn in_single(value: &str, shell: Shell) -> String {
    match shell {
        Shell::Fish => value.replace('\\', "\\\\").replace('\'', "\\'"),
        _ => value.replace('\'', "'\\''"),
    }
}

/// The value escaped for the inside of `"..."`.
pub fn in_double(value: &str, shell: Shell) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        let special = match shell {
            Shell::Fish => matches!(c, '\\' | '"' | '$'),
            _ => matches!(c, '\\' | '"' | '$' | '`'),
        };
        if special {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A single-quoted SQL string literal.
pub fn sql(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Percent-encodes every UTF-8 byte outside `A-Za-z0-9-._~`.
pub fn percent(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SH: Shell = Shell::Sh;

    #[test]
    fn plain_values_stay_plain() {
        for v in ["abc", "a-b.c/d", "x=1", "a,b:c", "feature_x", "-rf", "é"] {
            let want = if v == "é" { "'é'" } else { v };
            assert_eq!(unquoted(v, SH), want, "{v}");
        }
        assert_eq!(unquoted("", SH), "''");
    }

    #[test]
    fn unquoted_table_sh() {
        let cases = [
            ("a b", "'a b'"),
            ("a'b", "'a'\\''b'"),
            ("a\"b", "'a\"b'"),
            ("a`b`", "'a`b`'"),
            ("$(id)", "'$(id)'"),
            ("${HOME}", "'${HOME}'"),
            ("a;b", "'a;b'"),
            ("a&&b", "'a&&b'"),
            ("a|b", "'a|b'"),
            ("a>b", "'a>b'"),
            ("*", "'*'"),
            ("~", "'~'"),
            ("\\", "'\\'"),
            ("''", "''\\'''\\'''"),
            ("{{x}}", "'{{x}}'"),
            ("日本", "'日本'"),
            ("feature/x; rm -rf ~", "'feature/x; rm -rf ~'"),
            ("a&b c", "'a&b c'"),
        ];
        for (v, want) in cases {
            for shell in [Shell::Sh, Shell::Bash, Shell::Zsh] {
                assert_eq!(unquoted(v, shell), want, "{v} for {shell:?}");
            }
        }
    }

    #[test]
    fn unquoted_table_fish() {
        let cases = [
            ("a b", "'a b'"),
            ("a'b", "'a\\'b'"),
            ("a\\b", "'a\\\\b'"),
            ("$(id)", "'$(id)'"),
            ("", "''"),
            ("abc", "abc"),
        ];
        for (v, want) in cases {
            assert_eq!(unquoted(v, Shell::Fish), want, "{v}");
        }
    }

    #[test]
    fn inside_quotes() {
        assert_eq!(in_single("a'b", SH), "a'\\''b");
        assert_eq!(in_single("a'b\\c", Shell::Fish), "a\\'b\\\\c");
        assert_eq!(in_double("a\"b$c`d\\e", Shell::Bash), "a\\\"b\\$c\\`d\\\\e");
        assert_eq!(in_double("a`b", Shell::Fish), "a`b");
        assert_eq!(in_double("a b", SH), "a b");
    }

    #[test]
    fn sql_and_percent() {
        assert_eq!(sql("it's"), "'it''s'");
        assert_eq!(sql(""), "''");
        assert_eq!(percent("a&b c"), "a%26b%20c");
        assert_eq!(percent("é/~-._"), "%C3%A9%2F~-._");
        assert_eq!(percent(""), "");
    }

    #[test]
    fn the_tracker_follows_quotes() {
        assert_eq!(advance(Quote::None, "echo "), Quote::None);
        assert_eq!(advance(Quote::None, "grep \""), Quote::Double);
        assert_eq!(advance(Quote::None, "echo '"), Quote::Single);
        assert_eq!(advance(Quote::None, "echo 'a' \"b\" "), Quote::None);
        assert_eq!(advance(Quote::None, "echo \\\""), Quote::None);
        assert_eq!(advance(Quote::Single, "a\\"), Quote::Single);
        assert_eq!(advance(Quote::Double, "a\\\" b"), Quote::Double);
        assert_eq!(advance(Quote::Double, "\" x"), Quote::None);
        assert_eq!(advance(Quote::Single, "\" x"), Quote::Single);
    }
}
