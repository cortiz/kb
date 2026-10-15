//! Variable resolution (spec 006): list an entry's variables, pre-fill them, validate values and
//! substitute them. This is the only place `{{name}}` is replaced (FR-004).
//!
//! Everything here is a pure function of plain values. The environment is a closure and the
//! remembered values are a map, so tests need no process state.

pub mod escape;
pub mod secret;

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

use crate::body::unescape;
use crate::entry::{Entry, EntryType, Shell, TypeFields};
use crate::usage::content_hash;
use crate::vars::{Brace, Escape, VarKind, Variable, scan_refs};
use escape::Quote;
pub use secret::{MASK, Value};

/// A string of the entry that variables are resolved in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    Payload,
    Url,
    Cwd,
    Range,
    LogGroup(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    /// Literal text, with `\{{` already turned into `{{`.
    Lit(String),
    Ref(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Template {
    target: Target,
    pieces: Vec<Piece>,
}

/// A variable that the entry uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedVar {
    pub var: Variable,
    pub locations: Vec<Target>,
}

/// An entry's variables and the strings they go into. Built by [`plan_vars`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarPlan {
    /// In order of first appearance in the file.
    pub vars: Vec<PlannedVar>,
    /// The shell values are quoted for (`sh` for entries that are not `cmd`).
    pub shell: Shell,
    entry_type: Option<EntryType>,
    templates: Vec<Template>,
    /// Changes when the variables or the text around them change (to notice an edit).
    pub signature: u64,
}

impl VarPlan {
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    pub fn var(&self, name: &str) -> Option<&PlannedVar> {
        self.vars.iter().find(|v| v.var.name == name)
    }
}

fn scan(text: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut at = 0;
    for brace in scan_refs(text) {
        if let Brace::Ref { name, span } = brace {
            if span.start > at {
                pieces.push(Piece::Lit(unescape(&text[at..span.start])));
            }
            pieces.push(Piece::Ref(name));
            at = span.end;
        }
    }
    if at < text.len() {
        pieces.push(Piece::Lit(unescape(&text[at..])));
    }
    pieces
}

/// The variables the entry uses, with the strings to resolve. Declared variables that nothing
/// references are left out.
pub fn plan_vars(entry: &Entry) -> VarPlan {
    let mut templates = Vec::new();
    if let Some(p) = entry.payload() {
        let raw = entry.source().text()[p.raw_span.clone()].replace("\r\n", "\n");
        templates.push(Template {
            target: Target::Payload,
            pieces: scan(&raw),
        });
    }
    let mut field = |target: Target, value: &Option<String>| {
        if let Some(v) = value {
            templates.push(Template {
                target,
                pieces: scan(v),
            });
        }
    };
    let mut shell = Shell::Sh;
    match entry.fields() {
        TypeFields::Sumo { range } => field(Target::Range, range),
        TypeFields::Cw {
            log_groups, range, ..
        } => {
            for (i, g) in log_groups.iter().enumerate() {
                field(Target::LogGroup(i), &Some(g.clone()));
            }
            field(Target::Range, range);
        }
        TypeFields::Cmd { shell: s, cwd } => {
            shell = *s;
            field(Target::Cwd, cwd);
        }
        TypeFields::Link { url } => field(Target::Url, url),
        _ => {}
    }
    let mut used: Vec<(String, Vec<Target>)> = Vec::new();
    if entry.entry_type() == Some(EntryType::Runbook) {
        for v in entry.vars() {
            if !used.iter().any(|(n, _)| n == &v.name) {
                used.push((v.name.clone(), vec![Target::Payload]));
            }
        }
    }
    for t in &templates {
        for piece in &t.pieces {
            if let Piece::Ref(name) = piece {
                match used.iter_mut().find(|(n, _)| n == name) {
                    Some((_, locs)) => {
                        if !locs.contains(&t.target) {
                            locs.push(t.target.clone());
                        }
                    }
                    None => used.push((name.clone(), vec![t.target.clone()])),
                }
            }
        }
    }
    let first_span = |name: &str| {
        entry
            .vars()
            .iter()
            .find(|v| v.name == name)
            .and_then(|v| v.references.iter().map(|r| r.span.start).min())
            .unwrap_or(usize::MAX)
    };
    let mut vars: Vec<PlannedVar> = used
        .into_iter()
        .map(|(name, locations)| {
            let var = entry
                .vars()
                .iter()
                .find(|v| v.name == name)
                .cloned()
                .unwrap_or_else(|| Variable {
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
            PlannedVar { var, locations }
        })
        .collect();
    vars.sort_by_key(|v| first_span(&v.var.name)); // stable: ties keep scan order
    let signature = {
        let shape: Vec<_> = vars
            .iter()
            .map(|v| {
                (
                    &v.var.name,
                    v.var.kind as u8,
                    &v.var.options,
                    &v.var.default,
                    &v.var.env,
                    v.var.escape as u8,
                    v.var.declared,
                )
            })
            .collect();
        content_hash(format!("{shape:?}{shell:?}{templates:?}").as_bytes())
    };
    VarPlan {
        vars,
        shell,
        entry_type: entry.entry_type(),
        templates,
        signature,
    }
}

// ---------------------------------------------------------------------------------------------
// Pre-fill

/// Where a pre-filled or typed value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Flag,
    Env,
    Remembered,
    Default,
    Typed,
    None,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Flag => "flag",
            Source::Env => "env",
            Source::Remembered => "remembered",
            Source::Default => "default",
            Source::Typed => "typed",
            Source::None => "none",
        }
    }
}

