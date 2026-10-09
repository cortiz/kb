use super::*;
use crate::entry::parse_entry;

fn entry_of(ty: &str, front: &str, body: &str) -> Entry {
    let text = format!("---\nkb_format: 1\ntype: {ty}\ntitle: T\n{front}---\n\n{body}");
    parse_entry("x", "x.md", text.into_bytes()).entry.unwrap()
}

fn cmd(front: &str, payload: &str) -> Entry {
    entry_of("cmd", front, &format!("```\n{payload}\n```\n"))
}

fn plan(e: &Entry) -> VarPlan {
    plan_vars(e)
}

fn names(p: &VarPlan) -> Vec<&str> {
    p.vars.iter().map(|v| v.var.name.as_str()).collect()
}

fn values(pairs: &[(&str, &str)]) -> Values {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Value::plain(*v)))
        .collect()
}

fn text_of(r: &Resolved) -> &str {
    &r.primary().unwrap().text
}

// ---- planning -----------------------------------------------------------------------------

#[test]
fn order_is_first_appearance_in_the_file() {
    // `url`-like fields come before the payload; declaration order does not matter
    let e = entry_of(
        "cw",
        "log_groups: [\"/aws/{{svc}}\"]\nvars:\n  later: { kind: text }\n  svc: { kind: text }\n",
        "```\nfields {{later}} {{svc}}\n```\n",
    );
    assert_eq!(names(&plan(&e)), vec!["svc", "later"]);
    let e = cmd(
        "vars:\n  b: { kind: text }\n  a: { kind: text }\n",
        "echo {{a}} {{b}} {{a}}",
    );
    assert_eq!(names(&plan(&e)), vec!["a", "b"]);
}

#[test]
fn a_repeated_variable_is_one_field_with_all_locations() {
    let e = cmd("cwd: \"{{d}}\"\n", "ls {{d}} {{d}}");
    let p = plan(&e);
    assert_eq!(names(&p), vec!["d"]);
    assert_eq!(p.vars[0].locations, vec![Target::Payload, Target::Cwd]);
}

#[test]
fn unreferenced_declared_variables_are_left_out() {
    let e = cmd("vars:\n  unused: { kind: text }\n", "echo hi");
    assert!(plan(&e).is_empty());
}

#[test]
fn undeclared_is_text_without_default() {
    let e = cmd("", "echo {{who}}");
    let v = &plan(&e).vars[0].var;
    assert_eq!(
        (v.kind, v.declared, v.default.clone()),
        (VarKind::Text, false, None)
    );
}

#[test]
fn escaped_braces_are_not_variables_and_come_out_literal() {
    let e = cmd("", "echo \\{{lit}}");
    let p = plan(&e);
    assert!(p.is_empty());
}

#[test]
fn notes_are_never_scanned() {
    let e = entry_of("cmd", "", "```\nls\n```\n\nSee {{notes_only}}.\n");
    assert!(plan(&e).is_empty());
}

#[test]
fn the_signature_follows_the_variables_and_the_text() {
    let a = plan(&cmd("vars:\n  a: { kind: text }\n", "echo {{a}}"));
    let same = plan(&cmd("vars:\n  a: { kind: text }\n", "echo {{a}}"));
    let kind = plan(&cmd("vars:\n  a: { kind: number }\n", "echo {{a}}"));
    let text = plan(&cmd("vars:\n  a: { kind: text }\n", "echo -n {{a}}"));
    assert_eq!(a.signature, same.signature);
    assert_ne!(a.signature, kind.signature);
    assert_ne!(a.signature, text.signature);
}

// ---- prefill ------------------------------------------------------------------------------

fn prefilled(
    e: &Entry,
    flags: &[(&str, &str)],
    env: &[(&str, &str)],
    mem: &[(&str, &str)],
) -> Vec<Binding> {
    let flags: Vec<(String, String)> = flags
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let env: BTreeMap<String, String> = env
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let mem: BTreeMap<String, String> = mem
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let lookup = move |n: &str| env.get(n).cloned();
    prefill(
        &plan(e),
        &Inputs {
            flags: &flags,
            env: &lookup,
            memory: Some(&mem),
        },
    )
}

fn env_entry() -> Entry {
    cmd(
        "vars:\n  env: { kind: enum, options: [prod, stage, dev], default: stage, env: KB_ENV }\n",
        "x {{env}}",
    )
}

