//! Canonical rendering of a new entry (FR-018, research R5) and save-time checking (R6).

use std::time::{SystemTime, UNIX_EPOCH};

use crate::author::Clock;
use crate::author::draft::{Draft, DraftFields};
use crate::edit::{double_quoted, is_plain_safe};
use crate::entry::EntryType;
use crate::lint::{Finding, Severity, check_text, rules::MAX_FILE_BYTES};
use crate::vars::VarKind;

fn kind_name(kind: VarKind) -> &'static str {
    match kind {
        VarKind::Text => "text",
        VarKind::Number => "number",
        VarKind::Enum => "enum",
        VarKind::Secret => "secret",
    }
}

fn longest_backtick_run(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

/// A YAML value in block context: plain when it reads back unchanged, else double-quoted (the
/// same rules `set_field` uses).
fn scalar(s: &str) -> String {
    if is_plain_safe(s) {
        s.to_string()
    } else {
        double_quoted(s)
    }
}

/// A YAML value inside `[…]` or `{…}`, where commas and brackets would end it.
fn flow_scalar(s: &str) -> String {
    if is_plain_safe(s) && !s.contains([',', '[', ']', '{', '}']) {
        s.to_string()
    } else {
        double_quoted(s)
    }
}

fn opt_line(out: &mut String, key: &str, value: &Option<String>) {
    if let Some(v) = value.as_ref().filter(|v| !v.is_empty()) {
        out.push_str(&format!("{key}: {}\n", scalar(v)));
    }
}

/// The type-specific keys, in the order of `EntryType::keys()`.
fn type_fields(fields: &DraftFields, out: &mut String) {
    match fields {
        DraftFields::None => {}
        DraftFields::Sql { dialect, database } => {
            opt_line(out, "dialect", dialect);
            opt_line(out, "database", database);
        }
        DraftFields::Sumo { range } => opt_line(out, "range", range),
        DraftFields::Cw {
            log_groups,
            range,
            region,
        } => {
            if !log_groups.is_empty() {
                let groups: Vec<String> = log_groups.iter().map(|g| flow_scalar(g)).collect();
                out.push_str(&format!("log_groups: [{}]\n", groups.join(", ")));
            }
            opt_line(out, "range", range);
            opt_line(out, "region", region);
        }
        DraftFields::Cmd { shell, cwd } => {
            opt_line(out, "shell", shell);
            opt_line(out, "cwd", cwd);
        }
        DraftFields::Link { url } => out.push_str(&format!("url: {}\n", scalar(url))),
        DraftFields::Code {
            repo,
            git_ref,
            path,
            line,
            end_line,
            symbol,
        } => {
            out.push_str("code:\n");
            out.push_str(&format!(
                "  repo: {}\n  path: {}\n  line: {line}\n",
                scalar(repo),
                scalar(path)
            ));
            if let Some(end) = end_line {
                out.push_str(&format!("  end_line: {end}\n"));
            }
            out.push_str(&format!("  ref: {}\n", scalar(git_ref)));
            if let Some(symbol) = symbol.as_ref().filter(|s| !s.is_empty()) {
                out.push_str(&format!("  symbol: {}\n", scalar(symbol)));
            }
        }
    }
}

/// What is wrong with the bytes a draft would write.
#[derive(Debug, Clone)]
pub struct Check {
    pub id: String,
    pub bytes: Vec<u8>,
    pub findings: Vec<Finding>,
    /// The rendered file is over [`MAX_FILE_BYTES`]; this milestone refuses to write it.
    pub too_large: bool,
}

fn is_secret(f: &Finding) -> bool {
    (60..=65).contains(&f.code.number())
}

impl Check {
    /// Likely secrets (`KB060` to `KB065`, not suppressed with `kb:allow-secret`).
    pub fn secrets(&self) -> Vec<&Finding> {
        self.findings.iter().filter(|f| is_secret(f)).collect()
    }

    /// Errors that are not secrets.
    pub fn errors(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error && !is_secret(f))
            .collect()
    }

    /// Whether the bytes may be written. Secrets and oversize files never; other errors only when
    /// the user chose "save anyway".
    pub fn writable(&self, save_anyway: bool) -> bool {
        if self.too_large || !self.secrets().is_empty() {
            return false;
        }
        save_anyway || self.errors().is_empty()
    }
}

