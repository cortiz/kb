//! Type detection for pasted content (FR-004 to FR-006, research R2).

use std::sync::LazyLock;

use regex::Regex;

use crate::author::draft::DraftFields;
use crate::entry::EntryType;

/// How sure detection is. Only `Low` changes behavior: the form focuses the type field and
/// `kb add --yes` requires `--type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// What detection found in a piece of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub ty: EntryType,
    pub confidence: Confidence,
    /// Pre-filled from the content: a link's URL, a code location.
    pub fields: DraftFields,
    /// The text without a surrounding Markdown fence and (for `cmd`) without prompt markers.
    /// Empty for `link` and `code`.
    pub body: String,
    /// The language of the surrounding fence, if there was one.
    pub hint: Option<String>,
    /// `$ ` or `% ` markers were removed from every line.
    pub stripped_prompts: bool,
}

/// CRLF and CR become LF; blank lines at both ends are dropped (indentation of the first line is
/// kept).
pub fn normalize_clipboard(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(first) = lines.iter().position(|l| !l.trim().is_empty()) else {
        return String::new();
    };
    lines[first..].join("\n").trim_end().to_string()
}

/// Guesses the type of `text`. Deterministic, local rules only.
pub fn detect(text: &str) -> Detection {
    let normalized = normalize_clipboard(text);
    let (inner, hint) = match strip_fence(&normalized) {
        Some((body, hint)) => (normalize_clipboard(&body), hint),
        None => (normalized, None),
    };
    let mut detection = Detection {
        ty: EntryType::Cmd,
        confidence: Confidence::Low,
        fields: DraftFields::None,
        body: String::new(),
        hint: hint.clone(),
        stripped_prompts: false,
    };
    if inner.is_empty() {
        return detection;
    }
    let forced = match hint.as_deref() {
        Some("sql") => Some(EntryType::Sql),
        Some("bash" | "sh" | "zsh" | "shell") => Some(EntryType::Cmd),
        _ => None,
    };
    if let Some(ty) = forced {
        detection.ty = ty;
        detection.confidence = Confidence::High;
        detection.body = inner;
    } else {
        let (ty, confidence, fields, body) = classify(&inner);
        detection.ty = ty;
        detection.confidence = confidence;
        detection.fields = fields;
        detection.body = body;
    }
    if detection.ty == EntryType::Cmd
        && let Some(stripped) = strip_prompts(&detection.body)
    {
        detection.body = stripped;
        detection.stripped_prompts = true;
    }
    detection
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a valid detection pattern")
}

/// If the whole text is one Markdown fence, its content and its language.
pub fn strip_fence(text: &str) -> Option<(String, Option<String>)> {
    let lines: Vec<&str> = text.lines().collect();
    let (first, rest) = lines.split_first()?;
    let (last, middle) = rest.split_last()?;
    let open = first.trim_start();
    let marker = open.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let run = open.chars().take_while(|c| *c == marker).count();
    if run < 3 {
        return None;
    }
    let info = open[run..].trim();
    if marker == '`' && info.contains('`') {
        return None;
    }
    let closes = |line: &str| {
        let t = line.trim();
        t.len() >= run && t.chars().all(|c| c == marker)
    };
    if !closes(last) || middle.iter().any(|l| closes(l)) {
        return None;
    }
    let hint = info
        .split_whitespace()
        .next()
        .map(|w| w.trim_start_matches('{').to_ascii_lowercase());
    Some((middle.join("\n"), hint.filter(|h| !h.is_empty())))
}

/// Removes `$ ` or `% ` from every non-empty line, but only when every non-empty line has one.
pub fn strip_prompts(text: &str) -> Option<String> {
    let mut any = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            out.push(line);
            continue;
        }
        let rest = line
            .strip_prefix("$ ")
            .or_else(|| line.strip_prefix("% "))?;
        any = true;
        out.push(rest);
    }
    any.then(|| out.join("\n"))
}