fn got(b: &Binding) -> (Option<&str>, Source) {
    (b.value.as_ref().map(Value::reveal), b.source)
}

#[test]
fn prefill_order_flag_env_remembered_default_none() {
    let e = env_entry();
    let all = prefilled(
        &e,
        &[("env", "dev")],
        &[("KB_ENV", "prod")],
        &[("env", "prod")],
    );
    assert_eq!(got(&all[0]), (Some("dev"), Source::Flag));
    let env = prefilled(&e, &[], &[("KB_ENV", "prod")], &[("env", "dev")]);
    assert_eq!(got(&env[0]), (Some("prod"), Source::Env));
    let mem = prefilled(&e, &[], &[], &[("env", "dev")]);
    assert_eq!(got(&mem[0]), (Some("dev"), Source::Remembered));
    let def = prefilled(&e, &[], &[], &[]);
    assert_eq!(got(&def[0]), (Some("stage"), Source::Default));
    let none = prefilled(&cmd("vars:\n  a: { kind: text }\n", "{{a}}"), &[], &[], &[]);
    assert_eq!(got(&none[0]), (None, Source::None));
}

#[test]
fn an_empty_environment_value_is_skipped() {
    let b = prefilled(&env_entry(), &[], &[("KB_ENV", "")], &[]);
    assert_eq!(got(&b[0]), (Some("stage"), Source::Default));
}

#[test]
fn a_remembered_value_that_is_no_longer_valid_is_ignored() {
    let b = prefilled(&env_entry(), &[], &[], &[("env", "qa")]);
    assert_eq!(got(&b[0]), (Some("stage"), Source::Default));
    let e = cmd("vars:\n  n: { kind: number, default: 15 }\n", "{{n}}");
    let b = prefilled(&e, &[], &[], &[("n", "many")]);
    assert_eq!(got(&b[0]), (Some("15"), Source::Default));
}

#[test]
fn an_invalid_declared_default_is_ignored() {
    for bad in ["1e3", ".5", "5."] {
        let e = cmd(
            &format!("vars:\n  n: {{ kind: number, default: \"{bad}\" }}\n"),
            "{{n}}",
        );
        let b = prefilled(&e, &[], &[], &[]);
        assert_eq!(got(&b[0]), (None, Source::None), "{bad}");
    }
}

#[test]
fn an_empty_default_is_a_real_default_for_text() {
    let e = cmd("vars:\n  s: { kind: text, default: \"\" }\n", "x{{s}}");
    let b = prefilled(&e, &[], &[], &[]);
    assert_eq!(got(&b[0]), (Some(""), Source::Default));
}

#[test]
fn a_secret_takes_flag_or_env_only() {
    let e = cmd(
        "vars:\n  t: { kind: secret, env: KB_TOK }\n",
        "curl -H {{t}}",
    );
    let b = prefilled(&e, &[], &[], &[("t", "remembered")]);
    assert_eq!(got(&b[0]), (None, Source::None));
    let b = prefilled(&e, &[], &[("KB_TOK", "S3CR3T")], &[]);
    assert_eq!(got(&b[0]), (Some("S3CR3T"), Source::Env));
    assert!(b[0].secret && b[0].value.as_ref().unwrap().is_secret());
    assert!(!format!("{b:?}").contains("S3CR3T"));
}

// ---- validation ---------------------------------------------------------------------------

fn var(kind: VarKind, options: &[&str], default: Option<&str>) -> Variable {
    Variable {
        name: "v".into(),
        kind,
        declared: true,
        default: default.map(str::to_string),
        options: options.iter().map(|s| s.to_string()).collect(),
        description: None,
        env: None,
        escape: Escape::Auto,
        references: Vec::new(),
    }
}

fn ok(v: &Variable, s: &str) -> bool {
    validate(v, &Value::new(s, v.kind == VarKind::Secret)).is_ok()
}

#[test]
fn text_is_a_single_line_and_required_unless_the_default_is_empty() {
    let v = var(VarKind::Text, &[], None);
    assert!(ok(&v, "hello world"));
    assert!(!ok(&v, ""));
    assert!(!ok(&v, "a\nb"));
    assert!(!ok(&v, "a\rb"));
    assert!(!ok(&v, "a\0b"));
    assert!(ok(&var(VarKind::Text, &[], Some("")), ""));
    assert!(!ok(&var(VarKind::Text, &[], Some("x")), ""));
}