/// `2026-10-09T12:00:00Z`: UTC, whole seconds.
pub fn rfc3339_utc(t: SystemTime) -> String {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX),
    };
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Reads `YYYY-MM-DDTHH:MM:SSZ` (UTC, whole seconds, nothing else) back into a time. For fixing
/// the clock in tests.
pub fn parse_rfc3339_utc(s: &str) -> Option<SystemTime> {
    let b = s.as_bytes();
    let shaped = b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z';
    if !shaped {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        s.get(from..to)?
            .bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| s[from..to].parse().ok())
            .flatten()
    };
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    // days from civil (Howard Hinnant)
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = u64::try_from(days * 86_400 + h * 3600 + mi * 60 + sec).ok()?;
    Some(UNIX_EPOCH + std::time::Duration::from_secs(secs))
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

impl Draft {
    /// The exact bytes of the new file, in the canonical layout.
    pub fn render(&self, clock: &dyn Clock) -> Vec<u8> {
        let now = rfc3339_utc(clock.now());
        let mut out = String::from("---\nkb_format: 1\n");
        out.push_str(&format!("type: {}\n", self.entry_type.name()));
        out.push_str(&format!("title: {}\n", scalar(&self.title)));
        if !self.tags.is_empty() {
            let tags: Vec<String> = self.tags.iter().map(|t| flow_scalar(t)).collect();
            out.push_str(&format!("tags: [{}]\n", tags.join(", ")));
        }
        type_fields(&self.fields, &mut out);
        if self.entry_type.has_payload() && !self.vars.is_empty() {
            out.push_str("vars:\n");
            for v in &self.vars {
                let mut parts = vec![format!("kind: {}", kind_name(v.kind))];
                if v.kind == VarKind::Enum && !v.options.is_empty() {
                    let options: Vec<String> = v.options.iter().map(|o| flow_scalar(o)).collect();
                    parts.push(format!("options: [{}]", options.join(", ")));
                }
                if let Some(default) = &v.default {
                    let default = match v.kind {
                        VarKind::Number if default.parse::<f64>().is_ok() => default.clone(),
                        _ => flow_scalar(default),
                    };
                    parts.push(format!("default: {default}"));
                }
                out.push_str(&format!("  {}: {{ {} }}\n", v.name, parts.join(", ")));
            }
        }
        out.push_str(&format!("created: {now}\nupdated: {now}\n---\n"));
        if self.entry_type.has_payload() {
            let longest = longest_backtick_run(&self.body);
            let fence = "`".repeat((longest + 1).max(3));
            let info = match (self.entry_type, &self.fields) {
                (EntryType::Sql, _) => "sql",
                (
                    EntryType::Cmd,
                    DraftFields::Cmd {
                        shell: Some(shell), ..
                    },
                ) => shell,
                (EntryType::Cmd, _) => "sh",
                _ => "",
            };
            out.push_str(&format!("\n{fence}{info}\n"));
            if !self.body.is_empty() {
                out.push_str(&self.body);
                out.push('\n');
            }
            out.push_str(&format!("{fence}\n"));
        }
        let notes = self.notes.replace("\r\n", "\n");
        let notes = notes.trim_start_matches('\n').trim_end();
        if !notes.trim().is_empty() {
            out.push_str(&format!("\n{notes}\n"));
        }
        out.into_bytes()
    }