/// A variable with the value it starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    pub value: Option<Value>,
    pub source: Source,
    pub secret: bool,
}

/// What pre-filling looks at.
pub struct Inputs<'a> {
    /// `--var name=value`, in order; the last one for a name wins.
    pub flags: &'a [(String, String)],
    /// Looks up an environment variable.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// The values remembered for this entry.
    pub memory: Option<&'a BTreeMap<String, String>>,
}

/// The starting value of every variable, in plan order (FR-010): flag, environment, remembered,
/// default, nothing. A remembered value or default that is not valid for the variable is skipped.
pub fn prefill(plan: &VarPlan, inputs: &Inputs<'_>) -> Vec<Binding> {
    plan.vars
        .iter()
        .map(|p| {
            let v = &p.var;
            let secret = v.kind == VarKind::Secret;
            let make = |text: String, source| Binding {
                name: v.name.clone(),
                value: Some(Value::new(text, secret)),
                source,
                secret,
            };
            let valid = |text: &str| validate(v, &Value::new(text, secret)).is_ok();
            if let Some((_, text)) = inputs.flags.iter().rev().find(|(n, _)| *n == v.name) {
                return make(text.clone(), Source::Flag);
            }
            if let Some(text) = v
                .env
                .as_deref()
                .and_then(|name| (inputs.env)(name))
                .filter(|t| !t.is_empty())
            {
                return make(text, Source::Env);
            }
            if !secret {
                if let Some(text) = inputs
                    .memory
                    .and_then(|m| m.get(&v.name))
                    .filter(|t| valid(t))
                {
                    return make(text.clone(), Source::Remembered);
                }
                if let Some(text) = v.default.as_ref().filter(|t| valid(t)) {
                    return make(text.clone(), Source::Default);
                }
            }
            Binding {
                name: v.name.clone(),
                value: None,
                source: Source::None,
                secret,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Validation

/// A value that cannot be used. The message never contains a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarError {
    pub name: String,
    pub message: String,
}

impl fmt::Display for VarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.message)
    }
}

fn single_line(text: &str) -> bool {
    !text.contains(['\n', '\r', '\0'])
}