#[test]
fn numbers_are_integers_or_decimals() {
    let v = var(VarKind::Number, &[], None);
    for good in ["30", "-5", "0.25", "0", "007", "-0.5"] {
        assert!(ok(&v, good), "{good}");
    }
    for bad in [
        "abc", "1e3", ".5", "5.", "+3", " 3", "3 ", "--3", "1.2.3", "", "-", "١٢",
    ] {
        assert!(!ok(&v, bad), "{bad:?}");
    }
}

#[test]
fn enums_compare_exactly() {
    let v = var(VarKind::Enum, &["prod", "stage"], None);
    assert!(ok(&v, "prod"));
    assert!(!ok(&v, "Prod"));
    assert!(!ok(&v, "prod "));
    let e = validate(&v, &Value::plain("qa")).unwrap_err();
    assert_eq!(e.to_string(), "v: expected one of prod, stage");
}

#[test]
fn secrets_are_non_empty_single_line_and_errors_do_not_echo_them() {
    let v = var(VarKind::Secret, &[], None);
    assert!(ok(&v, "x"));
    assert!(!ok(&v, ""));
    let e = validate(&v, &Value::secret("S3CR3T\nMARKER")).unwrap_err();
    assert!(!e.to_string().contains("S3CR3T"));
    let n = validate(&var(VarKind::Number, &[], None), &Value::plain("abc")).unwrap_err();
    assert_eq!(n.to_string(), "v: expected a number like 30, -5 or 0.25");
}

// ---- resolve: substitution ----------------------------------------------------------------

#[test]
fn substitution_is_one_pass_and_values_are_never_expanded() {
    let e = cmd(
        "vars:\n  a: { kind: text, escape: none }\n  b: { kind: text, escape: none }\n",
        "echo {{a}} {{b}}",
    );
    let r = resolve(&plan(&e), &values(&[("a", "{{b}}"), ("b", "B")])).unwrap();
    assert_eq!(text_of(&r), "echo {{b}} B\n");
}

#[test]
fn escaped_braces_come_out_as_literal_braces() {
    let e = cmd("vars:\n  a: { kind: text }\n", "echo \\{{lit}} {{a}}");
    let r = resolve(&plan(&e), &values(&[("a", "x")])).unwrap();
    assert_eq!(text_of(&r), "echo {{lit}} x\n");
}

#[test]
fn a_repeated_variable_is_substituted_everywhere_and_spans_point_at_it() {
    let e = cmd("vars:\n  a: { kind: text }\n", "{{a}}-{{a}}");
    let r = resolve(&plan(&e), &values(&[("a", "xy")])).unwrap();
    let o = r.primary().unwrap();
    assert_eq!(o.text, "xy-xy\n");
    let spans: Vec<_> = o.spans.iter().map(|s| &o.text[s.range.clone()]).collect();
    assert_eq!(spans, vec!["xy", "xy"]);
}

#[test]
fn fields_are_resolved_too() {
    let e = entry_of(
        "cw",
        "log_groups: [\"/aws/{{svc}}\"]\nrange: \"{{win}}\"\nvars:\n  svc: { kind: text }\n  win: { kind: text }\n",
        "```\nfields @message\n```\n",
    );
    let r = resolve(&plan(&e), &values(&[("svc", "api"), ("win", "15m")])).unwrap();
    assert_eq!(r.get(&Target::LogGroup(0)).unwrap().text, "/aws/api");
    assert_eq!(r.get(&Target::Range).unwrap().text, "15m");
    assert_eq!(r.get(&Target::Payload).unwrap().text, "fields @message\n");
}

#[test]
fn all_errors_are_returned_and_nothing_is_resolved() {
    let e = cmd(
        "vars:\n  n: { kind: number }\n  m: { kind: number }\n",
        "{{n}} {{m}} {{missing}}",
    );
    let errs = resolve(&plan(&e), &values(&[("n", "x"), ("m", "1")])).unwrap_err();
    let msgs: Vec<_> = errs.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(msgs, vec!["n", "missing"]);
}

#[test]
fn resolving_is_deterministic() {
    let e = cmd("vars:\n  a: { kind: text }\n", "echo {{a}}");
    let v = values(&[("a", "a b")]);
    assert_eq!(resolve(&plan(&e), &v), resolve(&plan(&e), &v));
}