    /// The file `kb add --editor` opens: the canonical layout with the required fields in place,
    /// the optional type fields as `# key: value` comments, and an empty fence to fill in.
    pub fn template(&self, clock: &dyn Clock) -> String {
        let mut draft = self.clone();
        // placeholders where the lint step would otherwise only find empty strings
        if let DraftFields::Code {
            repo,
            git_ref,
            path,
            ..
        } = &mut draft.fields
        {
            for (field, placeholder) in [
                (repo, "owner/name"),
                (git_ref, "main"),
                (path, "src/file.rs"),
            ] {
                if field.is_empty() {
                    *field = placeholder.into();
                }
            }
        }
        let text = String::from_utf8_lossy(&draft.render(clock)).into_owned();
        let comments: &[&str] = match draft.entry_type {
            EntryType::Sql => &["# dialect: postgres", "# database: mydb"],
            EntryType::Sumo => &["# range: -15m"],
            EntryType::Cw => &[
                "# log_groups: [/aws/lambda/name]",
                "# range: -1h",
                "# region: us-east-1",
            ],
            EntryType::Cmd => &["# shell: bash", "# cwd: ~/work"],
            EntryType::Code => &["  # symbol: Class.method", "  # end_line: 20"],
            EntryType::Prompt | EntryType::Link => &[],
        };
        let at = text
            .find("\nvars:")
            .or_else(|| text.find("\ncreated:"))
            .map_or(text.len(), |i| i + 1);
        let mut out = String::with_capacity(text.len() + 120);
        out.push_str(&text[..at]);
        for line in comments {
            // optional keys the draft already holds are not offered again
            let key = line
                .trim_start_matches([' ', '#'])
                .split(':')
                .next()
                .unwrap_or("");
            if !text[..at].contains(&format!("{key}:")) {
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push_str(&text[at..]);
        out
    }

    /// Renders, then runs the checks `kb lint` runs on the bytes, as the entry `id`.
    pub fn check(&self, id: &str, clock: &dyn Clock) -> Check {
        let bytes = self.render(clock);
        Check {
            id: id.to_string(),
            too_large: bytes.len() > MAX_FILE_BYTES,
            findings: check_text(id, bytes.clone()),
            bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::author::FixedClock;
    use crate::author::draft::{DraftOptions, DraftSource};
    use crate::entry::parse_entry;
    use crate::lint::RuleCode;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    const NOW: u64 = 1_791_547_200; // 2026-10-09T12:00:00Z

    fn clock() -> FixedClock {
        FixedClock(UNIX_EPOCH + Duration::from_secs(NOW))
    }

    fn golden_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/authoring/golden")
    }

    /// Builds the draft a golden input describes: first line `# opts: title=…; tags=a,b; type=…; path=…; notes=…`.
    fn golden_draft(input: &str) -> Draft {
        let (header, body) = input.split_once('\n').unwrap();
        let opts_text = header
            .strip_prefix("# opts:")
            .expect("golden header")
            .trim();
        let mut opts = DraftOptions::default();
        let mut notes = String::new();
        for pair in opts_text.split(';') {
            let (k, v) = pair.split_once('=').unwrap();
            let v = v.trim().to_string();
            match k.trim() {
                "title" => opts.title = Some(v),
                "type" => opts.ty = EntryType::from_name(&v),
                "tags" => opts.tags = v.split(',').map(str::to_string).collect(),
                "path" => opts.path = Some(v),
                "notes" => notes = v,
                other => panic!("unknown golden option {other}"),
            }
        }
        let mut draft = Draft::from_text(body, DraftSource::Stdin, &opts);
        draft.notes = notes;
        draft
    }

    fn goldens() -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(golden_dir()).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".in.txt") {
                let want =
                    std::fs::read_to_string(golden_dir().join(format!("{stem}.md"))).unwrap();
                out.push((
                    stem.to_string(),
                    std::fs::read_to_string(&path).unwrap(),
                    want,
                ));
            }
        }
        out.sort();
        out
    }

    fn text(d: &Draft) -> String {
        String::from_utf8(d.render(&clock())).unwrap()
    }

    fn sql(body: &str) -> Draft {
        let opts = DraftOptions {
            title: Some("T".into()),
            ty: Some(EntryType::Sql),
            ..Default::default()
        };
        Draft::from_text(body, DraftSource::Stdin, &opts)
    }

    #[test]
    fn dates_are_utc_civil_time() {
        assert_eq!(
            rfc3339_utc(UNIX_EPOCH + Duration::from_secs(NOW)),
            "2026-10-09T12:00:00Z"
        );
        assert_eq!(rfc3339_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            rfc3339_utc(UNIX_EPOCH + Duration::from_secs(951_782_400 + 86_399)),
            "2000-02-29T23:59:59Z"
        );
        assert_eq!(
            rfc3339_utc(UNIX_EPOCH + Duration::from_secs(4_102_444_800)),
            "2100-01-01T00:00:00Z"
        );
    }