/// True for `-?\d+(\.\d+)?`.
fn is_number(text: &str) -> bool {
    let t = text.strip_prefix('-').unwrap_or(text);
    let (int, frac) = match t.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (t, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    digits(int) && frac.is_none_or(digits)
}

/// Checks `value` against the kind of `var` (FR-005, FR-006).
pub fn validate(var: &Variable, value: &Value) -> Result<(), VarError> {
    let text = value.reveal();
    let fail = |message: String| {
        Err(VarError {
            name: var.name.clone(),
            message,
        })
    };
    match var.kind {
        VarKind::Text | VarKind::Secret if !single_line(text) => {
            fail("expected a single line of text".into())
        }
        VarKind::Text if text.is_empty() && var.default.as_deref() != Some("") => {
            fail("a value is required".into())
        }
        VarKind::Secret if text.is_empty() => fail("a value is required".into()),
        VarKind::Number if !is_number(text) => fail("expected a number like 30, -5 or 0.25".into()),
        VarKind::Enum if !var.options.iter().any(|o| o == text) => {
            fail(format!("expected one of {}", var.options.join(", ")))
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------------------------
// Resolution

/// The values to substitute, by variable name.
pub type Values = BTreeMap<String, Value>;

/// One substituted value in a resolved string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub name: String,
    pub secret: bool,
}

/// A resolved string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub target: Target,
    pub text: String,
    pub spans: Vec<Span>,
}

impl Output {
    /// The text with every secret replaced by `••••`, for display.
    pub fn redacted(&self) -> String {
        self.display().0
    }

    /// The text for display, secrets masked, and where each substituted value sits in it.
    pub fn display(&self) -> (String, Vec<Range<usize>>) {
        let mut out = String::new();
        let mut marks = Vec::new();
        let mut at = 0;
        for s in &self.spans {
            out.push_str(&self.text[at..s.range.start]);
            let start = out.len();
            if s.secret {
                out.push_str(MASK);
            } else {
                out.push_str(&self.text[s.range.clone()]);
            }
            marks.push(start..out.len());
            at = s.range.end;
        }
        out.push_str(&self.text[at..]);
        (out, marks)
    }
}

/// The result of resolving an entry (the spec's ResolutionResult).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub outputs: Vec<Output>,
    /// A secret was substituted somewhere.
    pub has_secret: bool,
}

impl Resolved {
    pub fn get(&self, target: &Target) -> Option<&Output> {
        self.outputs.iter().find(|o| &o.target == target)
    }

    /// What `kb copy` puts on the clipboard: the payload, or the URL of a `link`.
    pub fn primary(&self) -> Option<&Output> {
        self.get(&Target::Payload)
            .or_else(|| self.get(&Target::Url))
    }
}

fn escape_value(
    plan: &VarPlan,
    var: &Variable,
    target: &Target,
    state: Quote,
    value: &str,
) -> String {
    let shell_target = plan.entry_type == Some(EntryType::Cmd)
        && matches!(target, Target::Payload | Target::Cwd)
        || *target == Target::Cwd;
    match var.escape {
        Escape::None => value.to_string(),
        Escape::Sql => escape::sql(value),
        Escape::Shell => escape::unquoted(value, plan.shell),
        Escape::Auto if shell_target => match state {
            Quote::None => escape::unquoted(value, plan.shell),
            Quote::Single => escape::in_single(value, plan.shell),
            Quote::Double => escape::in_double(value, plan.shell),
        },
        Escape::Auto if *target == Target::Url => escape::percent(value),
        Escape::Auto => value.to_string(),
    }
}

use crate::shell::ShellDialect;

/// Where resolved output is destined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionTarget {
    Standard,
    ShellWidget(ShellDialect),
    Mcp,
}