#[test]
fn secrets_are_flagged_and_redacted_for_display() {
    let e = cmd(
        "vars:\n  t: { kind: secret }\n  u: { kind: text }\n",
        "curl {{u}} -H {{t}}",
    );
    let mut v = values(&[("u", "host")]);
    v.insert("t".into(), Value::secret("S3CR3T"));
    let r = resolve(&plan(&e), &v).unwrap();
    assert!(r.has_secret);
    assert_eq!(text_of(&r), "curl host -H S3CR3T\n");
    assert_eq!(r.primary().unwrap().redacted(), "curl host -H ••••\n");
}

// ---- resolve: escaping --------------------------------------------------------------------

fn one(ty: &str, front: &str, body: &str, v: &str) -> String {
    let e = entry_of(ty, front, body);
    let r = resolve(&plan(&e), &values(&[("a", v)])).unwrap();
    r.primary().unwrap().text.clone()
}

const A: &str = "vars:\n  a: { kind: text }\n";

#[test]
fn cmd_values_are_quoted_by_where_they_land() {
    let c = |payload: &str, v: &str| one("cmd", A, &format!("```\n{payload}\n```\n"), v);
    assert_eq!(c("echo {{a}}", "plain"), "echo plain\n");
    assert_eq!(
        c("echo {{a}}", "feature/x; rm -rf ~"),
        "echo 'feature/x; rm -rf ~'\n"
    );
    assert_eq!(
        c("echo \"{{a}}\"", "a b $HOME \"q\""),
        "echo \"a b \\$HOME \\\"q\\\"\"\n"
    );
    assert_eq!(c("echo '{{a}}'", "it's"), "echo 'it'\\''s'\n");
    assert_eq!(c("echo \"x\" {{a}}", "a b"), "echo \"x\" 'a b'\n");
    assert_eq!(c("echo {{a}} {{a}}", "a b"), "echo 'a b' 'a b'\n");
}

#[test]
fn fish_uses_its_own_quoting() {
    let f = "shell: fish\nvars:\n  a: { kind: text }\n";
    assert_eq!(
        one("cmd", f, "```\necho {{a}}\n```\n", "it's"),
        "echo 'it\\'s'\n"
    );
    assert_eq!(
        one("cmd", f, "```\necho '{{a}}'\n```\n", "a\\b"),
        "echo 'a\\\\b'\n"
    );
}

#[test]
fn cwd_is_quoted_and_urls_are_percent_encoded() {
    let e = cmd("cwd: \"{{a}}\"\nvars:\n  a: { kind: text }\n", "ls");
    // a variable only in cwd is in the plan; the payload has none
    let r = resolve(&plan(&e), &values(&[("a", "my dir")])).unwrap();
    assert_eq!(r.get(&Target::Cwd).unwrap().text, "'my dir'");
    let link = entry_of(
        "link",
        "url: \"https://x.test/?q={{a}}\"\nvars:\n  a: { kind: text }\n",
        "",
    );
    let r = resolve(&plan(&link), &values(&[("a", "a&b c")])).unwrap();
    assert_eq!(text_of(&r), "https://x.test/?q=a%26b%20c");
}

#[test]
fn other_payloads_get_values_as_typed() {
    for ty in ["sql", "sumo", "prompt"] {
        let body = "```\nselect {{a}}\n```\n";
        assert_eq!(one(ty, A, body, "it's a b"), "select it's a b\n", "{ty}");
    }
    let cw = entry_of(
        "cw",
        "log_groups: [\"/g/{{a}}\"]\nvars:\n  a: { kind: text }\n",
        "```\nx\n```\n",
    );
    let r = resolve(&plan(&cw), &values(&[("a", "a b")])).unwrap();
    assert_eq!(r.get(&Target::LogGroup(0)).unwrap().text, "/g/a b");
}

