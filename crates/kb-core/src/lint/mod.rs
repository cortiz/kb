//! Findings, rule codes and the lint report.

use std::fmt;
use std::path::{Path, PathBuf};

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

            /// The rule's number, for example `1` for `KB001`.
            pub fn number(self) -> u16 {
                match self { $(RuleCode::$variant => $num,)* }
            }

            /// The rule's name, for example `unknown-type`.
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
    InvalidEscape = 54, "invalid-escape", Error;
    SecretAwsAccessKey = 60, "secret-aws-access-key", Error;
    SecretAwsSecretKey = 61, "secret-aws-secret-key", Error;
    SecretBearerToken = 62, "secret-bearer-token", Error;
    SecretJwt = 63, "secret-jwt", Error;
    SecretPrivateKey = 64, "secret-private-key", Error;
    SecretHighEntropy = 65, "secret-high-entropy", Error;
    NoteTitle = 70, "note-title", Error;
    NoteVars = 71, "note-vars", Error;
    NoteOversized = 72, "note-oversized", Warning;
    NoteEmpty = 73, "note-empty", Info;
    EntryLinkBroken = 74, "entry-link-broken", Warning;
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

/// The result of linting a KB or part of it.
#[derive(Debug, Clone, Default)]
pub struct LintReport {
    /// The canonical KB root.
    pub root: PathBuf,
    /// `.md` files examined, including ones with errors.
    pub entries: usize,
    /// Sorted by path, line, column, code.
    pub findings: Vec<Finding>,
    pub suppressions: Vec<Suppression>,
}

/// Why lint could not run (exit code 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintError {
    /// The KB root does not exist or is not a directory.
    RootMissing(PathBuf),
    /// A path argument does not exist.
    PathNotFound(PathBuf),
    /// A path argument is outside the KB root.
    OutsideRoot(PathBuf),
    /// Reading the KB failed.
    Io { path: PathBuf, message: String },
}

impl fmt::Display for LintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LintError::RootMissing(p) => {
                write!(f, "KB root {} does not exist (run `kb init`?)", p.display())
            }
            LintError::PathNotFound(p) => write!(f, "{} does not exist", p.display()),
            LintError::OutsideRoot(p) => write!(f, "{} is outside the KB root", p.display()),
            LintError::Io { path, message } => {
                write!(f, "cannot read {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for LintError {}

/// Resolves `kb lint` path arguments (research R10): absolute paths as given; relative paths
/// against `cwd` when they exist there, otherwise against `root`.
pub fn resolve_scope(
    root: &Path,
    cwd: &Path,
    paths: &[PathBuf],
) -> Result<Vec<PathBuf>, LintError> {
    let canonical_root = root
        .canonicalize()
        .map_err(|_| LintError::RootMissing(root.to_path_buf()))?;
    let mut out = Vec::new();
    for path in paths {
        let candidate = if path.is_absolute() {
            path.clone()
        } else if cwd.join(path).exists() {
            cwd.join(path)
        } else {
            root.join(path)
        };
        let Ok(canonical) = canonical_location(&candidate) else {
            return Err(LintError::PathNotFound(path.clone()));
        };
        if !canonical.starts_with(&canonical_root) {
            return Err(LintError::OutsideRoot(candidate));
        }
        out.push(candidate);
    }
    Ok(out)
}

/// The canonical location of `path` without resolving a final symlink: the canonical parent
/// joined with the file name, so a symlinked entry keeps its own ID.
fn canonical_location(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::symlink_metadata(path)?;
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
            Ok(parent.canonicalize()?.join(name))
        }
        _ => path.canonicalize(),
    }
}

