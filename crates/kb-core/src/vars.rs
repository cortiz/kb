//! Variables: declarations under `vars`, and `{{name}}` references.

use crate::entry::Sink;
use crate::lint::RuleCode;
use crate::source::Span;
use crate::yaml::Node;

/// The kind of a variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    Text,
    Number,
    Enum,
    Secret,
}

impl VarKind {
    /// The values `kind:` accepts.
    pub const NAMES: [&'static str; 4] = ["text", "number", "enum", "secret"];

    /// The kind named `name`, if it is one of the four.
    pub fn from_name(name: &str) -> Option<VarKind> {
        match name {
            "text" => Some(VarKind::Text),
            "number" => Some(VarKind::Number),
            "enum" => Some(VarKind::Enum),
            "secret" => Some(VarKind::Secret),
            _ => None,
        }
    }
}

/// How a value is escaped when it is substituted (`escape:` under a variable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Escape {
    /// Quote by where the variable lands: shell in `cmd`, percent-encoding in `url`.
    #[default]
    Auto,
    None,
    Shell,
    Sql,
}

impl Escape {
    /// The values `escape:` accepts.
    pub const NAMES: [&'static str; 4] = ["auto", "none", "shell", "sql"];

    pub fn from_name(name: &str) -> Option<Escape> {
        match name {
            "auto" => Some(Escape::Auto),
            "none" => Some(Escape::None),
            "shell" => Some(Escape::Shell),
            "sql" => Some(Escape::Sql),
            _ => None,
        }
    }
}

/// Where a reference was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefLocation {
    Payload,
    /// A frontmatter field: `range`, `url`, `cwd` or `log_groups[i]`.
    Field(String),
}

/// One `{{name}}` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarRef {
    pub location: RefLocation,
    /// The `{{name}}` bytes in the file.
    pub span: Span,
}

/// A variable: declared under `vars`, or only referenced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub kind: VarKind,
    pub declared: bool,
    /// The default as written; never set for `secret`.
    pub default: Option<String>,
    pub options: Vec<String>,
    pub description: Option<String>,
    pub env: Option<String>,
    pub escape: Escape,
    pub references: Vec<VarRef>,
}

/// A piece of text to scan for references: its location and its span in the file.
pub struct ScanTarget {
    pub location: RefLocation,
    pub span: Span,
}

/// True when `name` is a valid variable name: `[a-z][a-z0-9_]*`.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A `{{…}}` occurrence found by [`scan_refs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Brace {
    /// A valid reference to `name` at this span (relative to the scanned text).
    Ref { name: String, span: Span },
    /// Looks like a reference but isn't one; it stays literal.
    NotAVariable { text: String, span: Span },
}

/// Finds `{{name}}` references in `text`. `\{{` is a literal and is skipped.
pub fn scan_refs(text: &str) -> Vec<Brace> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = text[i..].find("{{") {
        let start = i + rel;
        if start > 0 && text.as_bytes()[start - 1] == b'\\' {
            i = start + 2;
            continue;
        }
        let line_end = text[start..].find('\n').map_or(text.len(), |e| start + e);
        match text[start + 2..line_end].find("}}") {
            Some(len) => {
                let end = start + 2 + len + 2;
                let inner = &text[start + 2..start + 2 + len];
                out.push(if is_valid_name(inner) {
                    Brace::Ref {
                        name: inner.to_string(),
                        span: start..end,
                    }
                } else {
                    Brace::NotAVariable {
                        text: text[start..end].to_string(),
                        span: start..end,
                    }
                });
                i = end;
            }
            None => i = start + 2,
        }
    }
    out
}