#[test]
fn escape_modes_apply_everywhere() {
    let sql = "vars:\n  a: { kind: text, escape: sql }\n";
    assert_eq!(
        one("sql", sql, "```\nwhere n = {{a}}\n```\n", "O'Neil"),
        "where n = 'O''Neil'\n"
    );
    let sh = "vars:\n  a: { kind: text, escape: shell }\n";
    assert_eq!(
        one("prompt", sh, "```\nrun {{a}}\n```\n", "a b"),
        "run 'a b'\n"
    );
    assert_eq!(
        one("cmd", sh, "```\nrun \"{{a}}\"\n```\n", "a b"),
        "run \"'a b'\"\n"
    );
    let none = "vars:\n  a: { kind: text, escape: none }\n";
    assert_eq!(
        one("cmd", none, "```\nrun {{a}}\n```\n", "a; b"),
        "run a; b\n"
    );
    let auto = "vars:\n  a: { kind: text, escape: auto }\n";
    assert_eq!(
        one("cmd", auto, "```\nrun {{a}}\n```\n", "a; b"),
        "run 'a; b'\n"
    );
}

#[test]
fn a_value_cannot_change_the_quote_state_for_what_follows() {
    let e = cmd(
        "vars:\n  a: { kind: text }\n  b: { kind: text }\n",
        "echo {{a}} {{b}}",
    );
    let r = resolve(&plan(&e), &values(&[("a", "\"'"), ("b", "x y")])).unwrap();
    assert_eq!(text_of(&r), "echo '\"'\\''' 'x y'\n");
}

// ---- hostile values through real shells ---------------------------------------------------

const HOSTILE: &[&str] = &[
    "a b",
    "a'b",
    "a\"b",
    "a`b`",
    "$(touch PWNED)",
    "${HOME}",
    "a;b",
    "a&&b",
    "a|b",
    "a>b",
    "*",
    "~",
    "-rf",
    "\\",
    "''",
    "{{x}}",
    "é日本",
    "feature/x; rm -rf ~",
    "a&b c",
    "$(touch PWNED); x",
    "`touch PWNED`",
    "a\\'b",
    "'",
    "\"",
    "x\\",
    "!",
];

fn have(shell: &str) -> bool {
    std::process::Command::new(shell)
        .arg("-c")
        .arg("true")
        .output()
        .is_ok()
}

fn run_shell(shell: &str, script: &str, dir: &std::path::Path) -> String {
    let out = std::process::Command::new(shell)
        .arg("-c")
        .arg(script)
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn quoted_values_arrive_as_one_argument_and_run_nothing() {
    let long = "x'\"$ `".repeat(2000);
    for shell in [Shell::Sh, Shell::Bash, Shell::Zsh, Shell::Fish] {
        let name = format!("{shell:?}").to_lowercase();
        if !have(&name) {
            eprintln!("skipping {name}: not installed");
            continue;
        }
        let dir = tempfile::tempdir().unwrap();
        for v in HOSTILE.iter().copied().chain([long.as_str()]) {
            for (ctx, payload) in [
                ("unquoted", "printf %s {{a}}"),
                ("single", "printf %s '{{a}}'"),
                ("double", "printf %s \"{{a}}\""),
            ] {
                // a single quoted fish string does not process escapes except \\ and \'
                let e = cmd(
                    &format!("shell: {name}\nvars:\n  a: {{ kind: text }}\n"),
                    payload,
                );
                let r = resolve(&plan(&e), &values(&[("a", v)])).unwrap();
                let script = r.primary().unwrap().text.clone();
                let out = run_shell(&name, &script, dir.path());
                assert_eq!(out, v, "{name} {ctx}: {script:?}");
            }
        }
        assert!(
            !dir.path().join("PWNED").exists(),
            "{name}: a value ran a command"
        );
    }
}

#[test]
fn a_preview_keeps_placeholders_for_missing_or_invalid_values() {
    let e = cmd(
        "vars:\n  n: { kind: number }\n  a: { kind: text }\n  t: { kind: secret }\n",
        "x {{n}} {{a}} {{t}}",
    );
    let p = plan(&e);
    let mut v = values(&[("n", "abc"), ("a", "b c")]);
    v.insert("t".into(), Value::secret("S3CR3T"));
    let r = preview(&p, &v);
    assert_eq!(r.primary().unwrap().text, "x {{n}} 'b c' S3CR3T\n");
    let (shown, marks) = r.primary().unwrap().display();
    assert_eq!(shown, "x {{n}} 'b c' ••••\n");
    let marked: Vec<_> = marks.iter().map(|m| &shown[m.clone()]).collect();
    assert_eq!(marked, vec!["'b c'", "••••"]);
    assert!(resolve(&p, &v).is_err());
}
