//! Variables on the command line: `--var` parsing, the prompts on the controlling terminal, and
//! `kb vars`. Values are checked and substituted by `kb_core::varres`; nothing here builds text.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process::ExitCode;

use clap::Args;
use kb_core::varres::{
    self, Binding, Inputs, MASK, PlannedVar, Source, Value, Values, VarError, VarPlan,
};
use kb_core::vars::VarKind;
use serde_json::{Value as Json, json};

use crate::output::CliError;
use crate::platform::Platform;

/// The flags `kb use` and `kb copy` share.
#[derive(Args, Debug, Clone, Default)]
pub struct VarArgs {
    /// Give a variable's value: `--var env=prod`. Repeatable.
    #[arg(long = "var", value_name = "NAME=VALUE")]
    pub vars: Vec<String>,
    /// Never ask: use the pre-filled value of every variable not given with --var.
    #[arg(short = 'y', long)]
    pub yes: bool,
    /// Write the resolved text to stdout instead of the clipboard.
    #[arg(long)]
    pub print: bool,
    /// Do not remember the values used.
    #[arg(long)]
    pub no_remember: bool,
}

/// What a run of `kb use` was asked to do, with the flags parsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UseOpts {
    pub flags: Vec<(String, String)>,
    pub yes: bool,
    pub print: bool,
    pub remember: bool,
    pub json: bool,
}

impl VarArgs {
    pub fn into_opts(self, json: bool) -> Result<UseOpts, CliError> {
        let mut flags = Vec::new();
        for raw in &self.vars {
            let Some((name, value)) = raw.split_once('=') else {
                return Err(CliError::new(
                    "usage",
                    format!("--var {raw:?}: expected NAME=VALUE"),
                ));
            };
            if !kb_core::vars::is_valid_name(name) {
                return Err(CliError::new(
                    "usage",
                    format!(
                        "--var {raw:?}: {name:?} is not a variable name (lowercase letters, digits and `_`)"
                    ),
                ));
            }
            flags.push((name.to_string(), value.to_string()));
        }
        Ok(UseOpts {
            flags,
            yes: self.yes,
            print: self.print,
            remember: !self.no_remember,
            json,
        })
    }
}

// ---- asking ----

/// Where questions are asked. The real one is the controlling terminal.
pub trait Ask {
    /// A line of input. `None`: the user gave up (end of input, Ctrl-C).
    fn line(&mut self, prompt: &str) -> io::Result<Option<String>>;
    /// The same, with nothing echoed.
    fn secret(&mut self, prompt: &str) -> io::Result<Option<String>>;
    fn say(&mut self, text: &str);
}

/// The controlling terminal (`/dev/tty`), so prompts work when stdin or stdout is redirected.
pub struct TtyAsk {
    reader: BufReader<File>,
    out: File,
}

impl TtyAsk {
    pub fn open(platform: &impl Platform) -> Option<TtyAsk> {
        let tty = platform.open_tty().ok()?;
        let out = tty.try_clone().ok()?;
        Some(TtyAsk {
            reader: BufReader::new(tty),
            out,
        })
    }
}

/// Puts the terminal in raw mode and puts it back, also on panic.
struct RawMode;

impl RawMode {
    fn on() -> io::Result<RawMode> {
        ratatui::crossterm::terminal::enable_raw_mode()?;
        Ok(RawMode)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = ratatui::crossterm::terminal::disable_raw_mode();
    }
}

impl Ask for TtyAsk {
    fn line(&mut self, prompt: &str) -> io::Result<Option<String>> {
        self.out.write_all(prompt.as_bytes())?;
        self.out.flush()?;
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\n', '\r']).to_string()))
    }

    fn secret(&mut self, prompt: &str) -> io::Result<Option<String>> {
        use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
        self.out.write_all(prompt.as_bytes())?;
        self.out.flush()?;
        let _raw = RawMode::on()?;
        let mut typed = String::new();
        let result = loop {
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Enter => break Some(typed),
                KeyCode::Esc => break None,
                KeyCode::Char('c') if ctrl => break None,
                KeyCode::Char('d') if ctrl && typed.is_empty() => break None,
                KeyCode::Char('u') if ctrl => typed.clear(),
                KeyCode::Backspace => {
                    typed.pop();
                }
                KeyCode::Char(c) if !ctrl => typed.push(c),
                _ => {}
            }
        };
        self.out.write_all(b"\r\n")?;
        self.out.flush()?;
        Ok(result)
    }

    fn say(&mut self, text: &str) {
        let _ = writeln!(self.out, "{text}");
    }
}