    #[test]
    fn a_fixed_clock_reads_back_what_was_written() {
        for secs in [0, NOW, 951_782_400 + 86_399, 4_102_444_800, 1_709_164_800] {
            let t = UNIX_EPOCH + Duration::from_secs(secs);
            assert_eq!(parse_rfc3339_utc(&rfc3339_utc(t)), Some(t), "{secs}");
        }
        for bad in [
            "",
            "2026-10-09",
            "2026-10-09T12:00:00",
            "2026-13-09T12:00:00Z",
            "2026-10-09T25:00:00Z",
            "2026-10-09 12:00:00Z",
            "abcd-10-09T12:00:00Z",
        ] {
            assert_eq!(parse_rfc3339_utc(bad), None, "{bad}");
        }
    }

    fn template_of(ty: EntryType, body: &str) -> (Draft, String) {
        let opts = DraftOptions {
            ty: Some(ty),
            ..Default::default()
        };
        let d = Draft::from_text(body, DraftSource::Empty, &opts);
        let t = d.template(&clock());
        (d, t)
    }

    #[test]
    fn templates_have_every_required_field_and_the_optional_ones_as_comments() {
        for ty in EntryType::ALL {
            let (_, t) = template_of(ty, "");
            let entry = parse_entry("new/x", "new/x.md", t.clone().into_bytes())
                .entry
                .unwrap_or_else(|| panic!("{} did not parse:\n{t}", ty.name()));
            assert_eq!(entry.entry_type(), Some(ty));
            assert!(
                t.contains("kb_format: 1\n")
                    && t.contains(&format!("created: {}", "2026-10-09T12:00:00Z")),
                "{t}"
            );
            assert!(t.contains("title:"), "{t}");
            let optional = match ty {
                EntryType::Sql => "# dialect: ",
                EntryType::Sumo => "# range: ",
                EntryType::Cw => "# log_groups: ",
                EntryType::Cmd => "# shell: ",
                EntryType::Link => "url: ",
                EntryType::Code => "# symbol: ",
                EntryType::Prompt => "type: prompt",
            };
            assert!(t.contains(optional), "{}:\n{t}", ty.name());
            assert_eq!(t.contains("```"), ty.has_payload(), "{}:\n{t}", ty.name());
        }
    }

    #[test]
    fn a_template_reaches_the_lint_step_and_keeps_what_was_given() {
        let opts = DraftOptions {
            title: Some("My title".into()),
            tags: vec!["a".into()],
            ty: Some(EntryType::Sql),
            ..Default::default()
        };
        let d = Draft::from_text("select 1 from t;", DraftSource::Stdin, &opts);
        let t = d.template(&clock());
        assert!(
            t.contains("title: My title\n")
                && t.contains("tags: [a]\n")
                && t.contains("select 1 from t;"),
            "{t}"
        );
        let findings = check_text("sql/my-title", t.into_bytes());
        assert!(
            findings.iter().all(|f| f.severity != Severity::Error),
            "{findings:?}"
        );
        // an untouched blank template has errors (no title), which is what "fill it in" means
        let (_, blank) = template_of(EntryType::Cmd, "");
        assert!(
            check_text("cmd/x", blank.into_bytes())
                .iter()
                .any(|f| f.severity == Severity::Error)
        );
    }

    #[test]
    fn every_golden_input_renders_byte_for_byte() {
        let all = goldens();
        assert_eq!(all.len(), 7, "one golden per type");
        for (name, input, want) in all {
            let got = text(&golden_draft(&input));
            assert_eq!(got, want, "{name}");
        }
    }

    #[test]
    fn rendered_files_round_trip_and_pass_lint() {
        for (name, input, _) in goldens() {
            let d = golden_draft(&input);
            let id = d.target_id(&|_| false).unwrap();
            let bytes = d.render(&clock());
            let parsed = parse_entry(&id, &format!("{id}.md"), bytes.clone());
            let entry = parsed.entry.expect(&name);
            assert_eq!(entry.to_bytes(), bytes, "{name}");
            let check = d.check(&id, &clock());
            assert!(check.findings.is_empty(), "{name}: {:?}", check.findings);
            assert!(check.writable(false), "{name}");
            assert!(!check.too_large);
        }
    }