/// Lints the files under `scope` (absolute paths inside `root`); an empty scope means the whole KB.
pub fn lint(root: &Path, scope: &[PathBuf]) -> Result<LintReport, LintError> {
    let canonical_root = root
        .canonicalize()
        .ok()
        .filter(|r| r.is_dir())
        .ok_or_else(|| LintError::RootMissing(root.to_path_buf()))?;
    let mut starts = Vec::new();
    for path in scope {
        let canonical =
            canonical_location(path).map_err(|_| LintError::PathNotFound(path.clone()))?;
        if !canonical.starts_with(&canonical_root) {
            return Err(LintError::OutsideRoot(path.clone()));
        }
        starts.push(canonical);
    }
    if starts.is_empty() {
        starts.push(canonical_root.clone());
    }
    let walked = crate::walk::walk(&canonical_root, &starts).map_err(|e| LintError::Io {
        path: e.path,
        message: e.message,
    })?;
    let mut report = LintReport {
        root: canonical_root,
        entries: walked.files.len(),
        findings: walked.findings,
        ..Default::default()
    };
    for file in &walked.files {
        let bytes = std::fs::read(&file.abs_path).map_err(|e| LintError::Io {
            path: file.abs_path.clone(),
            message: e.to_string(),
        })?;
        lint_file(file, bytes, &mut report);
    }
    report
        .findings
        .extend(crate::walk::case_collisions(&walked.files));
    report
        .findings
        .sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    report
        .suppressions
        .sort_by(|a, b| (&a.path, a.line, a.code).cmp(&(&b.path, b.line, b.code)));
    Ok(report)
}

/// Everything the per-file checks found, plus the parsed entry when the file is readable.
pub(crate) struct FileCheck {
    pub findings: Vec<Finding>,
    pub entry: Option<crate::entry::Entry>,
}

/// Runs the checks of one file (size, encoding, line endings, secrets, format). `lint` and the
/// search index both go through this, so "has errors" means the same thing in both.
pub(crate) fn check_file(file: &crate::walk::WalkEntry, bytes: Vec<u8>) -> FileCheck {
    let mut report = LintReport::default();
    let entry = lint_file(file, bytes, &mut report);
    FileCheck {
        findings: report.findings,
        entry,
    }
}

/// The findings `kb lint` would report for one entry held in memory (a draft about to be
/// written, or an edited file). Same checks as the index and `kb lint`: there is one definition of
/// "has errors".
pub fn check_text(id: &str, bytes: Vec<u8>) -> Vec<Finding> {
    let rel_path = format!("{id}.md");
    let file = crate::walk::WalkEntry {
        id: id.to_string(),
        abs_path: PathBuf::from(&rel_path),
        rel_path,
    };
    let mut findings = check_file(&file, bytes).findings;
    findings.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    findings
}

fn lint_file(
    file: &crate::walk::WalkEntry,
    bytes: Vec<u8>,
    report: &mut LintReport,
) -> Option<crate::entry::Entry> {
    let finding = |line, column, code: RuleCode, message: String, fix: Option<&str>| Finding {
        path: file.rel_path.clone(),
        id: Some(file.id.clone()),
        line,
        column,
        severity: code.severity(),
        code,
        message,
        fix: fix.map(str::to_string),
    };
    if bytes.len() > rules::MAX_FILE_BYTES {
        report.findings.push(finding(
            1,
            1,
            RuleCode::FileTooLarge,
            format!(
                "the file is {} bytes; entries are meant to be small (limit {})",
                bytes.len(),
                rules::MAX_FILE_BYTES
            ),
            None,
        ));
    }
    let source = match crate::source::Source::from_bytes(bytes.clone()) {
        Ok(source) => source,
        Err(e) => {
            report.findings.push(crate::entry::invalid_utf8(
                &file.id,
                &file.rel_path,
                &bytes,
                e.offset,
            ));
            return None;
        }
    };
    if let Some(offset) = rules::mixed_line_ending_at(&source) {
        let (line, column) = source.position(offset);
        report.findings.push(finding(
            line,
            column,
            RuleCode::MixedLineEndings,
            "the file mixes LF and CRLF line endings".into(),
            Some("convert the file to one line-ending style"),
        ));
    }
    let scan = secrets::scan_secrets(source.text());
    for hit in scan.hits {
        let (line, column) = source.position(hit.offset);
        report
            .findings
            .push(finding(line, column, hit.code, hit.message, Some(&hit.fix)));
    }
    for (offset, code) in scan.suppressed {
        let (line, _) = source.position(offset);
        report.suppressions.push(Suppression {
            path: file.rel_path.clone(),
            line,
            code,
        });
    }
    let outcome = crate::entry::parse_source(&file.id, &file.rel_path, source);
    report.findings.extend(outcome.findings);
    outcome.entry
}