// ---- collecting values ----

/// A value as it was used, for reports. A secret carries no text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Used {
    pub name: String,
    pub kind: VarKind,
    pub source: Source,
    pub text: Option<String>,
}

pub enum Collected {
    Ready(Values, Vec<Used>),
    /// Already reported on stderr; this is the exit code.
    Stop(ExitCode),
}

pub fn kind_label(p: &PlannedVar) -> String {
    match p.var.kind {
        VarKind::Enum => format!("enum: {}", p.var.options.join("|")),
        k => VarKind::NAMES[k as usize].to_string(),
    }
}

fn used(p: &PlannedVar, source: Source, value: &Value) -> Used {
    Used {
        name: p.var.name.clone(),
        kind: p.var.kind,
        source,
        text: (!value.is_secret()).then(|| value.reveal().to_string()),
    }
}

fn prompt_text(p: &PlannedVar, b: &Binding) -> String {
    let mut s = format!("{} ({})", p.var.name, kind_label(p));
    if let Some(v) = b.value.as_ref().filter(|_| !b.secret) {
        s.push_str(&format!(" [{}]", v.reveal()));
    }
    s.push_str(": ");
    s
}

/// Gets a value for every variable of `plan`: from `--var`, the pre-fill, or by asking.
/// With `ask = None` (no terminal, or `--yes`) a variable with nothing to start from is missing.
/// Reports problems on stderr and returns `Stop` with the exit code (FR-024).
pub fn collect(
    plan: &VarPlan,
    flags: &[(String, String)],
    env: &dyn Fn(&str) -> Option<String>,
    memory: Option<&BTreeMap<String, String>>,
    mut ask: Option<&mut dyn Ask>,
) -> Result<Collected, CliError> {
    let bindings = varres::prefill(plan, &Inputs { flags, env, memory });
    // values given on the command line must be valid before anything is asked
    let mut bad: Vec<VarError> = Vec::new();
    for (p, b) in plan.vars.iter().zip(&bindings) {
        if b.source == Source::Flag
            && let Some(v) = &b.value
        {
            bad.extend(varres::validate(&p.var, v).err());
        }
    }
    if !bad.is_empty() {
        for e in &bad {
            eprintln!("kb: invalid value for --var {e}");
        }
        return Ok(Collected::Stop(ExitCode::from(2)));
    }
    let mut values = Values::new();
    let mut report = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for (p, b) in plan.vars.iter().zip(&bindings) {
        let name = &p.var.name;
        // a secret from the environment is used as is; everything else is confirmed when asking
        let settled = b.value.as_ref().filter(|_| {
            b.source == Source::Flag || (b.secret && b.source == Source::Env) || ask.is_none()
        });
        if let Some(v) = settled {
            if let Err(e) = varres::validate(&p.var, v) {
                missing.push(format!("{e}"));
                continue;
            }
            if b.secret && b.source == Source::Flag {
                eprintln!(
                    "kb: warning: --var {name}=… may be saved in your shell history; use the variable's env source instead"
                );
            }
            report.push(used(p, b.source, v));
            values.insert(name.clone(), v.clone());
            continue;
        }
        let Some(ask) = ask.as_deref_mut() else {
            missing.push(format!(
                "{name}  {}  (no flag, env, remembered value or default)",
                kind_label(p)
            ));
            continue;
        };
        if let Some(d) = &p.var.description {
            ask.say(&format!("  {d}"));
        }
        let prompt = prompt_text(p, b);
        loop {
            let answer = if b.secret {
                ask.secret(&prompt)
            } else {
                ask.line(&prompt)
            }
            .map_err(|e| CliError::new("io", format!("cannot read from the terminal: {e}")))?;
            let Some(answer) = answer else {
                eprintln!("kb: cancelled");
                return Ok(Collected::Stop(ExitCode::from(130)));
            };
            let (text, source) = match (answer.is_empty(), &b.value) {
                (true, Some(v)) => (v.reveal().to_string(), b.source),
                _ => (answer, Source::Typed),
            };
            let value = Value::new(text, b.secret);
            match varres::validate(&p.var, &value) {
                Ok(()) => {
                    report.push(used(p, source, &value));
                    values.insert(name.clone(), value);
                    break;
                }
                Err(e) => ask.say(&format!("  {e}")),
            }
        }
    }
    if !missing.is_empty() {
        eprintln!("kb: missing values:");
        for m in &missing {
            eprintln!("  {m}");
        }
        return Ok(Collected::Stop(ExitCode::from(1)));
    }
    Ok(Collected::Ready(values, report))
}