fn classify(text: &str) -> (EntryType, Confidence, DraftFields, String) {
    let single = (text.lines().filter(|l| !l.trim().is_empty()).count() == 1).then(|| text.trim());
    if let Some(line) = single {
        if let Some(code) = github_code(line) {
            return (EntryType::Code, Confidence::High, code, String::new());
        }
        if URL.is_match(line) {
            let url = DraftFields::Link {
                url: line.to_string(),
            };
            return (EntryType::Link, Confidence::High, url, String::new());
        }
    }
    if let Some(confidence) = markdown_note(text) {
        return (
            EntryType::Note,
            confidence,
            DraftFields::None,
            text.to_string(),
        );
    }
    let candidates = [
        sumo(text).map(|c| (EntryType::Sumo, c)),
        cloudwatch(text).map(|c| (EntryType::Cw, c)),
        sql(text).map(|c| (EntryType::Sql, c)),
        cmd(text).map(|c| (EntryType::Cmd, c)),
    ];
    let found = candidates.iter().flatten();
    let best = found
        .clone()
        .find(|(_, c)| *c >= Confidence::Medium)
        .or_else(|| found.clone().next());
    let (ty, confidence) = best
        .copied()
        .unwrap_or((EntryType::Prompt, Confidence::Low));
    (ty, confidence, DraftFields::None, text.to_string())
}

static MD_HEADING: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^#{1,6}[ \t]+\S"));
static MD_LIST_ITEM: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?m)^[ \t]*(?:[-*+]|\d+[.)])[ \t]+\S"));
static MD_TABLE_RULE: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?m)^[ \t]*\|?[ \t]*:?-{2,}:?[ \t]*(?:\|[ \t]*:?-{2,}:?[ \t]*)+\|?[ \t]*$")
});
static MD_CALLOUT: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?mi)^>[ \t]*\[!(?:note|tip|important|warning|caution)\]"));

/// Markdown structure (headings, lists, tables, callouts, several paragraphs) means a `note`.
/// A lone `# comment` above a command is not enough: it takes two kinds of structure.
fn markdown_note(text: &str) -> Option<Confidence> {
    let paragraphs = text.split("\n\n").filter(|p| !p.trim().is_empty()).count();
    let kinds = [
        MD_HEADING.is_match(text),
        MD_LIST_ITEM.find_iter(text).count() >= 2,
        MD_TABLE_RULE.is_match(text),
        MD_CALLOUT.is_match(text),
        paragraphs >= 3 && MD_HEADING.is_match(text),
    ]
    .iter()
    .filter(|k| **k)
    .count();
    match kinds {
        3.. => Some(Confidence::High),
        2 => Some(Confidence::Medium),
        _ => None,
    }
}

static URL: LazyLock<Regex> = LazyLock::new(|| re(r"^https?://\S+$"));
static BLOB: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^https?://([^/\s]+)/(.+?)/(?:-/)?blob/([^/\s#?]+)/([^\s#?]+)#L(\d+)(?:-L?(\d+))?$")
});

/// `https://github.com/<owner>/<name>/blob/<ref>/<path>#L10-L20` (and GitLab's `/-/blob/`).
fn github_code(line: &str) -> Option<DraftFields> {
    let caps = BLOB.captures(line)?;
    let host = caps[1].to_ascii_lowercase();
    if !(host.ends_with("github.com") || host.contains("gitlab")) {
        return None;
    }
    let project = &caps[2];
    let repo = if project.matches('/').count() == 1 {
        project.to_string()
    } else if host.contains("gitlab") {
        format!("https://{}/{project}", &caps[1])
    } else {
        return None;
    };
    Some(DraftFields::Code {
        repo,
        git_ref: caps[3].to_string(),
        path: caps[4].to_string(),
        line: caps[5].parse().ok().filter(|l| *l >= 1)?,
        end_line: caps.get(6).and_then(|m| m.as_str().parse().ok()),
        symbol: None,
    })
}

static AT_FIELD: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|[\s,(\[])@[A-Za-z]\w*"));
static SUMO_META: LazyLock<Regex> =
    LazyLock::new(|| re(r"\b_(?:sourceCategory|index|view|collector)\b"));
static SUMO_OP: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\|\s*(timeslice|parse|json|count_distinct|transpose|nodrop|(?:count|sort)\s+by|(?:avg|sum|min|max|pct|count)\s*\([^)]*\)\s*by)\b",
    )
});