impl LintReport {
    /// How many findings have this severity.
    pub fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }

    /// `0`, or `1` when there is an error, or a warning under `strict`.
    pub fn exit_code(&self, strict: bool) -> i32 {
        let failed =
            self.count(Severity::Error) > 0 || (strict && self.count(Severity::Warning) > 0);
        i32::from(failed)
    }

    /// The `kb lint --json` document (contracts/lint-report.schema.json, version 1).
    pub fn to_json(&self, strict: bool) -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "root": self.root.to_string_lossy(),
            "summary": {
                "entries": self.entries,
                "errors": self.count(Severity::Error),
                "warnings": self.count(Severity::Warning),
                "info": self.count(Severity::Info),
                "suppressed": self.suppressions.len(),
                "failed": self.exit_code(strict) != 0,
            },
            "findings": self.findings,
            "suppressions": self.suppressions,
        })
    }

    /// `284 entries, 0 errors, 0 warnings`, plus info and suppressed counts when non-zero.
    pub fn summary_line(&self) -> String {
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut line = format!(
            "{}, {}, {}",
            plural(self.entries, "entry", "entries"),
            plural(self.count(Severity::Error), "error", "errors"),
            plural(self.count(Severity::Warning), "warning", "warnings"),
        );
        let info = self.count(Severity::Info);
        if info > 0 {
            line.push_str(&format!(", {info} info"));
        }
        if !self.suppressions.is_empty() {
            line.push_str(&format!(", {} suppressed", self.suppressions.len()));
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(
        errors: usize,
        warnings: usize,
        info: usize,
        suppressed: usize,
        entries: usize,
    ) -> LintReport {
        let f = |severity, code| Finding {
            path: "a.md".into(),
            id: None,
            line: 1,
            column: 1,
            severity,
            code,
            message: String::new(),
            fix: None,
        };
        let mut findings = Vec::new();
        findings.extend((0..errors).map(|_| f(Severity::Error, RuleCode::UnknownType)));
        findings.extend((0..warnings).map(|_| f(Severity::Warning, RuleCode::UnusedVar)));
        findings.extend((0..info).map(|_| f(Severity::Info, RuleCode::UnknownKey)));
        let suppressions = (0..suppressed)
            .map(|_| Suppression {
                path: "a.md".into(),
                line: 1,
                code: RuleCode::SecretJwt,
            })
            .collect();
        LintReport {
            root: PathBuf::new(),
            entries,
            findings,
            suppressions,
        }
    }

    #[test]
    fn summary_lines() {
        assert_eq!(
            report(0, 0, 0, 0, 284).summary_line(),
            "284 entries, 0 errors, 0 warnings"
        );
        assert_eq!(
            report(1, 1, 0, 0, 1).summary_line(),
            "1 entry, 1 error, 1 warning"
        );
        assert_eq!(
            report(0, 2, 3, 1, 5).summary_line(),
            "5 entries, 0 errors, 2 warnings, 3 info, 1 suppressed"
        );
        assert_eq!(
            report(0, 0, 0, 2, 5).summary_line(),
            "5 entries, 0 errors, 0 warnings, 2 suppressed"
        );
    }

    #[test]
    fn exit_codes() {
        assert_eq!(report(0, 0, 3, 1, 1).exit_code(true), 0);
        assert_eq!(report(1, 0, 0, 0, 1).exit_code(false), 1);
        assert_eq!(report(0, 1, 0, 0, 1).exit_code(false), 0);
        assert_eq!(report(0, 1, 0, 0, 1).exit_code(true), 1);
    }

    #[test]
    fn scope_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("kb");
        let cwd = dir.path().join("cwd");
        for p in [
            root.join("sumo/a.md"),
            cwd.join("sumo/a.md"),
            root.join("sql/b.md"),
        ] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
        }
        // Relative path found under cwd wins, but it is outside the root.
        assert_eq!(
            resolve_scope(&root, &cwd, &["sumo/a.md".into()]),
            Err(LintError::OutsideRoot(cwd.join("sumo/a.md")))
        );
        // Only under the root.
        assert_eq!(
            resolve_scope(&root, &cwd, &["sql/b.md".into()]).unwrap(),
            vec![root.join("sql/b.md")]
        );
        assert_eq!(
            resolve_scope(&root, &cwd, &["sql".into()]).unwrap(),
            vec![root.join("sql")]
        );
        // cwd inside the root.
        assert_eq!(
            resolve_scope(&root, &root.join("sql"), &["b.md".into()]).unwrap(),
            vec![root.join("sql/b.md")]
        );
        assert_eq!(
            resolve_scope(&root, &cwd, &["nope.md".into()]),
            Err(LintError::PathNotFound("nope.md".into()))
        );
        assert_eq!(
            resolve_scope(&root, &cwd, &[root.join("sql/b.md")]).unwrap(),
            vec![root.join("sql/b.md")]
        );
    }

    /// SC-003: at least 80% of error rules carry a suggested fix. Every error rule except
    /// KB006 (whose message comes from the YAML parser) must produce one.
    #[test]
    fn error_rules_carry_fixes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        let mut seen = std::collections::BTreeMap::new();
        for dir in ["invalid", "secrets"] {
            let report = lint(&root.join(dir), &[]).unwrap();
            for f in report
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error)
            {
                *seen.entry(f.code).or_insert(true) &= f.fix.is_some();
            }
        }
        for f in crate::entry::parse_entry("x", "x.md", b"---\nab\xff\n".to_vec()).findings {
            seen.insert(f.code, f.fix.is_some());
        }
        let errors: Vec<RuleCode> = RuleCode::ALL
            .iter()
            .copied()
            .filter(|c| c.severity() == Severity::Error)
            .collect();
        for code in &errors {
            if *code == RuleCode::IdCaseCollision {
                continue; // needs two files; covered in walk.rs
            }
            let has_fix = seen
                .get(code)
                .copied()
                .unwrap_or_else(|| panic!("no fixture produces {code}"));
            assert_eq!(has_fix, *code != RuleCode::YamlSyntax, "{code}");
        }
        assert_eq!(errors.len(), 41);
    }

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
        assert_eq!(RuleCode::ALL.len(), 54);
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

    const GOOD: &str = "---\nkb_format: 1\ntype: cmd\ntitle: Restart api\n---\n\n```sh\nkubectl rollout restart deploy/api\n```\n";

    fn codes(findings: &[Finding]) -> Vec<RuleCode> {
        findings.iter().map(|f| f.code).collect()
    }

    /// What `lint()` reports for the same bytes written to `<id>.md` in a temp KB.
    fn via_files(id: &str, text: &str) -> Vec<Finding> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("{id}.md"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        lint(dir.path(), &[]).unwrap().findings
    }

    #[test]
    fn check_text_valid_entry_has_no_findings() {
        assert!(check_text("cmd/restart-api", GOOD.as_bytes().to_vec()).is_empty());
    }

    #[test]
    fn check_text_matches_lint_on_the_same_bytes() {
        let aws = GOOD.replace(
            "kubectl",
            "export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE; kubectl",
        );
        for text in [
            GOOD.replace("title: Restart api\n", ""),
            GOOD.replace("type: cmd", "type: nope"),
            aws,
        ] {
            let expected = via_files("cmd/restart-api", &text);
            let got = check_text("cmd/restart-api", text.clone().into_bytes());
            assert!(!expected.is_empty());
            assert_eq!(codes(&got), codes(&expected), "{text}");
            assert_eq!(
                got.iter().map(|f| f.line).collect::<Vec<_>>(),
                expected.iter().map(|f| f.line).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn check_text_honors_allow_secret() {
        let text = GOOD.replace(
            "kubectl rollout restart deploy/api",
            "export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE # kb:allow-secret",
        );
        assert!(check_text("cmd/x", text.clone().into_bytes()).is_empty());
        assert!(via_files("cmd/x", &text).is_empty());
    }
}
