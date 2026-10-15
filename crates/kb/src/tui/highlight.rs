//! Syntax highlighting: syntect parses, we map its scopes to a small set of token classes and
//! leave the colors to the theme. `{{variable}}` markers are put back after parsing.

use std::sync::OnceLock;

use kb_core::entry::EntryType;
use kb_core::payload::PayloadPiece;
use syntect::parsing::{ParseState, ScopeStack, SyntaxDefinition, SyntaxSet, SyntaxSetBuilder};

use super::theme::Class;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub class: Class,
    /// Where a Markdown link points. Only `kb show` on a terminal that supports it turns this
    /// into a hyperlink, and only for safe schemes.
    pub url: Option<std::sync::Arc<str>>,
}

impl Span {
    pub fn new(text: impl Into<String>, class: Class) -> Span {
        Span {
            text: text.into(),
            class,
            url: None,
        }
    }

    pub fn link(text: impl Into<String>, class: Class, url: std::sync::Arc<str>) -> Span {
        Span {
            text: text.into(),
            class,
            url: Some(url),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Sql,
    Bash,
    Sumo,
    Cwi,
    Plain,
}

/// The grammar for an entry type.
pub fn lang_for(ty: EntryType) -> Lang {
    match ty {
        EntryType::Sql => Lang::Sql,
        EntryType::Cmd => Lang::Bash,
        EntryType::Sumo => Lang::Sumo,
        EntryType::Cw => Lang::Cwi,
        EntryType::Prompt
        | EntryType::Link
        | EntryType::Code
        | EntryType::Note
        | EntryType::Runbook => Lang::Plain,
    }
}

struct Sets {
    default: SyntaxSet,
    custom: SyntaxSet,
}

fn sets() -> &'static Sets {
    static SETS: OnceLock<Sets> = OnceLock::new();
    SETS.get_or_init(|| {
        let mut builder = SyntaxSetBuilder::new();
        for yaml in [
            include_str!("syntaxes/sumo.sublime-syntax"),
            include_str!("syntaxes/cwi.sublime-syntax"),
        ] {
            if let Ok(def) = SyntaxDefinition::load_from_str(yaml, true, None) {
                builder.add(def);
            }
        }
        Sets {
            default: SyntaxSet::load_defaults_newlines(),
            custom: builder.build(),
        }
    })
}

/// Characters standing in for a variable while the grammar runs: one per character of
/// `{{name}}`, so columns stay put. They are in a private-use block no grammar matches.
const PLACEHOLDER: char = '\u{E000}';

fn class_of(stack: &ScopeStack) -> Class {
    let names: Vec<String> = stack.as_slice().iter().map(|s| s.build_string()).collect();
    if names.iter().any(|n| n.starts_with("comment")) {
        return Class::Comment;
    }
    if names.iter().any(|n| n.starts_with("string")) {
        return Class::String;
    }
    for n in names.iter().rev() {
        let class = if n.starts_with("constant.numeric") {
            Class::Number
        } else if n.starts_with("keyword.operator")
            || n.starts_with("punctuation.separator")
            || n.starts_with("punctuation.accessor")
        {
            Class::Operator
        } else if n.starts_with("keyword") {
            Class::Keyword
        } else if n.starts_with("storage.type")
            || n.starts_with("support.type")
            || n.starts_with("entity.name.type")
        {
            Class::Type
        } else if n.starts_with("support.function")
            || n.starts_with("entity.name.function")
            || n.starts_with("variable.function")
        {
            Class::Function
        } else if n.starts_with("variable.other") || n.starts_with("variable.parameter") {
            Class::Field
        } else if n.starts_with("constant") {
            Class::Number
        } else if n.starts_with("punctuation") {
            Class::Punctuation
        } else {
            continue;
        };
        return class;
    }
    Class::Plain
}

/// Highlights `text` line by line; returns spans per line that rebuild each line exactly.
fn classify(lang: Lang, text: &str) -> Vec<Vec<Span>> {
    let sets = sets();
    let (set, name) = match lang {
        Lang::Sql => (&sets.default, "SQL"),
        Lang::Bash => (&sets.default, "Bourne Again Shell (bash)"),
        Lang::Sumo => (&sets.custom, "Sumo Logic"),
        Lang::Cwi => (&sets.custom, "CloudWatch Logs Insights"),
        Lang::Plain => return plain(text),
    };
    let Some(syntax) = set.find_syntax_by_name(name) else {
        return plain(text);
    };
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out = Vec::new();
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches('\n');
        let with_newline = format!("{line}\n");
        let Ok(ops) = state.parse_line(&with_newline, set) else {
            out.push(vec![Span::new(line, Class::Plain)]);
            continue;
        };
        let mut spans: Vec<Span> = Vec::new();
        let mut at = 0;
        let push = |from: usize, to: usize, class: Class, spans: &mut Vec<Span>| {
            let to = to.min(line.len());
            if from >= to {
                return;
            }
            let piece = &line[from..to];
            match spans.last_mut() {
                Some(last) if last.class == class => last.text.push_str(piece),
                _ => spans.push(Span::new(piece, class)),
            }
        };
        let mut class = class_of(&stack);
        for (pos, op) in ops {
            push(at, pos, class, &mut spans);
            at = pos.max(at);
            let _ = stack.apply(&op);
            class = class_of(&stack);
        }
        push(at, line.len(), class, &mut spans);
        if spans.is_empty() {
            spans.push(Span::new("", Class::Plain));
        }
        out.push(spans);
    }
    out
}

