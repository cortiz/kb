//! Variable suggestions for literal values (FR-014 to FR-016, research R4).

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use crate::author::draft::VarDecl;
use crate::vars::VarKind;

/// Which of the five patterns a suggestion came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestPattern {
    Env,
    Region,
    Time,
    Account,
    Id,
}

/// A literal value that could become a variable. Never applied on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableSuggestion {
    pub value: String,
    /// Byte ranges in the body; they cover the replaced part only (the digits for `Time`).
    pub positions: Vec<Range<usize>>,
    pub name: String,
    pub kind: VarKind,
    pub options: Vec<String>,
    pub default: String,
    pub pattern: SuggestPattern,
}

/// Literal values in `body` that could be variables. Names are unique against `declared` and each other.
pub fn suggest_vars(body: &str, declared: &[VarDecl]) -> Vec<VariableSuggestion> {
    // start -> end of every range that is taken; they never overlap, so only the last one that
    // starts before `r` ends can get in its way
    type Claimed = BTreeMap<usize, usize>;
    let mut claimed: Claimed = REFERENCE
        .find_iter(body)
        .map(|m| (m.start(), m.end()))
        .collect();
    let free = |claimed: &Claimed, r: &Range<usize>| {
        claimed
            .range(..r.end)
            .next_back()
            .is_none_or(|(_, end)| *end <= r.start)
    };
    // (pattern, name, value, kind, options, positions)
    let mut found: Vec<Found> = Vec::new();
    let add = |claimed: &mut Claimed,
               found: &mut Vec<Found>,
               pattern: SuggestPattern,
               base: &str,
               value: &str,
               range: Range<usize>| {
        if !free(claimed, &range) {
            return;
        }
        claimed.insert(range.start, range.end);
        match found
            .iter_mut()
            .find(|f| f.pattern == pattern && f.base == base && f.value == value)
        {
            Some(f) => f.positions.push(range),
            None => found.push(Found {
                pattern,
                base: base.to_string(),
                value: value.to_string(),
                positions: vec![range],
            }),
        }
    };
    for m in ENV.find_iter(body) {
        add(
            &mut claimed,
            &mut found,
            SuggestPattern::Env,
            "env",
            m.as_str(),
            m.range(),
        );
    }
    for m in REGION.find_iter(body) {
        add(
            &mut claimed,
            &mut found,
            SuggestPattern::Region,
            "region",
            m.as_str(),
            m.range(),
        );
    }
    for caps in TIME.captures_iter(body) {
        let whole = caps.get(0).expect("group 0");
        let before = body[..whole.start()].chars().next_back();
        let after = body[whole.end()..].chars().next();
        if before.is_some_and(char::is_alphanumeric) || after.is_some_and(char::is_alphanumeric) {
            continue;
        }
        let digits = caps.get(1).expect("digits");
        let name = match &caps[2] {
            "m" => "minutes",
            "h" => "hours",
            _ => "days",
        };
        add(
            &mut claimed,
            &mut found,
            SuggestPattern::Time,
            name,
            digits.as_str(),
            digits.range(),
        );
    }
    for caps in INTERVAL.captures_iter(body) {
        let digits = caps.get(1).expect("digits");
        let name = match caps[2].to_ascii_lowercase().as_str() {
            "minute" => "minutes",
            "hour" => "hours",
            _ => "days",
        };
        add(
            &mut claimed,
            &mut found,
            SuggestPattern::Time,
            name,
            digits.as_str(),
            digits.range(),
        );
    }
    for caps in ACCOUNT.captures_iter(body) {
        let id = caps.get(1).or_else(|| caps.get(2)).expect("account id");
        add(
            &mut claimed,
            &mut found,
            SuggestPattern::Account,
            "account",
            id.as_str(),
            id.range(),
        );
    }
    for m in UUID.find_iter(body).chain(LONG_NUMBER.find_iter(body)) {
        add(
            &mut claimed,
            &mut found,
            SuggestPattern::Id,
            "id",
            m.as_str(),
            m.range(),
        );
    }
    found.sort_by_key(|f| f.positions.iter().map(|r| r.start).min());
    let mut used: Vec<String> = declared.iter().map(|d| d.name.clone()).collect();
    found
        .into_iter()
        .map(|mut f| {
            f.positions.sort_by_key(|r| r.start);
            let mut name = f.base.clone();
            let mut n = 2;
            while used.contains(&name) {
                name = format!("{}_{n}", f.base);
                n += 1;
            }
            used.push(name.clone());
            let (kind, options) = match f.pattern {
                SuggestPattern::Env => (VarKind::Enum, env_options(&f.value)),
                SuggestPattern::Time => (VarKind::Number, Vec::new()),
                _ => (VarKind::Text, Vec::new()),
            };
            VariableSuggestion {
                default: f.value.clone(),
                value: f.value,
                positions: f.positions,
                name,
                kind,
                options,
                pattern: f.pattern,
            }
        })
        .collect()
}