    #[test]
    fn created_equals_updated_on_creation() {
        let t = text(&sql("select 1 from t;"));
        let created = t.lines().find(|l| l.starts_with("created: ")).unwrap();
        let updated = t.lines().find(|l| l.starts_with("updated: ")).unwrap();
        assert_eq!(created["created: ".len()..], updated["updated: ".len()..]);
    }

    #[test]
    fn the_fence_is_longer_than_any_backtick_run_in_the_body() {
        let t = text(&sql("select '```' from t;"));
        assert!(t.contains("\n````sql\nselect '```' from t;\n````\n"), "{t}");
        let t = text(&sql("a `````b"));
        assert!(t.contains("``````sql\n"), "{t}");
        let d = sql("a ```\nb\n```");
        let bytes = d.render(&clock());
        let parsed = parse_entry("sql/t", "sql/t.md", bytes).entry.unwrap();
        assert_eq!(parsed.payload().unwrap().text.trim_end(), "a ```\nb\n```");
    }

    #[test]
    fn titles_that_need_quotes_are_quoted() {
        for (title, line) in [
            ("Incident: five", "title: \"Incident: five\""),
            ("Fix #12 now", "title: \"Fix #12 now\""),
            ("- dash", "title: \"- dash\""),
            ("null", "title: \"null\""),
            ("42", "title: \"42\""),
            ("say \"hi\"", "title: say \"hi\""),
            ("plain title", "title: plain title"),
        ] {
            let opts = DraftOptions {
                title: Some(title.into()),
                ty: Some(EntryType::Prompt),
                ..Default::default()
            };
            let d = Draft::from_text("hello there", DraftSource::Stdin, &opts);
            let t = text(&d);
            assert!(t.contains(&format!("\n{line}\n")), "{title}: {t}");
            let entry = parse_entry("p/x", "p/x.md", t.into_bytes()).entry.unwrap();
            assert_eq!(entry.title(), Some(title), "{title}");
        }
    }

    #[test]
    fn link_and_code_have_no_fence() {
        for (_, input, want) in goldens()
            .into_iter()
            .filter(|g| g.0.ends_with("link") || g.0.ends_with("code"))
        {
            assert!(!want.contains("```"));
            assert_eq!(text(&golden_draft(&input)), want);
        }
        let mut d = golden_draft("# opts: title=x\nhttps://example.com/a\n");
        d.notes = "Some notes.".into();
        assert!(text(&d).ends_with("---\n\nSome notes.\n"), "{}", text(&d));
    }

    #[test]
    fn an_empty_notes_section_leaves_no_trailing_blank_line() {
        let t = text(&sql("select 1 from t;"));
        assert!(t.ends_with("```\n") && !t.ends_with("\n\n"), "{t:?}");
        let mut d = sql("select 1 from t;");
        d.notes = "  \n".into();
        assert_eq!(text(&d), t);
        d.notes = "Hello.\r\n\r\n".into();
        assert!(text(&d).ends_with("```\n\nHello.\n"));
    }

    #[test]
    fn variables_render_with_options_and_defaults() {
        let mut d = Draft::from_text(
            "echo {{env}} {{minutes}} {{token}} {{note}}",
            DraftSource::Stdin,
            &DraftOptions {
                title: Some("T".into()),
                ..Default::default()
            },
        );
        d.set_var_kind("env", VarKind::Enum).unwrap();
        d.set_var_options("env", vec!["dev".into(), "prod".into(), "a,b".into()])
            .unwrap();
        d.set_var_default("env", Some("prod".into())).unwrap();
        d.set_var_default("minutes", Some("15".into())).unwrap();
        d.set_var_default("note", Some("a: b".into())).unwrap();
        let t = text(&d);
        assert!(
            t.contains("  env: { kind: enum, options: [dev, prod, \"a,b\"], default: prod }\n"),
            "{t}"
        );
        assert!(
            t.contains("  minutes: { kind: number, default: 15 }\n"),
            "{t}"
        );
        assert!(t.contains("  token: { kind: secret }\n"), "{t}");
        assert!(
            t.contains("  note: { kind: text, default: \"a: b\" }\n"),
            "{t}"
        );
        let entry = parse_entry("cmd/t", "cmd/t.md", t.into_bytes())
            .entry
            .unwrap();
        let vars = entry.vars();
        assert_eq!(vars[0].options, ["dev", "prod", "a,b"]);
        assert_eq!(vars[1].default.as_deref(), Some("15"));
    }