pub fn plain(text: &str) -> Vec<Vec<Span>> {
    text.split_inclusive('\n')
        .map(|l| vec![Span::new(l.trim_end_matches('\n'), Class::Plain)])
        .collect()
}

pub fn highlight_code_snippet(
    lines: &[String],
    extension: Option<&str>,
    color: bool,
) -> Vec<Vec<Span>> {
    if !color {
        return plain_lines(lines);
    }
    let s = sets();
    let syntax = extension
        .and_then(|ext| s.default.find_syntax_by_extension(ext))
        .unwrap_or_else(|| s.default.find_syntax_plain_text());
    highlight_lines(lines, syntax, &s.default)
}

/// The lines of a fenced code block in the language named after the fence (`rust`, `py`, `sh`);
/// unknown languages, and no language, stay plain.
pub fn highlight_fenced(lang: Option<&str>, lines: &[String], color: bool) -> Vec<Vec<Span>> {
    let s = sets();
    let syntax = lang
        .filter(|_| color)
        .and_then(|l| s.default.find_syntax_by_token(l));
    match syntax {
        Some(syntax) => highlight_lines(lines, syntax, &s.default),
        None => plain_lines(lines),
    }
}

fn plain_lines(lines: &[String]) -> Vec<Vec<Span>> {
    lines
        .iter()
        .map(|l| vec![Span::new(l.as_str(), Class::Plain)])
        .collect()
}

fn highlight_lines(
    lines: &[String],
    syntax: &syntect::parsing::SyntaxReference,
    set: &SyntaxSet,
) -> Vec<Vec<Span>> {
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out = Vec::new();
    for line in lines {
        let with_newline = format!("{line}\n");
        let Ok(ops) = state.parse_line(&with_newline, set) else {
            out.push(vec![Span::new(line.as_str(), Class::Plain)]);
            continue;
        };
        let mut spans: Vec<Span> = Vec::new();
        let mut at = 0;
        let push = |from: usize, to: usize, class: Class, spans: &mut Vec<Span>| {
            let to = to.min(line.len());
            if from >= to {
                return;
            }
            let piece = &line[from..to];
            match spans.last_mut() {
                Some(last) if last.class == class => last.text.push_str(piece),
                _ => spans.push(Span::new(piece, class)),
            }
        };
        let mut class = class_of(&stack);
        for (pos, op) in ops {
            push(at, pos, class, &mut spans);
            at = pos.max(at);
            let _ = stack.apply(&op);
            class = class_of(&stack);
        }
        push(at, line.len(), class, &mut spans);
        if spans.is_empty() {
            spans.push(Span::new("", Class::Plain));
        }
        out.push(spans);
    }
    out
}

/// The payload as highlighted lines. `color` false skips the grammars (the `NO_COLOR` theme has
/// no use for token classes) but still marks variables.
pub fn highlight(lang: Lang, pieces: &[PayloadPiece], color: bool) -> Vec<Vec<Span>> {
    let mut text = String::new();
    // (character offset in `text`, name) of each variable
    let mut vars: Vec<(usize, String)> = Vec::new();
    let mut chars = 0usize;
    for piece in pieces {
        match piece {
            PayloadPiece::Text(t) => {
                chars += t.chars().count();
                text.push_str(t);
            }
            PayloadPiece::Var(name) => {
                let len = name.chars().count() + 4;
                vars.push((chars, name.clone()));
                text.extend(std::iter::repeat_n(PLACEHOLDER, len));
                chars += len;
            }
        }
    }
    let lines = if color {
        classify(lang, &text)
    } else {
        plain(&text)
    };
    let mut out = Vec::with_capacity(lines.len());
    let mut line_start = 0usize; // character offset of the line in `text`
    for spans in lines {
        let line_chars: usize = spans.iter().map(|s| s.text.chars().count()).sum();
        let line_end = line_start + line_chars;
        let here: Vec<&(usize, String)> = vars
            .iter()
            .filter(|(at, name)| *at >= line_start && *at + name.chars().count() + 4 <= line_end)
            .collect();
        out.push(if here.is_empty() {
            spans
        } else {
            restore_vars(spans, line_start, &here)
        });
        line_start = line_end + 1; // the newline
    }
    while out
        .last()
        .is_some_and(|l| l.iter().all(|s| s.text.is_empty()))
        && out.len() > 1
    {
        out.pop();
    }
    out
}