/// Reads `vars`, scans `targets` in `text` for references, and reports KB040–KB049.
pub(crate) fn collect(
    tree: &Node,
    text: &str,
    targets: &[ScanTarget],
    sink: &mut Sink<'_>,
) -> Vec<Variable> {
    let (mut vars, key_offsets) = declarations(tree, sink);
    let mut reported_undeclared = Vec::new();
    for target in targets {
        for brace in scan_refs(&text[target.span.clone()]) {
            match brace {
                Brace::Ref { name, span } => {
                    let span = target.span.start + span.start..target.span.start + span.end;
                    let var = match vars.iter_mut().position(|v| v.name == name) {
                        Some(i) => &mut vars[i],
                        None => {
                            vars.push(Variable {
                                name: name.clone(),
                                kind: VarKind::Text,
                                declared: false,
                                default: None,
                                options: Vec::new(),
                                description: None,
                                env: None,
                                escape: Escape::Auto,
                                references: Vec::new(),
                            });
                            vars.last_mut().expect("just pushed")
                        }
                    };
                    if !var.declared && !reported_undeclared.contains(&name) {
                        sink.push(
                            RuleCode::UndeclaredVar,
                            span.start,
                            format!("`{{{{{name}}}}}` is not declared under `vars`; it is treated as text with no default"),
                            Some(format!("declare it: `{name}: {{ kind: text }}` under `vars`")),
                        );
                        reported_undeclared.push(name);
                    }
                    var.references.push(VarRef { location: target.location.clone(), span });
                }
                Brace::NotAVariable { text: found, span } => sink.push(
                    RuleCode::NotAVariable,
                    target.span.start + span.start,
                    format!("`{found}` is not a variable reference, so it stays as written"),
                    Some("variable names use lowercase letters, digits and `_` (`{{name}}`); write `\\{{` to silence this".into()),
                ),
            }
        }
    }
    for (var, offset) in vars.iter().zip(&key_offsets) {
        if var.references.is_empty() {
            sink.push(
                RuleCode::UnusedVar,
                *offset,
                format!("variable `{}` is declared but never used", var.name),
                Some(format!(
                    "remove it from `vars`, or reference it as `{{{{{}}}}}`",
                    var.name
                )),
            );
        }
    }
    vars
}

/// Reads the `vars` mapping. Returns the declared variables and the offset of each one's key.
fn declarations(tree: &Node, sink: &mut Sink<'_>) -> (Vec<Variable>, Vec<usize>) {
    let mut vars = Vec::new();
    let mut offsets = Vec::new();
    let Some(node) = tree.get("vars") else {
        return (vars, offsets);
    };
    if node.is_null() {
        return (vars, offsets);
    }
    if !matches!(node, Node::Map { .. }) {
        sink.push(
            RuleCode::WrongValueType,
            node.span().start,
            format!(
                "`vars` must be a mapping of variable names, not a {}",
                node.kind_name()
            ),
            Some("write `vars:` followed by lines like `  env: { kind: text }`".into()),
        );
        return (vars, offsets);
    }
    for (key, def) in node.entries() {
        if !is_valid_name(&key.name) {
            let suggested: String = key
                .name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            sink.push(
                RuleCode::InvalidVarName,
                key.span.start,
                format!("`{}` is not a valid variable name", key.name),
                Some(format!("use lowercase letters, digits and `_`, starting with a letter, for example `{suggested}`")),
            );
            continue;
        }
        let var = definition(&key.name, key.span.start, def, sink);
        vars.push(var);
        offsets.push(key.span.start);
    }
    (vars, offsets)
}

/// Common names people use for a kind, mapped to kb's name.
fn kind_synonym(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "string" | "str" => Some("text"),
        "int" | "integer" | "float" | "num" => Some("number"),
        "choice" | "select" | "list" => Some("enum"),
        "password" | "token" => Some("secret"),
        _ => None,
    }
}