    #[test]
    fn type_fields_follow_the_key_order() {
        let mut d = Draft::from_text(
            "x",
            DraftSource::Stdin,
            &DraftOptions {
                title: Some("T".into()),
                ty: Some(EntryType::Cw),
                ..Default::default()
            },
        );
        d.fields = DraftFields::Cw {
            log_groups: vec!["/aws/lambda/{{fn}}".into(), "/aws/x".into()],
            range: Some("-1h".into()),
            region: Some("us-east-1".into()),
        };
        let t = text(&d);
        let at = |s: &str| t.find(s).unwrap_or_else(|| panic!("{s} in {t}"));
        assert!(at("log_groups: [\"/aws/lambda/{{fn}}\", /aws/x]") < at("range: \"-1h\""));
        assert!(at("range: \"-1h\"") < at("region: us-east-1"));
        assert!(at("region: us-east-1") < at("created:"));
    }

    #[test]
    fn oversize_files_are_flagged_and_never_writable() {
        let big = "a".repeat(MAX_FILE_BYTES);
        let check = sql(&big).check("sql/t", &clock());
        assert!(check.too_large);
        assert!(!check.writable(true) && !check.writable(false));
    }

    #[test]
    fn secrets_are_never_writable_wherever_they_are() {
        let key = "AKIAIOSFODNN7EXAMPLE";
        let in_body = sql(&format!("select '{key}' from t;")).check("sql/t", &clock());
        let mut in_title = sql("select 1 from t;");
        in_title.title = format!("key {key}");
        let mut in_notes = sql("select 1 from t;");
        in_notes.notes = format!("use {key}");
        for (name, c) in [
            ("body", in_body),
            ("title", in_title.check("sql/t", &clock())),
            ("notes", in_notes.check("sql/t", &clock())),
        ] {
            assert!(!c.secrets().is_empty(), "{name}");
            assert!(!c.writable(true), "{name}");
            assert!(!c.writable(false), "{name}");
        }
    }

    #[test]
    fn other_errors_need_save_anyway() {
        let mut d = sql("select 1 from t;");
        d.title.clear();
        let c = d.check("sql/t", &clock());
        assert!(!c.errors().is_empty() && c.secrets().is_empty());
        assert!(!c.writable(false));
        assert!(c.writable(true));
        assert!(
            c.errors()
                .iter()
                .all(|f| f.code != RuleCode::SecretAwsAccessKey)
        );
    }

    #[test]
    fn every_corpus_snippet_renders_a_clean_round_trip() {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/authoring/detect");
        let mut n = 0;
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let input = std::fs::read_to_string(&p).unwrap();
                let opts = DraftOptions {
                    title: Some("Corpus snippet".into()),
                    ..Default::default()
                };
                let d = Draft::from_text(&input, DraftSource::Stdin, &opts);
                let id = d.target_id(&|_| false).unwrap();
                let c = d.check(&id, &clock());
                let name = p.display();
                let errors: Vec<_> = c
                    .findings
                    .iter()
                    .filter(|f| f.severity == Severity::Error)
                    .collect();
                assert!(
                    errors.is_empty(),
                    "{name}: {errors:?}\n{}",
                    String::from_utf8_lossy(&c.bytes)
                );
                let parsed = parse_entry(&id, &format!("{id}.md"), c.bytes.clone())
                    .entry
                    .expect("parses");
                assert_eq!(parsed.to_bytes(), c.bytes, "{name}");
                n += 1;
            }
        }
        assert!(n >= 100);
    }
}