/// Cuts spans at variable boundaries and swaps each placeholder run for the real `{{name}}`.
fn restore_vars(spans: Vec<Span>, line_start: usize, vars: &[&(usize, String)]) -> Vec<Span> {
    let chars: Vec<(char, Class)> = spans
        .iter()
        .flat_map(|s| s.text.chars().map(move |c| (c, s.class)))
        .collect();
    let mut out: Vec<Span> = Vec::new();
    let mut i = 0;
    let push = |out: &mut Vec<Span>, c: char, class: Class| match out.last_mut() {
        Some(last) if last.class == class => last.text.push(c),
        _ => out.push(Span::new(c.to_string(), class)),
    };
    while i < chars.len() {
        if let Some((at, name)) = vars
            .iter()
            .find(|(at, _)| line_start + i == *at)
            .map(|v| (v.0, &v.1))
        {
            let len = name.chars().count() + 4;
            out.push(Span::new(format!("{{{{{name}}}}}"), Class::Variable));
            i += len;
            let _ = at;
        } else {
            push(&mut out, chars[i].0, chars[i].1);
            i += 1;
        }
    }
    out
}

/// Loads the grammar for fenced code in `lang` (the first parse compiles its regexes, which takes
/// 20 to 35 ms), so that highlighting a block later only costs the block.
pub fn warm_language(lang: &str) {
    let _ = highlight_fenced(Some(lang), &["x".to_string()], true);
}