/// Resolves variables with target destination rules.
pub fn resolve_for_target(
    plan: &VarPlan,
    values: &Values,
    target: ResolutionTarget,
) -> Result<Resolved, Vec<VarError>> {
    let mut errors = Vec::new();
    for p in &plan.vars {
        if p.var.kind == VarKind::Secret {
            if let ResolutionTarget::ShellWidget(_) = target {
                if p.var.env.is_none() {
                    errors.push(VarError {
                        name: p.var.name.clone(),
                        message: format!(
                            "secret variable '{}' has no environment variable source; command line insertion refused (use kb run)",
                            p.var.name
                        ),
                    });
                }
                continue;
            }
            if let ResolutionTarget::Mcp = target {
                if values.contains_key(&p.var.name) {
                    errors.push(VarError {
                        name: p.var.name.clone(),
                        message: format!(
                            "cannot supply value for secret variable '{}'",
                            p.var.name
                        ),
                    });
                }
                continue;
            }
        }
        match values.get(&p.var.name) {
            Some(v) => {
                if let Err(e) = validate(&p.var, v) {
                    errors.push(e);
                }
            }
            None => {
                let msg = if let ResolutionTarget::Mcp = target {
                    let options_desc = if p.var.options.is_empty() {
                        "".to_string()
                    } else {
                        format!(", options: {:?}", p.var.options)
                    };
                    format!(
                        "missing required variable '{}' (kind: {}{})",
                        p.var.name,
                        p.var.kind.name(),
                        options_desc
                    )
                } else {
                    "a value is required".into()
                };
                errors.push(VarError {
                    name: p.var.name.clone(),
                    message: msg,
                });
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(substitute_target(plan, values, false, target))
}

/// Validates `values` and substitutes them, in one pass over each string. Returns every problem
/// found, not just the first, and nothing partially resolved.
pub fn resolve(plan: &VarPlan, values: &Values) -> Result<Resolved, Vec<VarError>> {
    resolve_for_target(plan, values, ResolutionTarget::Standard)
}

/// The same substitution for a live preview: a variable without a valid value keeps its
/// `{{name}}` placeholder instead of failing.
pub fn preview(plan: &VarPlan, values: &Values) -> Resolved {
    preview_for_target(plan, values, ResolutionTarget::Standard)
}

/// Live preview with a specific resolution target.
pub fn preview_for_target(plan: &VarPlan, values: &Values, target: ResolutionTarget) -> Resolved {
    substitute_target(plan, values, true, target)
}

fn substitute_target(
    plan: &VarPlan,
    values: &Values,
    lenient: bool,
    target: ResolutionTarget,
) -> Resolved {
    let mut outputs = Vec::new();
    let mut has_secret = false;
    for t in &plan.templates {
        let mut text = String::new();
        let mut spans = Vec::new();
        let mut state = Quote::None;
        for piece in &t.pieces {
            match piece {
                Piece::Lit(l) => {
                    text.push_str(l);
                    state = escape::advance(state, l);
                }
                Piece::Ref(name) => {
                    let p = plan.var(name).expect("planned");
                    if p.var.kind == VarKind::Secret {
                        match target {
                            ResolutionTarget::ShellWidget(dialect) => {
                                if let Some(env_name) = &p.var.env {
                                    let start = text.len();
                                    let ref_str = match state {
                                        Quote::Double => format!("${env_name}"),
                                        Quote::None | Quote::Single => {
                                            dialect.env_var_ref(env_name)
                                        }
                                    };
                                    text.push_str(&ref_str);
                                    has_secret = true;
                                    spans.push(Span {
                                        range: start..text.len(),
                                        name: name.clone(),
                                        secret: true,
                                    });
                                    state = escape::advance(state, &ref_str);
                                    continue;
                                }
                            }
                            ResolutionTarget::Mcp => {
                                if let Some(env_name) = &p.var.env {
                                    let start = text.len();
                                    let ref_str = format!("${env_name}");
                                    text.push_str(&ref_str);
                                    has_secret = true;
                                    spans.push(Span {
                                        range: start..text.len(),
                                        name: name.clone(),
                                        secret: true,
                                    });
                                    state = escape::advance(state, &ref_str);
                                    continue;
                                }
                            }
                            ResolutionTarget::Standard => {}
                        }
                    }
                    let value = values
                        .get(name)
                        .filter(|v| !lenient || validate(&p.var, v).is_ok());
                    let Some(value) = value else {
                        text.push_str(&format!("{{{{{name}}}}}"));
                        continue;
                    };
                    let start = text.len();
                    text.push_str(&escape_value(
                        plan,
                        &p.var,
                        &t.target,
                        state,
                        value.reveal(),
                    ));
                    has_secret |= value.is_secret();
                    spans.push(Span {
                        range: start..text.len(),
                        name: name.clone(),
                        secret: value.is_secret(),
                    });
                }
            }
        }
        outputs.push(Output {
            target: t.target.clone(),
            text,
            spans,
        });
    }
    Resolved {
        outputs,
        has_secret,
    }
}

#[cfg(test)]
mod tests;