fn definition(name: &str, key_offset: usize, def: &Node, sink: &mut Sink<'_>) -> Variable {
    let mut var = Variable {
        name: name.to_string(),
        kind: VarKind::Text,
        declared: true,
        default: None,
        options: Vec::new(),
        description: None,
        env: None,
        escape: Escape::Auto,
        references: Vec::new(),
    };
    if !matches!(def, Node::Map { .. }) {
        sink.push(
            RuleCode::WrongValueType,
            def.span().start,
            format!(
                "variable `{name}` must be a mapping, not a {}",
                def.kind_name()
            ),
            Some(format!("write `{name}: {{ kind: text }}`")),
        );
        return var;
    }
    match def.get("kind") {
        None => sink.push(
            RuleCode::MissingVarKind,
            key_offset,
            format!("variable `{name}` has no `kind`"),
            Some("add `kind:` with one of text, number, enum, secret".into()),
        ),
        Some(node) => {
            match node.as_str().and_then(VarKind::from_name) {
                Some(kind) => var.kind = kind,
                None => {
                    let shown = node.scalar().unwrap_or("");
                    let fix = match kind_synonym(shown)
                        .or_else(|| crate::lint::suggest::suggest(shown, &VarKind::NAMES))
                    {
                        Some(s) => format!("use \"{s}\""),
                        None => "use one of text, number, enum, secret".into(),
                    };
                    sink.push(
                    RuleCode::UnknownVarKind,
                    node.span().start,
                    format!("unknown variable kind \"{shown}\"; expected text, number, enum or secret"),
                    Some(fix),
                );
                }
            }
        }
    }
    var.description = def
        .get("description")
        .and_then(Node::as_str)
        .map(str::to_string);
    var.env = def.get("env").and_then(Node::as_str).map(str::to_string);
    if let Some(node) = def.get("escape") {
        match node.as_str().and_then(Escape::from_name) {
            Some(escape) => var.escape = escape,
            None => {
                let shown = node.scalar().unwrap_or("");
                let fix = match crate::lint::suggest::suggest(shown, &Escape::NAMES) {
                    Some(s) => format!("use \"{s}\""),
                    None => "use one of auto, none, shell, sql".into(),
                };
                sink.push(
                    RuleCode::InvalidEscape,
                    node.span().start,
                    format!("unknown escape \"{shown}\"; expected auto, none, shell or sql"),
                    Some(fix),
                );
            }
        }
    }
    if let Some(options) = def.get("options") {
        match options {
            Node::Seq { items, .. } => {
                var.options = items
                    .iter()
                    .filter_map(Node::scalar)
                    .map(str::to_string)
                    .collect()
            }
            _ if options.is_null() => {}
            _ => sink.push(
                RuleCode::WrongValueType,
                options.span().start,
                format!("`options` must be a list, not a {}", options.kind_name()),
                Some("write `options: [a, b]`".into()),
            ),
        }
    }
    if var.kind == VarKind::Enum && var.options.is_empty() {
        let at = def.get("options").map_or(key_offset, |o| o.span().start);
        sink.push(
            RuleCode::EnumOptions,
            at,
            format!("enum variable `{name}` needs a non-empty `options` list"),
            Some("add `options: [first, second]`".into()),
        );
    }
    if let Some((default_key, default)) = def.entry("default") {
        if var.kind == VarKind::Secret {
            sink.push(
                RuleCode::SecretDefault,
                default_key.span.start,
                format!("secret variable `{name}` cannot have a default; secrets are never stored"),
                Some(
                    "remove `default` and read the value from `env`, or let kb prompt for it"
                        .into(),
                ),
            );
            return var;
        }
        let value = default.scalar().map(str::to_string);
        match var.kind {
            VarKind::Number
                if !(default.is_number()
                    || value
                        .as_deref()
                        .is_some_and(|v| v.trim().parse::<f64>().is_ok())) =>
            {
                sink.push(
                    RuleCode::InvalidNumberDefault,
                    default.span().start,
                    format!("the default of number variable `{name}` is not a number"),
                    Some("use a number, for example `default: 15`".into()),
                );
            }
            VarKind::Enum
                if !var.options.is_empty()
                    && !value.as_ref().is_some_and(|v| var.options.contains(v)) =>
            {
                sink.push(
                    RuleCode::DefaultNotInOptions,
                    default.span().start,
                    format!("the default of enum variable `{name}` is not one of its options"),
                    Some(format!("use one of: {}", var.options.join(", "))),
                );
            }
            _ => {}
        }
        var.default = value;
    }
    var
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::parse_entry;

    #[test]
    fn names() {
        assert!(is_valid_name("env"));
        assert!(is_valid_name("a_1"));
        assert!(!is_valid_name("Env"));
        assert!(!is_valid_name("1a"));
        assert!(!is_valid_name("a-b"));
        assert!(!is_valid_name(""));
    }

    #[test]
    fn scanning() {
        let found = scan_refs("x {{env}} \\{{lit}} {{ Name }} {{a-b}} {{ok_2}} {{ unterminated");
        assert_eq!(
            found,
            vec![
                Brace::Ref {
                    name: "env".into(),
                    span: 2..9
                },
                Brace::NotAVariable {
                    text: "{{ Name }}".into(),
                    span: 19..29
                },
                Brace::NotAVariable {
                    text: "{{a-b}}".into(),
                    span: 30..37
                },
                Brace::Ref {
                    name: "ok_2".into(),
                    span: 38..46
                },
            ]
        );
    }

    #[test]
    fn braces_do_not_span_lines() {
        assert_eq!(scan_refs("{{a\n}}"), vec![]);
    }

    fn entry(front: &str, payload: &str) -> crate::entry::ParseOutcome {
        let text =
            format!("---\nkb_format: 1\ntype: sumo\ntitle: T\n{front}---\n\n```\n{payload}\n```\n");
        parse_entry("x", "x.md", text.into_bytes())
    }

    fn codes(front: &str, payload: &str) -> Vec<(u32, u32, RuleCode)> {
        entry(front, payload)
            .findings
            .iter()
            .map(|f| (f.line, f.column, f.code))
            .collect()
    }

    #[test]
    fn declared_and_used() {
        let out = entry(
            "range: \"-{{minutes}}m\"\nvars:\n  env: { kind: enum, options: [prod, dev], default: prod }\n  minutes: { kind: number, default: 15 }\n",
            "_sourceCategory={{env}}/x",
        );
        assert!(out.findings.is_empty(), "{:#?}", out.findings);
        let e = out.entry.unwrap();
        let vars = e.vars();
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "env");
        assert_eq!(vars[0].kind, VarKind::Enum);
        assert_eq!(vars[0].options, vec!["prod", "dev"]);
        assert_eq!(vars[0].default.as_deref(), Some("prod"));
        assert_eq!(vars[0].references.len(), 1);
        assert_eq!(vars[0].references[0].location, RefLocation::Payload);
        assert_eq!(
            vars[1].references[0].location,
            RefLocation::Field("range".into())
        );
    }

    #[test]
    fn undeclared_is_text_and_info() {
        let out = entry("", "x {{who}}");
        let f: Vec<_> = out
            .findings
            .iter()
            .map(|f| (f.line, f.column, f.code))
            .collect();
        assert_eq!(f, vec![(8, 3, RuleCode::UndeclaredVar)]);
        let e = out.entry.unwrap();
        let v = &e.vars()[0];
        assert_eq!(
            (v.name.as_str(), v.kind, v.declared),
            ("who", VarKind::Text, false)
        );
    }

    #[test]
    fn unused_is_warning() {
        assert_eq!(
            codes("vars:\n  region: { kind: text }\n", "x"),
            vec![(6, 3, RuleCode::UnusedVar)]
        );
    }

    #[test]
    fn not_a_variable_is_info() {
        assert_eq!(
            codes("", "{{ .Values }}"),
            vec![(8, 1, RuleCode::NotAVariable)]
        );
        assert_eq!(codes("", "\\{{ .Values }}"), vec![]);
    }

    #[test]
    fn references_in_fields() {
        let text = "---\nkb_format: 1\ntype: cw\ntitle: T\nlog_groups: [\"/aws/{{env}}\"]\nvars:\n  env: { kind: text }\n---\n\n```\nfields @message\n```\n";
        let out = parse_entry("x", "x.md", text.as_bytes().to_vec());
        assert!(out.findings.is_empty(), "{:#?}", out.findings);
        let e = out.entry.unwrap();
        assert_eq!(
            e.vars()[0].references[0].location,
            RefLocation::Field("log_groups[0]".into())
        );
        let text =
            "---\nkb_format: 1\ntype: cmd\ntitle: T\ncwd: \"{{dir}}\"\n---\n\n```\nls\n```\n";
        let out = parse_entry("x", "x.md", text.as_bytes().to_vec());
        assert_eq!(out.findings[0].code, RuleCode::UndeclaredVar);
        assert_eq!((out.findings[0].line, out.findings[0].column), (5, 7));
        let text = "---\nkb_format: 1\ntype: link\ntitle: T\nurl: \"https://{{host}}/x\"\nvars:\n  host: { kind: text }\n---\n";
        assert!(
            parse_entry("x", "x.md", text.as_bytes().to_vec())
                .findings
                .is_empty()
        );
    }

    #[test]
    fn definition_rules() {
        assert_eq!(
            codes("vars:\n  Env: { kind: text }\n", "{{env}}"),
            vec![
                (6, 3, RuleCode::InvalidVarName),
                (10, 1, RuleCode::UndeclaredVar)
            ]
        );
        assert_eq!(
            codes("vars:\n  a: { default: x }\n", "{{a}}"),
            vec![(6, 3, RuleCode::MissingVarKind)]
        );
        let out = entry("vars:\n  a: { kind: string }\n", "{{a}}");
        let f = &out.findings[0];
        assert_eq!(
            (f.line, f.column, f.code),
            (6, 14, RuleCode::UnknownVarKind)
        );
        assert_eq!(f.fix.as_deref(), Some("use \"text\""));
        assert_eq!(
            codes("vars:\n  a: { kind: secret, default: x }\n", "{{a}}"),
            vec![(6, 22, RuleCode::SecretDefault)]
        );
        assert_eq!(
            codes("vars:\n  a: { kind: enum }\n", "{{a}}"),
            vec![(6, 3, RuleCode::EnumOptions)]
        );
        assert_eq!(
            codes("vars:\n  a: { kind: enum, options: [] }\n", "{{a}}"),
            vec![(6, 29, RuleCode::EnumOptions)]
        );
        assert_eq!(
            codes(
                "vars:\n  a: { kind: enum, options: [x], default: y }\n",
                "{{a}}"
            ),
            vec![(6, 43, RuleCode::DefaultNotInOptions)]
        );
        assert_eq!(
            codes("vars:\n  a: { kind: number, default: abc }\n", "{{a}}"),
            vec![(6, 31, RuleCode::InvalidNumberDefault)]
        );
        assert_eq!(
            codes("vars:\n  a: { kind: number, default: \"2.5\" }\n", "{{a}}"),
            vec![]
        );
        assert_eq!(
            codes("vars: [a]\n", "x"),
            vec![(5, 7, RuleCode::WrongValueType)]
        );
        assert_eq!(
            codes("vars:\n  a: text\n", "{{a}}"),
            vec![(6, 6, RuleCode::WrongValueType)]
        );
    }

    #[test]
    fn secret_never_keeps_default() {
        let out = entry(
            "vars:\n  t: { kind: secret, env: API_TOKEN, default: x }\n",
            "{{t}}",
        );
        let e = out.entry.unwrap();
        let v = &e.vars()[0];
        assert_eq!(v.kind, VarKind::Secret);
        assert_eq!(v.default, None);
        assert_eq!(v.env.as_deref(), Some("API_TOKEN"));
    }

    #[test]
    fn escape_is_read_and_checked() {
        let at = |v: &str| {
            let out = entry(
                &format!("vars:\n  a: {{ kind: text, escape: {v} }}\n"),
                "{{a}}",
            );
            (out.findings, out.entry.unwrap().vars()[0].escape)
        };
        for (name, want) in [
            ("auto", Escape::Auto),
            ("none", Escape::None),
            ("shell", Escape::Shell),
            ("sql", Escape::Sql),
        ] {
            let (findings, got) = at(name);
            assert!(findings.is_empty(), "{name}: {findings:?}");
            assert_eq!(got, want);
        }
        let out = entry("vars:\n  a: { kind: text }\n", "{{a}}");
        assert_eq!(out.entry.unwrap().vars()[0].escape, Escape::Auto);
        let out = entry("vars:\n  t: { kind: secret, escape: shell }\n", "{{t}}");
        assert!(out.findings.is_empty());
        let out = entry("vars:\n  a: { kind: text, escape: shel }\n", "{{a}}");
        let f = &out.findings[0];
        assert_eq!((f.line, f.column, f.code), (6, 28, RuleCode::InvalidEscape));
        assert_eq!(f.fix.as_deref(), Some("use \"shell\""));
        assert_eq!(out.entry.unwrap().vars()[0].escape, Escape::Auto);
        let out = entry("vars:\n  a: { kind: text, escape: [shell] }\n", "{{a}}");
        assert_eq!(out.findings[0].code, RuleCode::InvalidEscape);
    }
}
