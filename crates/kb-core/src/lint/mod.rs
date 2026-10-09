//! Findings, rule codes and the lint report.

use std::fmt;

use serde::{Serialize, Serializer};

pub mod rules;
pub mod secrets;
pub mod suggest;

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

macro_rules! rule_codes {
    ($($variant:ident = $num:literal, $slug:literal, $sev:ident;)*) => {
        /// A stable lint rule code, displayed as `KB001-unknown-type`.
        ///
        /// See `specs/001-entry-format/contracts/lint-rules.md`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum RuleCode {
            $($variant,)*
        }

        impl RuleCode {
            /// Every rule code, in numeric order.
            pub const ALL: &'static [RuleCode] = &[$(RuleCode::$variant,)*];

            pub fn number(self) -> u16 {
                match self { $(RuleCode::$variant => $num,)* }
            }

            pub fn slug(self) -> &'static str {
                match self { $(RuleCode::$variant => $slug,)* }
            }

            /// The severity findings of this rule are reported with.
            pub fn severity(self) -> Severity {
                match self { $(RuleCode::$variant => Severity::$sev,)* }
            }
        }
    };
}

rule_codes! {
    UnknownType = 1, "unknown-type", Error;
    NoFrontmatter = 2, "no-frontmatter", Error;
    UnclosedFrontmatter = 3, "unclosed-frontmatter", Error;
    EmptyFile = 4, "empty-file", Error;
    InvalidUtf8 = 5, "invalid-utf8", Error;
    YamlSyntax = 6, "yaml-syntax", Error;
    FrontmatterNotMapping = 7, "frontmatter-not-mapping", Error;
    UnsupportedYaml = 8, "unsupported-yaml", Error;
    DuplicateKey = 9, "duplicate-key", Error;
    MissingKbFormat = 10, "missing-kb-format", Error;
    InvalidKbFormat = 11, "invalid-kb-format", Error;
    UnsupportedKbFormat = 12, "unsupported-kb-format", Error;
    MissingType = 13, "missing-type", Error;
    MissingTitle = 14, "missing-title", Error;
    InvalidTitle = 15, "invalid-title", Error;
    UnknownKey = 16, "unknown-key", Info;
    FieldNotForType = 17, "field-not-for-type", Warning;
    WrongValueType = 18, "wrong-value-type", Error;
    InvalidTag = 20, "invalid-tag", Error;
    DuplicateTag = 21, "duplicate-tag", Warning;
    InvalidDate = 22, "invalid-date", Error;
    MissingUrl = 30, "missing-url", Error;
    InvalidUrl = 31, "invalid-url", Error;
    MissingCodeRef = 32, "missing-code-ref", Error;
    InvalidCodeRef = 33, "invalid-code-ref", Error;
    InvalidShell = 34, "invalid-shell", Error;
    MissingPayload = 35, "missing-payload", Error;
    EmptyPayload = 36, "empty-payload", Warning;
    InvalidVarName = 40, "invalid-var-name", Error;
    MissingVarKind = 41, "missing-var-kind", Error;
    UnknownVarKind = 42, "unknown-var-kind", Error;
    SecretDefault = 43, "secret-default", Error;
    EnumOptions = 44, "enum-options", Error;
    DefaultNotInOptions = 45, "default-not-in-options", Error;
    InvalidNumberDefault = 46, "invalid-number-default", Error;
    UndeclaredVar = 47, "undeclared-var", Info;
    UnusedVar = 48, "unused-var", Warning;
    NotAVariable = 49, "not-a-variable", Info;
    MixedLineEndings = 50, "mixed-line-endings", Warning;
    FileTooLarge = 51, "file-too-large", Warning;
    SymlinkOutsideRoot = 52, "symlink-outside-root", Warning;
    IdCaseCollision = 53, "id-case-collision", Error;
    SecretAwsAccessKey = 60, "secret-aws-access-key", Error;
    SecretAwsSecretKey = 61, "secret-aws-secret-key", Error;
    SecretBearerToken = 62, "secret-bearer-token", Error;
    SecretJwt = 63, "secret-jwt", Error;
    SecretPrivateKey = 64, "secret-private-key", Error;
    SecretHighEntropy = 65, "secret-high-entropy", Error;
}

impl fmt::Display for RuleCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KB{:03}-{}", self.number(), self.slug())
    }
}

impl std::str::FromStr for RuleCode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RuleCode::ALL
            .iter()
            .copied()
            .find(|c| c.to_string() == s)
            .ok_or_else(|| format!("unknown rule code {s:?}"))
    }
}

impl Serialize for RuleCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// One problem found in one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// Path relative to the KB root, with `/` separators.
    pub path: String,
    /// The entry ID, when the file is an entry.
    pub id: Option<String>,
    /// 1-based line.
    pub line: u32,
    /// 1-based column, counted in Unicode characters.
    pub column: u32,
    pub severity: Severity,
    pub code: RuleCode,
    pub message: String,
    pub fix: Option<String>,
}

impl Finding {
    /// Sort key: path, line, column, code.
    pub fn sort_key(&self) -> (&str, u32, u32, RuleCode) {
        (&self.path, self.line, self.column, self.code)
    }
}

/// A secret finding silenced by a trailing `kb:allow-secret`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Suppression {
    pub path: String,
    pub line: u32,
    pub code: RuleCode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_form() {
        assert_eq!(RuleCode::UnknownType.to_string(), "KB001-unknown-type");
        assert_eq!(
            RuleCode::SecretHighEntropy.to_string(),
            "KB065-secret-high-entropy"
        );
    }

    #[test]
    fn every_code_round_trips_and_is_unique() {
        let mut seen = std::collections::HashSet::new();
        for &code in RuleCode::ALL {
            let shown = code.to_string();
            assert_eq!(shown.parse::<RuleCode>().unwrap(), code);
            assert!(
                seen.insert(code.number()),
                "duplicate number {}",
                code.number()
            );
        }
        assert_eq!(RuleCode::ALL.len(), 48);
    }

    #[test]
    fn severity_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Severity::Error).unwrap(),
            "\"error\""
        );
        assert_eq!(
            serde_json::to_string(&Severity::Warning).unwrap(),
            "\"warning\""
        );
        assert_eq!(serde_json::to_string(&Severity::Info).unwrap(), "\"info\"");
        assert_eq!(
            serde_json::to_string(&RuleCode::EmptyFile).unwrap(),
            "\"KB004-empty-file\""
        );
    }

    #[test]
    fn findings_sort_by_path_line_column_code() {
        let f = |path: &str, line, column, code| Finding {
            path: path.into(),
            id: None,
            line,
            column,
            severity: Severity::Error,
            code,
            message: String::new(),
            fix: None,
        };
        let mut v = [
            f("b.md", 1, 1, RuleCode::UnknownType),
            f("a.md", 2, 1, RuleCode::UnknownType),
            f("a.md", 1, 5, RuleCode::MissingTitle),
            f("a.md", 1, 5, RuleCode::UnknownType),
        ];
        v.sort_by(|x, y| x.sort_key().cmp(&y.sort_key()));
        let order: Vec<_> = v
            .iter()
            .map(|x| (x.path.as_str(), x.line, x.column, x.code))
            .collect();
        assert_eq!(
            order,
            vec![
                ("a.md", 1, 5, RuleCode::UnknownType),
                ("a.md", 1, 5, RuleCode::MissingTitle),
                ("a.md", 2, 1, RuleCode::UnknownType),
                ("b.md", 1, 1, RuleCode::UnknownType),
            ]
        );
    }
}