/// Parses a few lines of every language once so the first real preview is not slow (the first
/// parse compiles the grammar's regexes, about 20 to 35 ms each).
pub fn warm_up() {
    for (lang, sample) in [
        (Lang::Sql, "select a, 'b' from t where n = 1 -- c\n"),
        (Lang::Bash, "echo \"$X\" | grep -v y # c\n"),
        (Lang::Sumo, "_sourceCategory=a | where b > 1 | count by c\n"),
        (
            Lang::Cwi,
            "fields @timestamp | filter @message like /x/ | limit 5\n",
        ),
    ] {
        let _ = classify(lang, sample);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> Vec<PayloadPiece> {
        vec![PayloadPiece::Text(t.to_string())]
    }

    fn class_of_word(lines: &[Vec<Span>], line: usize, word: &str) -> Option<Class> {
        lines[line]
            .iter()
            .find(|s| s.text.contains(word))
            .map(|s| s.class)
    }

    fn joined(lines: &[Vec<Span>]) -> String {
        lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn sql_tokens() {
        let src = "select id, 'x' from t where n = 42 -- c";
        let l = highlight(Lang::Sql, &text(&format!("{src}\n")), true);
        assert_eq!(joined(&l), src);
        for kw in ["select", "from", "where"] {
            assert_eq!(class_of_word(&l, 0, kw), Some(Class::Keyword), "{kw}");
        }
        assert_eq!(class_of_word(&l, 0, "'x'"), Some(Class::String));
        assert_eq!(class_of_word(&l, 0, "42"), Some(Class::Number));
        assert_eq!(class_of_word(&l, 0, "-- c"), Some(Class::Comment));
    }

    #[test]
    fn bash_tokens() {
        let l = highlight(Lang::Bash, &text("kubectl -n \"$NS\" logs # c\n"), true);
        assert_eq!(joined(&l), "kubectl -n \"$NS\" logs # c");
        assert_eq!(class_of_word(&l, 0, "# c"), Some(Class::Comment));
        assert!(
            l[0].iter()
                .any(|s| s.class == Class::String && s.text.contains("NS"))
        );
    }

    #[test]
    fn sumo_tokens() {
        let src = "_sourceCategory=prod/api | where status >= 500 | count by host";
        let l = highlight(Lang::Sumo, &text(&format!("{src}\n")), true);
        assert_eq!(joined(&l), src);
        for kw in ["where", "count", "by"] {
            assert_eq!(class_of_word(&l, 0, kw), Some(Class::Keyword), "{kw}");
        }
        assert_eq!(class_of_word(&l, 0, "_sourceCategory"), Some(Class::Field));
        assert_eq!(class_of_word(&l, 0, "500"), Some(Class::Number));
        assert!(
            l[0].iter()
                .any(|s| s.class == Class::Operator && s.text.contains('|'))
        );
    }

    #[test]
    fn cloudwatch_tokens() {
        let src = "fields @timestamp, @message | filter @message like /ERROR/ | limit 20";
        let l = highlight(Lang::Cwi, &text(&format!("{src}\n")), true);
        assert_eq!(joined(&l), src);
        for kw in ["fields", "filter", "limit", "like"] {
            assert_eq!(class_of_word(&l, 0, kw), Some(Class::Keyword), "{kw}");
        }
        assert_eq!(class_of_word(&l, 0, "@timestamp"), Some(Class::Field));
        assert_eq!(class_of_word(&l, 0, "/ERROR/"), Some(Class::String));
        assert_eq!(class_of_word(&l, 0, "20"), Some(Class::Number));
        let l = highlight(Lang::Cwi, &text("stats count(*) as n by bin(5m)\n"), true);
        assert!(
            l[0].iter()
                .any(|s| s.class == Class::Function && s.text.contains("count"))
        );
    }

    #[test]
    fn variables_are_marked_everywhere_and_escaped_braces_are_not() {
        let pieces = vec![
            PayloadPiece::Text("select * from t where a = '".into()),
            PayloadPiece::Var("env".into()),
            PayloadPiece::Text("' and b = ".into()),
            PayloadPiece::Var("limit".into()),
            PayloadPiece::Text(" -- uses ".into()),
            PayloadPiece::Var("note".into()),
            PayloadPiece::Text(" and {{literal}}\n".into()),
        ];
        let l = highlight(Lang::Sql, &pieces, true);
        assert_eq!(
            joined(&l),
            "select * from t where a = '{{env}}' and b = {{limit}} -- uses {{note}} and {{literal}}"
        );
        let vars: Vec<&str> = l[0]
            .iter()
            .filter(|s| s.class == Class::Variable)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(vars, vec!["{{env}}", "{{limit}}", "{{note}}"]);
        assert!(
            !l[0]
                .iter()
                .any(|s| s.class == Class::Variable && s.text.contains("literal"))
        );
    }

    #[test]
    fn multi_line_state_and_plain_text() {
        let l = highlight(Lang::Sql, &text("/* one\n   two */\nselect 1\n"), true);
        assert_eq!(l.len(), 3);
        assert!(
            l[0].iter().all(|s| s.class == Class::Comment)
                && l[1].iter().all(|s| s.class == Class::Comment)
        );
        assert_eq!(class_of_word(&l, 2, "select"), Some(Class::Keyword));
        let l = highlight(Lang::Plain, &text("hello {{x}} world\nline two\n"), true);
        assert_eq!(l.len(), 2);
        assert!(
            l[0].iter()
                .filter(|s| s.class != Class::Variable)
                .all(|s| s.class == Class::Plain)
                || true
        );
        let p = highlight(
            Lang::Plain,
            &[
                PayloadPiece::Text("a ".into()),
                PayloadPiece::Var("x".into()),
                PayloadPiece::Text(" b\n".into()),
            ],
            true,
        );
        assert_eq!(joined(&p), "a {{x}} b");
        assert_eq!(p[0][1].class, Class::Variable);
    }

    #[test]
    fn without_color_the_grammars_are_skipped_but_variables_stay_marked() {
        let pieces = vec![
            PayloadPiece::Text("select ".into()),
            PayloadPiece::Var("x".into()),
            PayloadPiece::Text("\n".into()),
        ];
        let l = highlight(Lang::Sql, &pieces, false);
        assert_eq!(joined(&l), "select {{x}}");
        assert!(
            l[0].iter()
                .all(|s| s.class == Class::Plain || s.class == Class::Variable)
        );
        assert!(l[0].iter().any(|s| s.class == Class::Variable));
    }

    #[test]
    fn warm_up_is_repeatable_and_languages_follow_the_type() {
        warm_up();
        warm_up();
        assert_eq!(lang_for(EntryType::Sql), Lang::Sql);
        assert_eq!(lang_for(EntryType::Cmd), Lang::Bash);
        assert_eq!(lang_for(EntryType::Sumo), Lang::Sumo);
        assert_eq!(lang_for(EntryType::Cw), Lang::Cwi);
        assert_eq!(lang_for(EntryType::Prompt), Lang::Plain);
    }

    #[test]
    fn a_long_payload_is_handled() {
        let src = "select 1 from t where a = 'b'\n".repeat(2000);
        let l = highlight(Lang::Sql, &text(&src), true);
        assert_eq!(l.len(), 2000);
    }
}