fn sumo(text: &str) -> Option<Confidence> {
    let meta = SUMO_META.is_match(text);
    // CloudWatch has `| parse @message …` too: an `@` field blocks the operator route.
    let ops = if AT_FIELD.is_match(text) {
        0
    } else {
        let mut seen: Vec<String> = SUMO_OP
            .captures_iter(text)
            .map(|c| c[1].split_whitespace().next().unwrap_or("").to_string())
            .collect();
        seen.sort();
        seen.dedup();
        seen.len()
    };
    match (meta, ops) {
        (true, 1..) => Some(Confidence::High),
        (true, 0) => Some(Confidence::Medium),
        (false, 2..) => Some(Confidence::Medium),
        (false, 1) => Some(Confidence::Low),
        (false, 0) => None,
    }
}

static CW_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?m)^\s*\|?\s*(?:fields|filter|stats|parse|sort|limit|display|dedup|unnest)\b")
});
static CW_STATS_BY: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^\s*\|?\s*stats\s+\S.*\bby\b"));

fn cloudwatch(text: &str) -> Option<Confidence> {
    let command = CW_COMMAND.is_match(text);
    match (command, AT_FIELD.is_match(text)) {
        (true, true) => Some(Confidence::High),
        (true, false) if CW_STATS_BY.is_match(text) => Some(Confidence::Medium),
        _ => None,
    }
}

const SQL_START: [&str; 8] = [
    "SELECT", "WITH", "INSERT", "UPDATE", "DELETE", "EXPLAIN", "CREATE", "ALTER",
];
static SQL_WORD: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:select|from|where|join|group|order|by|set|into|values|and|or|limit|on|as|having|union|distinct|inner|left|right)\b",
    )
});

/// What must follow the first keyword for it to be SQL and not a sentence that starts with one.
fn sql_marker(keyword: &str) -> Regex {
    match keyword {
        "SELECT" => re(r"(?i)\bfrom\b|;"),
        "WITH" => re(r"(?i)\bas\s*\("),
        "INSERT" => re(r"(?i)\binto\b"),
        "UPDATE" => re(r"(?i)\bset\b"),
        "DELETE" => re(r"(?i)\bfrom\b"),
        "EXPLAIN" => re(
            r"(?is)^\s*explain\s+(?:analyze\s+|verbose\s+|\([^)]*\)\s*)*(?:select|insert|update|delete|with)\b",
        ),
        _ => re(r"(?i)\b(?:table|index|view|database|schema|sequence|trigger|extension)\b"),
    }
}

fn sql(text: &str) -> Option<Confidence> {
    let mut rest = text.trim_start();
    loop {
        if let Some(r) = rest.strip_prefix("--") {
            rest = r.split_once('\n').map_or("", |(_, tail)| tail).trim_start();
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.split_once("*/").map_or("", |(_, tail)| tail).trim_start();
        } else {
            break;
        }
    }
    let first = rest.split(|c: char| !c.is_ascii_alphabetic()).next()?;
    let keyword = first.to_ascii_uppercase();
    if !SQL_START.contains(&keyword.as_str()) {
        return None;
    }
    if sql_marker(&keyword).is_match(rest) {
        let looks_like_sql =
            first == keyword || rest.contains(';') || SQL_WORD.find_iter(rest).count() >= 3;
        return Some(if looks_like_sql {
            Confidence::High
        } else {
            Confidence::Medium
        });
    }
    (rest.split_whitespace().count() <= 3).then_some(Confidence::Low)
}