/// `--var` names the entry does not use: exit 2 with the names it does.
pub fn check_flag_names(plan: &VarPlan, flags: &[(String, String)]) -> Result<(), CliError> {
    for (name, _) in flags {
        if plan.var(name).is_none() {
            let uses: Vec<_> = plan.vars.iter().map(|v| v.var.name.as_str()).collect();
            let uses = if uses.is_empty() {
                "none".to_string()
            } else {
                uses.join(", ")
            };
            return Err(CliError::new(
                "usage",
                format!("--var {name}: the entry has no such variable; it uses: {uses}"),
            ));
        }
    }
    Ok(())
}

pub fn used_json(used: &[Used]) -> Json {
    Json::Array(
        used.iter()
            .map(|u| {
                json!({
                    "name": u.name,
                    "kind": VarKind::NAMES[u.kind as usize],
                    "source": u.source.name(),
                    "value": u.text,
                })
            })
            .collect(),
    )
}

// ---- kb vars ----

/// `kb vars`: the entry's variables and what would be pre-filled.
pub fn vars_report(
    plan: &VarPlan,
    id: &str,
    env: &dyn Fn(&str) -> Option<String>,
    memory: Option<&BTreeMap<String, String>>,
    json: bool,
) -> String {
    let bindings = varres::prefill(
        plan,
        &Inputs {
            flags: &[],
            env,
            memory,
        },
    );
    if json {
        let vars: Vec<Json> = plan
            .vars
            .iter()
            .zip(&bindings)
            .map(|(p, b)| {
                let v = &p.var;
                let secret = v.kind == VarKind::Secret;
                let default_valid = v.default.as_ref().map(|d| {
                    varres::validate(v, &Value::plain(d.clone())).is_ok()
                });
                json!({
                    "name": v.name,
                    "kind": VarKind::NAMES[v.kind as usize],
                    "options": v.options,
                    "default": if secret { Json::Null } else { json!(v.default) },
                    "default_valid": default_valid,
                    "env": v.env.as_ref().map(|n| json!({ "name": n, "set": env(n).is_some_and(|x| !x.is_empty()) })),
                    "remembered": if secret { Json::Null } else { json!(memory.and_then(|m| m.get(&v.name))) },
                    "prefill": { "value": if secret { Json::Null } else { json!(b.value.as_ref().map(|x| x.reveal())) }, "source": b.source.name() },
                    "declared": v.declared,
                    "escape": kb_core::vars::Escape::NAMES[v.escape as usize],
                    "locations": p.locations.iter().map(|t| format!("{t:?}")).collect::<Vec<_>>(),
                })
            })
            .collect();
        return format!("{}\n", json!({ "id": id, "variables": vars }));
    }
    if plan.is_empty() {
        return "no variables\n".to_string();
    }
    let mut out = String::new();
    for (p, b) in plan.vars.iter().zip(&bindings) {
        let v = &p.var;
        let secret = v.kind == VarKind::Secret;
        let mut parts = vec![format!("{:<10}", v.name), format!("{:<22}", kind_label(p))];
        if !secret && let Some(d) = &v.default {
            let ok = varres::validate(v, &Value::plain(d.clone())).is_ok();
            parts.push(format!(
                "default: {d}{}",
                if ok { "" } else { " (invalid, ignored)" }
            ));
        }
        if let Some(n) = &v.env {
            let set = env(n).is_some_and(|x| !x.is_empty());
            parts.push(format!("env: {n} ({})", if set { "set" } else { "unset" }));
        }
        if !secret && let Some(m) = memory.and_then(|m| m.get(&v.name)) {
            parts.push(format!("remembered: {m}"));
        }
        if !v.declared {
            parts.push("undeclared".into());
        }
        if v.escape != kb_core::vars::Escape::Auto {
            parts.push(format!(
                "escape: {}",
                kb_core::vars::Escape::NAMES[v.escape as usize]
            ));
        }
        let arrow = match (&b.value, secret, b.source) {
            (Some(_), true, _) => {
                format!("→ {MASK}  (from ${})", v.env.as_deref().unwrap_or("env"))
            }
            (Some(x), false, s) => format!("→ {}  ({})", x.reveal(), s.name()),
            (None, true, _) => "→ (will ask)".to_string(),
            (None, false, _) => "→ (will ask)".to_string(),
        };
        parts.push(arrow);
        out.push_str(parts.join("   ").trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kb_core::entry::parse_entry;

    struct Script {
        answers: Vec<Option<String>>,
        said: Vec<String>,
        prompts: Vec<String>,
        secret_prompts: usize,
    }

    impl Script {
        fn new(answers: &[Option<&str>]) -> Script {
            Script {
                answers: answers
                    .iter()
                    .rev()
                    .map(|a| a.map(str::to_string))
                    .collect(),
                said: Vec::new(),
                prompts: Vec::new(),
                secret_prompts: 0,
            }
        }
    }

    impl Ask for Script {
        fn line(&mut self, prompt: &str) -> io::Result<Option<String>> {
            self.prompts.push(prompt.to_string());
            Ok(self.answers.pop().expect("an answer is scripted"))
        }
        fn secret(&mut self, prompt: &str) -> io::Result<Option<String>> {
            self.secret_prompts += 1;
            self.line(prompt)
        }
        fn say(&mut self, text: &str) {
            self.said.push(text.to_string());
        }
    }

    fn plan(front: &str, payload: &str) -> VarPlan {
        let text =
            format!("---\nkb_format: 1\ntype: cmd\ntitle: T\n{front}---\n\n```\n{payload}\n```\n");
        varres::plan_vars(&parse_entry("x", "x.md", text.into_bytes()).entry.unwrap())
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    const FRONT: &str = "vars:\n  env: { kind: enum, options: [prod, stage], default: stage }\n  n: { kind: number }\n";

    fn ready(c: Collected) -> (Values, Vec<Used>) {
        match c {
            Collected::Ready(v, u) => (v, u),
            Collected::Stop(_) => panic!("stopped"),
        }
    }

    #[test]
    fn enter_accepts_the_prefill_and_answers_are_checked() {
        let p = plan(FRONT, "{{env}} {{n}}");
        let mut ask = Script::new(&[Some(""), Some("abc"), Some("30")]);
        let c = collect(&p, &[], &no_env, None, Some(&mut ask)).unwrap();
        let (v, used) = ready(c);
        assert_eq!(v["env"].reveal(), "stage");
        assert_eq!(v["n"].reveal(), "30");
        assert_eq!(used[0].source, Source::Default);
        assert_eq!(used[1].source, Source::Typed);
        assert_eq!(ask.prompts[0], "env (enum: prod|stage) [stage]: ");
        assert_eq!(ask.prompts[1], "n (number): ");
        assert_eq!(ask.said, vec!["  n: expected a number like 30, -5 or 0.25"]);
    }

    #[test]
    fn flags_are_not_asked_about() {
        let p = plan(FRONT, "{{env}} {{n}}");
        let flags = vec![("env".to_string(), "prod".to_string())];
        let mut ask = Script::new(&[Some("5")]);
        let (v, _) = ready(collect(&p, &flags, &no_env, None, Some(&mut ask)).unwrap());
        assert_eq!(v["env"].reveal(), "prod");
        assert_eq!(ask.prompts.len(), 1);
    }

    #[test]
    fn giving_up_cancels() {
        let p = plan(FRONT, "{{env}} {{n}}");
        let mut ask = Script::new(&[None]);
        match collect(&p, &[], &no_env, None, Some(&mut ask)).unwrap() {
            Collected::Stop(code) => {
                assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(130)))
            }
            Collected::Ready(..) => panic!(),
        }
    }

    #[test]
    fn without_a_way_to_ask_a_variable_with_nothing_is_missing() {
        let p = plan(FRONT, "{{env}} {{n}}");
        match collect(&p, &[], &no_env, None, None).unwrap() {
            Collected::Stop(code) => {
                assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(1)))
            }
            Collected::Ready(..) => panic!(),
        }
        let flags = vec![("n".to_string(), "5".to_string())];
        let (v, _) = ready(collect(&p, &flags, &no_env, None, None).unwrap());
        assert_eq!(v["env"].reveal(), "stage");
    }

    #[test]
    fn an_invalid_flag_is_exit_2_before_anything_is_asked() {
        let p = plan(FRONT, "{{env}} {{n}}");
        let flags = vec![("n".to_string(), "abc".to_string())];
        let mut ask = Script::new(&[]);
        match collect(&p, &flags, &no_env, None, Some(&mut ask)).unwrap() {
            Collected::Stop(code) => {
                assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(2)))
            }
            Collected::Ready(..) => panic!(),
        }
        assert!(ask.prompts.is_empty());
    }

    #[test]
    fn a_secret_from_the_environment_is_not_asked_and_never_reported() {
        let p = plan("vars:\n  t: { kind: secret, env: KB_TOK }\n", "curl {{t}}");
        let env = |n: &str| (n == "KB_TOK").then(|| "S3CR3T-MARKER".to_string());
        let mut ask = Script::new(&[]);
        let (v, used) = ready(collect(&p, &[], &env, None, Some(&mut ask)).unwrap());
        assert!(ask.prompts.is_empty());
        assert_eq!(v["t"].reveal(), "S3CR3T-MARKER");
        assert_eq!(used[0].text, None);
        assert!(!format!("{used:?}{v:?}").contains("S3CR3T"));
    }

    #[test]
    fn a_secret_without_a_source_is_asked_with_masked_input() {
        let p = plan("vars:\n  t: { kind: secret }\n", "curl {{t}}");
        let mut ask = Script::new(&[Some("S3CR3T-MARKER")]);
        let (v, _) = ready(collect(&p, &[], &no_env, None, Some(&mut ask)).unwrap());
        assert_eq!(ask.secret_prompts, 1);
        assert!(v["t"].is_secret());
        assert_eq!(ask.prompts[0], "t (secret): ");
    }

    #[test]
    fn unknown_flag_names_list_the_ones_the_entry_uses() {
        let p = plan(FRONT, "{{env}} {{n}}");
        let e = check_flag_names(&p, &[("nope".into(), "1".into())]).unwrap_err();
        assert!(e.message.contains("it uses: env, n"), "{}", e.message);
        assert!(check_flag_names(&p, &[("n".into(), "1".into())]).is_ok());
    }

    #[test]
    fn flags_are_parsed_at_the_first_equals() {
        let a = VarArgs {
            vars: vec!["q=a=b".into(), "x=".into()],
            ..VarArgs::default()
        };
        let o = a.into_opts(false).unwrap();
        assert_eq!(
            o.flags,
            vec![("q".into(), "a=b".into()), ("x".into(), "".into())]
        );
        for bad in ["novalue", "Bad=1", "=x"] {
            let a = VarArgs {
                vars: vec![bad.into()],
                ..VarArgs::default()
            };
            assert!(a.into_opts(false).is_err(), "{bad}");
        }
    }

    #[test]
    fn vars_text_shows_sources_and_never_a_secret() {
        let p = plan(
            "vars:\n  env: { kind: enum, options: [prod, stage], default: stage, env: KB_ENV }\n  t: { kind: secret, env: KB_TOK }\n",
            "{{env}} {{t}}",
        );
        let env = |n: &str| (n == "KB_TOK").then(|| "S3CR3T-MARKER".to_string());
        let mem: BTreeMap<String, String> = [("env".to_string(), "prod".to_string())].into();
        let text = vars_report(&p, "x", &env, Some(&mem), false);
        assert!(text.contains("→ prod  (remembered)"), "{text}");
        assert!(text.contains("→ ••••  (from $KB_TOK)"), "{text}");
        assert!(text.contains("env: KB_ENV (unset)"), "{text}");
        assert!(!text.contains("S3CR3T"));
        let json = vars_report(&p, "x", &env, Some(&mem), true);
        assert!(!json.contains("S3CR3T"));
        let doc: Json = serde_json::from_str(&json).unwrap();
        assert_eq!(doc["variables"][1]["env"]["set"], true);
        assert_eq!(doc["variables"][1]["prefill"]["value"], Json::Null);
    }
}