struct Found {
    pattern: SuggestPattern,
    base: String,
    value: String,
    positions: Vec<Range<usize>>,
}

/// The options of an `env` variable: the family of the value (`staging` and `production` are the
/// long form).
fn env_options(value: &str) -> Vec<String> {
    let family: [&str; 4] = if matches!(value, "staging" | "production") {
        ["dev", "qa", "staging", "production"]
    } else {
        ["dev", "qa", "stage", "prod"]
    };
    family.map(str::to_string).to_vec()
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a valid suggestion pattern")
}

static REFERENCE: LazyLock<Regex> = LazyLock::new(|| re(r"\\?\{\{[^}\n]*\}\}"));
static ENV: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:prod|production|stage|staging|dev|qa)\b"));
static REGION: LazyLock<Regex> =
    LazyLock::new(|| re(r"\b(?:us|eu|ap|sa|ca|me|af|il|mx)-[a-z]+-\d\b"));
static TIME: LazyLock<Regex> = LazyLock::new(|| re(r"-?(\d+)([mhd])"));
static INTERVAL: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\bINTERVAL\s+'(\d+)\s+(minute|hour|day)s?'"));
static ACCOUNT: LazyLock<Regex> = LazyLock::new(|| {
    re(r#"(?i)arn:aws:[a-z0-9-]*:[a-z0-9-]*:(\d{12}):|\baccount(?:[_-]id)?[\s:='"]+(\d{12})\b"#)
});
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    re(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b")
});
static LONG_NUMBER: LazyLock<Regex> = LazyLock::new(|| re(r"\b\d{9,}\b"));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::author::draft::{Draft, DraftOptions, DraftSource};
    use crate::entry::{EntryType, parse_entry};
    use crate::varres::{self, Inputs, Values};
    use std::path::{Path, PathBuf};

    fn dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/authoring/suggest")
    }

    fn read(name: &str) -> String {
        std::fs::read_to_string(dir().join(name)).unwrap()
    }

    fn draft(text: &str, ty: EntryType) -> Draft {
        let opts = DraftOptions {
            title: Some("T".into()),
            ty: Some(ty),
            ..Default::default()
        };
        Draft::from_text(text, DraftSource::Stdin, &opts)
    }

    fn cases() -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir())
            .unwrap()
            .flatten()
            .filter_map(|e| {
                e.file_name()
                    .to_string_lossy()
                    .strip_suffix(".in.txt")
                    .map(str::to_string)
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn every_pattern_case_accepts_to_the_expected_body_and_declarations() {
        let names = cases();
        assert!(names.len() >= 14, "{names:?}");
        for name in names {
            let input = read(&format!("{name}.in.txt"));
            let mut d = draft(input.trim_end(), EntryType::Cmd);
            let mut guard = 0;
            while !d.suggestions.is_empty() {
                d.accept(0);
                guard += 1;
                assert!(guard < 20, "{name}: suggestions never run out");
            }
            assert_eq!(
                format!("{}\n", d.body),
                read(&format!("{name}.out.txt")),
                "{name}"
            );
            let got: Vec<String> = d
                .vars
                .iter()
                .map(|v| {
                    format!(
                        "{}|{}|{}|{}",
                        v.name,
                        crate::vars::VarKind::NAMES[v.kind as usize],
                        v.default.clone().unwrap_or_default(),
                        v.options.join(",")
                    )
                })
                .collect();
            let want: Vec<String> = read(&format!("{name}.vars.txt"))
                .lines()
                .map(str::to_string)
                .collect();
            assert_eq!(got, want, "{name}");
        }
    }

    #[test]
    fn negative_cases_suggest_nothing() {
        for e in std::fs::read_dir(dir().join("none")).unwrap().flatten() {
            let text = std::fs::read_to_string(e.path()).unwrap();
            let found = suggest_vars(text.trim_end(), &[]);
            assert!(found.is_empty(), "{:?}: {found:?}", e.file_name());
        }
    }

    #[test]
    fn one_suggestion_per_value_with_every_position() {
        let found = suggest_vars("prod to prod via stage", &[]);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].positions, [0..4, 8..12]);
        assert_eq!(
            (found[0].name.as_str(), found[1].name.as_str()),
            ("env", "env_2")
        );
        assert_eq!(found[0].pattern, SuggestPattern::Env);
        assert!(found.iter().all(|s| {
            s.value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
        }));
    }

    #[test]
    fn names_avoid_the_declared_ones() {
        let declared = vec![VarDecl {
            name: "env".into(),
            kind: crate::vars::VarKind::Text,
            default: None,
            options: vec![],
        }];
        assert_eq!(suggest_vars("run in prod", &declared)[0].name, "env_2");
    }

    #[test]
    fn existing_references_and_escapes_are_left_alone() {
        assert!(suggest_vars("{{prod}} \\{{stage}} {{minutes}}m", &[]).is_empty());
        assert_eq!(suggest_vars("{{x}} prod", &[]).len(), 1);
    }

    #[test]
    fn accepted_suggestions_leave_the_rest_in_place() {
        let mut d = draft("prod in us-east-1 for 30m", EntryType::Cmd);
        assert_eq!(d.suggestions.len(), 3);
        d.accept(1);
        assert_eq!(d.body, "prod in {{region}} for 30m");
        assert_eq!(d.suggestions.len(), 2);
        assert!(d.suggestions.iter().all(|s| s.name != "region"));
        let v = d.vars.iter().find(|v| v.name == "region").unwrap();
        assert_eq!(v.default.as_deref(), Some("us-east-1"));
    }

    /// SC-006: accept everything, resolve with the defaults, get the pasted text back.
    #[test]
    fn accepting_all_and_resolving_with_defaults_reproduces_the_paste() {
        let pastes = [
            (
                EntryType::Cmd,
                "kubectl -n prod logs --since=30m deploy/api --region us-east-1",
            ),
            (EntryType::Cmd, "echo \"prod us-east-1 30m 123456789012\""),
            (
                EntryType::Cmd,
                "echo 'staging 7d 3f2504e0-4f89-41d3-9a0c-0305e82c3301'",
            ),
            (
                EntryType::Sql,
                "select * from t where env = 'prod' and ts > now() - INTERVAL '1 day' and acct = '98765432101'",
            ),
            (
                EntryType::Sumo,
                "_sourceCategory=prod/lambda/authorizer | timeslice 1m | where region = \"eu-west-2\"",
            ),
            (
                EntryType::Prompt,
                "see https://example.com/prod/us-east-1/-30m?x=7d",
            ),
        ];
        for (ty, paste) in pastes {
            let mut d = draft(paste, ty);
            while !d.suggestions.is_empty() {
                d.accept(0);
            }
            assert_ne!(d.body, paste, "{paste}");
            let bytes = d.render(&crate::author::FixedClock(std::time::UNIX_EPOCH));
            let entry = parse_entry("x/y", "x/y.md", bytes).entry.unwrap();
            let plan = varres::plan_vars(&entry);
            let bindings = varres::prefill(
                &plan,
                &Inputs {
                    flags: &[],
                    env: &|_| None,
                    memory: None,
                },
            );
            let values: Values = bindings
                .into_iter()
                .map(|b| {
                    (
                        b.name,
                        b.value.expect("every accepted variable has a default"),
                    )
                })
                .collect();
            let resolved = varres::resolve(&plan, &values).unwrap();
            assert_eq!(resolved.primary().unwrap().text.trim_end(), paste, "{ty:?}");
        }
        // the values are exactly the characters that need no quoting anywhere
        for v in [
            "prod",
            "us-east-1",
            "30",
            "123456789012",
            "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        ] {
            assert_eq!(varres::escape::percent(v), v);
            assert_eq!(varres::escape::unquoted(v, crate::entry::Shell::Sh), v);
        }
    }
}