const COMMANDS: &[&str] = &[
    "aws",
    "kubectl",
    "docker",
    "git",
    "gh",
    "curl",
    "wget",
    "grep",
    "egrep",
    "sed",
    "awk",
    "ssh",
    "scp",
    "rsync",
    "terraform",
    "tofu",
    "helm",
    "jq",
    "yq",
    "ls",
    "cd",
    "cat",
    "echo",
    "export",
    "tail",
    "head",
    "find",
    "tar",
    "gzip",
    "gunzip",
    "zip",
    "unzip",
    "rm",
    "mv",
    "cp",
    "mkdir",
    "touch",
    "chmod",
    "chown",
    "sudo",
    "systemctl",
    "journalctl",
    "ps",
    "top",
    "kill",
    "pkill",
    "npm",
    "yarn",
    "pnpm",
    "npx",
    "node",
    "python",
    "python3",
    "pip",
    "pip3",
    "cargo",
    "go",
    "make",
    "brew",
    "apt",
    "apt-get",
    "yum",
    "dnf",
    "docker-compose",
    "kubectx",
    "kubens",
    "minikube",
    "kind",
    "k9s",
    "stern",
    "kustomize",
    "eksctl",
    "gcloud",
    "az",
    "psql",
    "mysql",
    "sqlite3",
    "redis-cli",
    "mongosh",
    "openssl",
    "dig",
    "nslookup",
    "ping",
    "nc",
    "netstat",
    "ss",
    "lsof",
    "df",
    "du",
    "free",
    "uname",
    "whoami",
    "pwd",
    "env",
    "source",
    "sort",
    "uniq",
    "wc",
    "xargs",
    "tee",
    "date",
    "sleep",
    "for",
    "while",
    "if",
    "case",
    "ansible",
    "bash",
    "sh",
    "zsh",
    "diff",
    "patch",
    "less",
    "watch",
    "vault",
    "consul",
    "nomad",
    "pulumi",
    "sam",
    "cdk",
    "open",
    "ip",
    "tcpdump",
    "nmap",
];
static FLAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|\s)--?[A-Za-z][\w-]*"));
static CHAIN: LazyLock<Regex> = LazyLock::new(|| re(r"\s(?:\||\|\||&&)\s|;\s|;$"));
static REDIRECT: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|\s)(?:\d?>>?|<)\s*\S"));

fn cmd(text: &str) -> Option<Confidence> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() || lines.len() > 8 {
        return None;
    }
    let prompts = strip_prompts(text).is_some();
    let stripped = strip_prompts(text);
    let body = stripped.as_deref().unwrap_or(text);
    let first_line = body.lines().find(|l| !l.trim().is_empty())?;
    let mut words = first_line.split_whitespace().peekable();
    while words
        .peek()
        .is_some_and(|w| w.contains('=') && !w.starts_with(['-', '/', '.']))
    {
        words.next();
    }
    if words.peek() == Some(&"sudo") {
        words.next();
    }
    let program = words.next().unwrap_or("");
    let known =
        COMMANDS.contains(&program) || program.starts_with("./") || program.starts_with("~/");
    // several lines that each start with a known command are a script even without flags
    let every_line_a_command = lines.len() >= 2
        && body.lines().filter(|l| !l.trim().is_empty()).all(|l| {
            let first = l.split_whitespace().next().unwrap_or("");
            COMMANDS.contains(&first)
        });
    let others = [
        every_line_a_command,
        FLAG.is_match(body),
        CHAIN.is_match(body),
        REDIRECT.is_match(body),
        prompts,
    ]
    .iter()
    .filter(|b| **b)
    .count();
    let score = others + usize::from(known);
    if !(known || prompts || others >= 2) {
        return None;
    }
    let words = text.split_whitespace().count();
    match score {
        3.. if known => Some(Confidence::High),
        2.. => Some(Confidence::Medium),
        _ if words <= 3 || prompts => Some(Confidence::Low),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn corpus_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/authoring/detect")
    }

    /// (expected type, is a low-confidence case, name, text)
    fn corpus() -> Vec<(EntryType, bool, String, String)> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(corpus_root()).unwrap() {
            let dir = entry.unwrap().path();
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            if name == "low" {
                for sub in std::fs::read_dir(&dir).unwrap() {
                    let sub = sub.unwrap().path();
                    let ty = EntryType::from_name(sub.file_name().unwrap().to_str().unwrap())
                        .expect("low/<type>");
                    collect(&sub, ty, true, &mut out);
                }
            } else if let Some(ty) = EntryType::from_name(&name) {
                collect(&dir, ty, false, &mut out);
            }
        }
        out.sort_by(|a, b| a.2.cmp(&b.2));
        out
    }

    fn collect(
        dir: &Path,
        ty: EntryType,
        low: bool,
        out: &mut Vec<(EntryType, bool, String, String)>,
    ) {
        for f in std::fs::read_dir(dir).unwrap() {
            let f = f.unwrap().path();
            let text = std::fs::read_to_string(&f).unwrap();
            let name = format!("{}/{}", ty.name(), f.file_name().unwrap().to_string_lossy());
            out.push((
                ty,
                low,
                if low { format!("low/{name}") } else { name },
                text,
            ));
        }
    }

    #[test]
    fn corpus_is_big_enough() {
        let c = corpus();
        assert!(c.len() >= 100, "{} snippets", c.len());
        for ty in EntryType::ALL {
            if ty == EntryType::Runbook {
                continue;
            }
            let n = c.iter().filter(|s| s.0 == ty && !s.1).count();
            assert!(n >= 10, "{} has {n}", ty.name());
        }
    }

    #[test]
    fn at_least_95_percent_are_detected_right() {
        let c = corpus();
        let wrong: Vec<_> = c
            .iter()
            .filter(|(ty, _, _, text)| detect(text).ty != *ty)
            .map(|(ty, _, name, text)| {
                format!("{name}: want {} got {}", ty.name(), detect(text).ty.name())
                    + &format!("\n{text}")
            })
            .collect();
        let ok = c.len() - wrong.len();
        assert!(
            ok * 100 >= c.len() * 95,
            "{ok}/{} right; wrong:\n{}",
            c.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn nothing_detected_high_is_wrong() {
        for (ty, _, name, text) in corpus() {
            let d = detect(&text);
            if d.confidence == Confidence::High {
                assert_eq!(d.ty, ty, "{name} was High and wrong\n{text}");
            }
        }
    }

    #[test]
    fn ambiguous_snippets_are_low() {
        for (ty, low, name, text) in corpus() {
            if low {
                let d = detect(&text);
                assert_eq!(d.confidence, Confidence::Low, "{name}");
                assert_eq!(d.ty, ty, "{name}");
            }
        }
    }

    #[test]
    fn detection_is_deterministic() {
        for (_, _, _, text) in corpus() {
            assert_eq!(detect(&text), detect(&text));
        }
    }

    fn d(text: &str) -> Detection {
        detect(text)
    }

    #[test]
    fn fence_language_hint_wins() {
        let sql = d("```sql\nselect 1\n```");
        assert_eq!(
            (sql.ty, sql.confidence, sql.hint.as_deref()),
            (EntryType::Sql, Confidence::High, Some("sql"))
        );
        assert_eq!(sql.body, "select 1");
        for lang in ["bash", "sh", "zsh", "shell"] {
            let c = d(&format!("```{lang}\nls\n```"));
            assert_eq!(
                (c.ty, c.confidence),
                (EntryType::Cmd, Confidence::High),
                "{lang}"
            );
            assert_eq!(c.body, "ls");
        }
        // a hint beats what the content looks like
        assert_eq!(d("```bash\nSELECT * FROM t;\n```").ty, EntryType::Cmd);
        // an unknown language is only unwrapped
        let json = d("```json\n{\"a\": 1}\n```");
        assert_eq!(json.hint.as_deref(), Some("json"));
        assert_eq!(json.body, "{\"a\": 1}");
        // a tilde fence works the same
        assert_eq!(d("~~~sql\nselect 1\n~~~").ty, EntryType::Sql);
        // text around the fence is not a single fence
        assert!(d("see:\n```sql\nselect 1\n```").hint.is_none());
        // two fences are not one
        assert!(d("```\na\n```\n\n```\nb\n```").hint.is_none());
    }

    #[test]
    fn prompt_markers_are_stripped_only_when_on_every_line() {
        let all = d("$ kubectl get pods\n$ kubectl get nodes\n");
        assert_eq!(all.ty, EntryType::Cmd);
        assert_eq!(all.body, "kubectl get pods\nkubectl get nodes");
        assert!(all.stripped_prompts);
        let pct = d("% ls -la");
        assert_eq!(pct.body, "ls -la");
        let some = d("$ kubectl get pods\nNAME READY\n");
        assert!(!some.stripped_prompts);
        assert!(some.body.starts_with("$ kubectl"));
        // not stripped for other types
        let sql = d("```sql\n$ SELECT 1 FROM t;\n```");
        assert_eq!(sql.ty, EntryType::Sql);
        assert_eq!(sql.body, "$ SELECT 1 FROM t;");
    }

    #[test]
    fn github_and_gitlab_urls_become_code() {
        let g = d("https://github.com/acme/api/blob/a1b9c3e/src/main/App.java#L147");
        assert_eq!((g.ty, g.confidence), (EntryType::Code, Confidence::High));
        assert_eq!(
            g.fields,
            DraftFields::Code {
                repo: "acme/api".into(),
                git_ref: "a1b9c3e".into(),
                path: "src/main/App.java".into(),
                line: 147,
                end_line: None,
                symbol: None,
            }
        );
        assert_eq!(g.body, "");
        let r = d("https://github.com/acme/api/blob/main/src/a.rs#L10-L20");
        let DraftFields::Code {
            line,
            end_line,
            git_ref,
            ..
        } = r.fields
        else {
            panic!()
        };
        assert_eq!((line, end_line, git_ref.as_str()), (10, Some(20), "main"));
        let gl = d("https://gitlab.com/acme/platform/-/blob/main/app.py#L33");
        let DraftFields::Code { repo, path, .. } = gl.fields else {
            panic!()
        };
        assert_eq!((repo.as_str(), path.as_str()), ("acme/platform", "app.py"));
        let nested = d("https://gitlab.com/acme/sub/group/-/blob/main/db/schema.sql#L400");
        let DraftFields::Code { repo, .. } = nested.fields else {
            panic!()
        };
        assert_eq!(repo, "https://gitlab.com/acme/sub/group");
        // a blob URL without a line anchor is a plain link
        assert_eq!(
            d("https://github.com/acme/api/blob/main/README.md").ty,
            EntryType::Link
        );
    }

    #[test]
    fn a_single_url_is_a_link() {
        let l = d("  https://example.com/a?b=1  \n");
        assert_eq!((l.ty, l.confidence), (EntryType::Link, Confidence::High));
        assert_eq!(
            l.fields,
            DraftFields::Link {
                url: "https://example.com/a?b=1".into()
            }
        );
        assert_eq!(l.body, "");
        // a URL inside prose is not a link
        assert_ne!(d("read https://example.com/a first").ty, EntryType::Link);
        assert_ne!(
            d("https://a.example/x\nhttps://b.example/y").ty,
            EntryType::Link
        );
    }

    #[test]
    fn crlf_is_normalized() {
        let c = d("SELECT a\r\nFROM t;\r\n");
        assert_eq!(c.body, "SELECT a\nFROM t;");
        assert_eq!(normalize_clipboard("a\rb\r\nc\n\n\n"), "a\nb\nc");
        assert_eq!(normalize_clipboard("\n\n  x\n"), "  x");
    }

    #[test]
    fn empty_text_is_cmd_low_with_an_empty_body() {
        for t in ["", "   ", "\n\n\t\n"] {
            let e = d(t);
            assert_eq!(
                (e.ty, e.confidence, e.body.as_str()),
                (EntryType::Cmd, Confidence::Low, "")
            );
        }
    }

    #[test]
    fn cloudwatch_parse_is_not_sumo() {
        let cw = d(
            "fields @timestamp, @message\n| parse @message \"user=*\" as user\n| stats count() by user",
        );
        assert_eq!(cw.ty, EntryType::Cw);
        let cw2 = d("parse @message \"a=*\" as a\n| timeslice 5m\n| count by a");
        assert_eq!(
            cw2.ty,
            EntryType::Cw,
            "an @ field blocks the Sumo operator route"
        );
        // metadata fields are always Sumo, even next to an @ sign
        assert_eq!(d("_sourceCategory=prod/x \"a@b.com\"").ty, EntryType::Sumo);
    }

    #[test]
    fn sumo_markers() {
        for t in [
            "_sourceCategory=a/b",
            "_index=main error",
            "_view=v x",
            "_collector=c y",
        ] {
            assert_eq!(d(t).ty, EntryType::Sumo, "{t}");
        }
        assert_eq!(
            d("error | timeslice 5m | count by _timeslice").ty,
            EntryType::Sumo
        );
    }

    #[test]
    fn prose_with_command_words_is_not_cloudwatch() {
        let p = d(
            "Please filter the list, sort it by date and limit it to ten items.\nThen display the result.",
        );
        assert_eq!(p.ty, EntryType::Prompt);
        assert_eq!(p.confidence, Confidence::Low);
    }

    #[test]
    fn long_text_is_a_prompt_even_with_a_command_first() {
        let long = (0..12)
            .map(|i| format!("line {i} of a long explanation"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(d(&format!("git is a tool\n{long}")).ty, EntryType::Prompt);
    }
}
