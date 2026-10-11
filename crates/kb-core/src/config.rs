//! The settings: one table (`SETTINGS`), loaded from flags, environment and a TOML file, and
//! edited without losing comments. See specs/004-platform-clipboard/contracts/config.md.

use std::fmt;
use std::path::{Path, PathBuf};

/// Which colors the TUI uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    #[default]
    Auto,
    Dark,
    Light,
}

// ===================================================================================
// M4: the settings table, loading with precedence, and warnings (see specs/004-platform-clipboard)
// ===================================================================================

use std::collections::BTreeMap;

/// What kind of value a setting holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Path,
    Text,
    Bool,
    Int,
    Choice(&'static [&'static str]),
}

/// One row of the settings table.
#[derive(Debug, Clone, Copy)]
pub struct Setting {
    pub key: &'static str,
    pub kind: Kind,
    /// The default as text; empty means "unset" for `Path` and `Text`.
    pub default: &'static str,
    pub env: &'static str,
    pub doc: &'static str,
}

const THEMES: &[&str] = &["auto", "dark", "light"];

/// The twenty-two settings. `config init`, `list`, `get`, `set` and the loader are all driven by this
/// table, so none of them can disagree about a key, its type, its default or its variable.
pub const SETTINGS: &[Setting] = &[
    Setting {
        key: "root",
        kind: Kind::Path,
        default: "",
        env: "KB_ROOT",
        doc: "Where your entries live (default ~/kb). A leading ~ is expanded.",
    },
    Setting {
        key: "editor",
        kind: Kind::Text,
        default: "",
        env: "KB_EDITOR",
        doc: "Command for Ctrl-E and `kb edit`, arguments allowed. Unset: $VISUAL, then $EDITOR, then vi.",
    },
    Setting {
        key: "path_template",
        kind: Kind::Text,
        default: "",
        env: "KB_PATH_TEMPLATE",
        doc: "Where `kb add` puts new entries: placeholders {type}, {slug} and {tag} (the first tag, else the type); must contain {slug}. Unset: {type}/{slug}.",
    },
    Setting {
        key: "theme",
        kind: Kind::Choice(THEMES),
        default: "auto",
        env: "KB_THEME",
        doc: "auto | dark | light. auto reads the terminal background and falls back to dark.",
    },
    Setting {
        key: "tui.enter",
        kind: Kind::Choice(&["copy", "print", "primary"]),
        default: "copy",
        env: "KB_TUI_ENTER",
        doc: "What Enter does in the UI: copy the payload, print it to stdout, or execute primary open action.",
    },
    Setting {
        key: "tui.mouse",
        kind: Kind::Bool,
        default: "false",
        env: "KB_TUI_MOUSE",
        doc: "Wheel scrolling and click to select. Off keeps terminal text selection working.",
    },
    Setting {
        key: "clipboard.method",
        kind: Kind::Choice(&["auto", "native", "osc52", "stdout"]),
        default: "auto",
        env: "KB_CLIPBOARD_METHOD",
        doc: "auto | native | osc52 | stdout. A method that fails falls back to the next one.",
    },
    Setting {
        key: "clipboard.command",
        kind: Kind::Text,
        default: "",
        env: "KB_CLIPBOARD_COMMAND",
        doc: "A custom copy command that reads the text on stdin, for example \"xclip -selection clipboard\". Do not put secrets here.",
    },
    Setting {
        key: "clipboard.osc52_max_bytes",
        kind: Kind::Int,
        default: "74000",
        env: "KB_CLIPBOARD_OSC52_MAX_BYTES",
        doc: "Largest payload (bytes of text) sent through the terminal; bigger ones are refused.",
    },
    Setting {
        key: "clipboard.trim_trailing_newline",
        kind: Kind::Bool,
        default: "true",
        env: "KB_CLIPBOARD_TRIM_TRAILING_NEWLINE",
        doc: "Remove trailing newlines from the copied text.",
    },
    Setting {
        key: "opener.command",
        kind: Kind::Text,
        default: "",
        env: "KB_OPENER_COMMAND",
        doc: "Command that opens a URL or file, given as its argument. Unset: open (macOS) or xdg-open.",
    },
    Setting {
        key: "vars.remember",
        kind: Kind::Bool,
        default: "true",
        env: "KB_VARS_REMEMBER",
        doc: "Remember the values you use for variables (never secrets), to pre-fill them next time.",
    },
    Setting {
        key: "sumo.base_url",
        kind: Kind::Text,
        default: "",
        env: "KB_SUMO_BASE_URL",
        doc: "Sumo Logic web deployment base URL, e.g. https://service.us2.sumologic.com.",
    },
    Setting {
        key: "aws.default_region",
        kind: Kind::Text,
        default: "",
        env: "KB_AWS_DEFAULT_REGION",
        doc: "Default AWS region for CloudWatch console links when entry.region is unset.",
    },
    Setting {
        key: "code.default_open",
        kind: Kind::Choice(&["web", "vscode", "idea"]),
        default: "web",
        env: "KB_CODE_DEFAULT_OPEN",
        doc: "Default target when opening code: web | vscode | idea.",
    },
    Setting {
        key: "code.web_url",
        kind: Kind::Text,
        default: "",
        env: "KB_CODE_WEB_URL",
        doc: "Custom web permalink template for non-GitHub/GitLab/Bitbucket hosts.",
    },
    Setting {
        key: "run.pause_after",
        kind: Kind::Bool,
        default: "true",
        env: "KB_RUN_PAUSE_AFTER",
        doc: "Whether to pause and show exit code and duration after running a command.",
    },
    Setting {
        key: "git.auto_commit",
        kind: Kind::Bool,
        default: "true",
        env: "KB_GIT_AUTO_COMMIT",
        doc: "Automatically commit entries created, edited, moved, removed, or repinned through kb.",
    },
    Setting {
        key: "git.commit_template",
        kind: Kind::Text,
        default: "kb: {action} {id}",
        env: "KB_GIT_COMMIT_TEMPLATE",
        doc: "Template used for generating commit messages ({action}, {id}).",
    },
    Setting {
        key: "sync.strategy",
        kind: Kind::Choice(&["rebase", "merge"]),
        default: "rebase",
        env: "KB_SYNC_STRATEGY",
        doc: "Integration strategy for kb sync: rebase | merge.",
    },
    Setting {
        key: "sync.timeout",
        kind: Kind::Int,
        default: "30",
        env: "KB_SYNC_TIMEOUT",
        doc: "Foreground network timeout in seconds for fetch and push operations.",
    },
    Setting {
        key: "sync.fetch_on_start",
        kind: Kind::Bool,
        default: "false",
        env: "KB_SYNC_FETCH_ON_START",
        doc: "Whether the TUI starts a non-blocking background fetch on startup.",
    },
];

/// A value as it was written, before it is checked against the setting's kind.
#[derive(Debug, Clone, PartialEq)]
enum Raw {
    Str(String),
    Bool(bool),
    Int(i64),
    /// Something else (an array, a date, a float), described for the message.
    Other(&'static str),
}

impl Raw {
    fn shown(&self) -> String {
        match self {
            Raw::Str(s) => format!("{s:?}"),
            Raw::Bool(b) => b.to_string(),
            Raw::Int(i) => i.to_string(),
            Raw::Other(what) => (*what).to_string(),
        }
    }
}

impl Kind {
    /// Checks `raw` and returns the canonical text, or what was expected. `lenient` is for
    /// environment variables and flags, which are always text: `1`, `yes` and `no` count as
    /// booleans there but not in the file.
    fn check(self, raw: &Raw, lenient: bool) -> Result<String, String> {
        match (self, raw) {
            (Kind::Path | Kind::Text, Raw::Str(s)) => Ok(s.clone()),
            (Kind::Path | Kind::Text, _) => Err("expected a string".into()),
            (Kind::Bool, Raw::Bool(b)) => Ok(b.to_string()),
            (Kind::Bool, Raw::Str(s)) if lenient => match s.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => Ok("true".into()),
                "false" | "0" | "no" => Ok("false".into()),
                _ => Err("expected true or false".into()),
            },
            (Kind::Bool, _) => Err("expected true or false".into()),
            (Kind::Int, Raw::Int(i)) if *i >= 1 => Ok(i.to_string()),
            (Kind::Int, Raw::Str(s))
                if lenient && s.trim().parse::<i64>().is_ok_and(|i| i >= 1) =>
            {
                Ok(s.trim().parse::<i64>().unwrap_or(1).to_string())
            }
            (Kind::Int, _) => Err("expected a whole number of at least 1".into()),
            (Kind::Choice(list), Raw::Str(s)) if list.contains(&s.as_str()) => Ok(s.clone()),
            (Kind::Choice(list), _) => Err(format!("expected one of {}", list.join(", "))),
        }
    }
}

fn setting(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// `using "auto"` or, for settings that are unset by default, `using the default`.
fn using(s: &Setting) -> String {
    if s.default.is_empty() {
        "using the default".into()
    } else {
        format!("using {:?}", s.default)
    }
}

/// Where a value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Default,
    File(PathBuf),
    Env(&'static str),
    Flag(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved<T> {
    pub value: T,
    pub source: Source,
    pub line: Option<usize>,
}

impl<T> Resolved<T> {
    /// `default`, `file`, `env` or `flag`.
    pub fn source_name(&self) -> &'static str {
        match self.source {
            Source::Default => "default",
            Source::File(_) => "file",
            Source::Env(_) => "env",
            Source::Flag(_) => "flag",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterAction {
    Copy,
    Print,
    Primary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncStrategy {
    #[default]
    Rebase,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CustomActionInput {
    #[default]
    Stdin,
    Arg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomActionConfig {
    pub entry_type: String,
    pub name: String,
    pub key: char,
    pub command: String,
    pub input: CustomActionInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardChoice {
    Auto,
    Native,
    Osc52,
    Stdout,
}

/// Every setting with its value and source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    entries: BTreeMap<&'static str, Resolved<String>>,
    code_search_paths: Vec<String>,
    code_repos: BTreeMap<String, String>,
    run_destructive_patterns: Vec<String>,
    custom_actions: Vec<CustomActionConfig>,
}

/// Values given on the command line that map to settings.
#[derive(Debug, Clone, Default)]
pub struct Flags {
    pub root: Option<String>,
    pub clipboard_method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWarning {
    pub path: Option<PathBuf>,
    pub line: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub settings: Settings,
    pub warnings: Vec<ConfigWarning>,
    /// The file the values came from, if any.
    pub file: Option<PathBuf>,
    /// The file is the pre-M4 `config.yml`.
    pub legacy: bool,
    /// Other config files that exist but are not used (macOS has two locations).
    pub ignored: Vec<PathBuf>,
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.path, self.line) {
            (Some(p), Some(line)) => write!(f, "{}:{line}: {}", p.display(), self.message),
            (Some(p), None) => write!(f, "{}: {}", p.display(), self.message),
            (None, _) => f.write_str(&self.message),
        }
    }
}

impl Settings {
    fn defaults() -> Settings {
        Settings {
            entries: SETTINGS
                .iter()
                .map(|s| {
                    (
                        s.key,
                        Resolved {
                            value: s.default.to_string(),
                            source: Source::Default,
                            line: None,
                        },
                    )
                })
                .collect(),
            code_search_paths: Vec::new(),
            code_repos: BTreeMap::new(),
            run_destructive_patterns: Vec::new(),
            custom_actions: Vec::new(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Settings {
    fn put(&mut self, s: &'static Setting, value: String, source: Source, line: Option<usize>) {
        self.entries.insert(
            s.key,
            Resolved {
                value,
                source,
                line,
            },
        );
    }

    pub fn get(&self, key: &str) -> Option<&Resolved<String>> {
        self.entries.get(key)
    }

    pub fn set_text(&mut self, key: &str, value: &str) {
        if let Some(s) = SETTINGS.iter().find(|s| s.key == key) {
            self.put(s, value.to_string(), Source::Default, None);
        }
    }

    /// Settings in table order.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &Resolved<String>)> {
        SETTINGS
            .iter()
            .filter_map(|s| self.entries.get_key_value(s.key).map(|(k, v)| (*k, v)))
    }

    fn text(&self, key: &str) -> &str {
        self.entries.get(key).map_or("", |r| r.value.as_str())
    }

    fn optional(&self, key: &str) -> Option<&str> {
        Some(self.text(key)).filter(|v| !v.is_empty())
    }

    fn flag(&self, key: &str) -> bool {
        self.text(key) == "true"
    }

    pub fn root(&self) -> Option<&str> {
        self.optional("root")
    }

    pub fn editor(&self) -> Option<&str> {
        self.optional("editor")
    }

    pub fn theme(&self) -> ThemeChoice {
        match self.text("theme") {
            "dark" => ThemeChoice::Dark,
            "light" => ThemeChoice::Light,
            _ => ThemeChoice::Auto,
        }
    }

    pub fn tui_enter(&self) -> EnterAction {
        match self.text("tui.enter") {
            "print" => EnterAction::Print,
            "primary" => EnterAction::Primary,
            _ => EnterAction::Copy,
        }
    }

    pub fn vars_remember(&self) -> bool {
        self.flag("vars.remember")
    }

    pub fn tui_mouse(&self) -> bool {
        self.flag("tui.mouse")
    }

    pub fn clipboard_method(&self) -> ClipboardChoice {
        match self.text("clipboard.method") {
            "native" => ClipboardChoice::Native,
            "osc52" => ClipboardChoice::Osc52,
            "stdout" => ClipboardChoice::Stdout,
            _ => ClipboardChoice::Auto,
        }
    }

    pub fn clipboard_command(&self) -> Option<&str> {
        self.optional("clipboard.command")
    }

    pub fn osc52_max_bytes(&self) -> usize {
        self.text("clipboard.osc52_max_bytes")
            .parse()
            .unwrap_or(74_000)
    }

    pub fn trim_trailing_newline(&self) -> bool {
        self.flag("clipboard.trim_trailing_newline")
    }

    pub fn opener_command(&self) -> Option<&str> {
        self.optional("opener.command")
    }

    pub fn sumo_base_url(&self) -> Option<&str> {
        self.optional("sumo.base_url")
    }

    pub fn aws_default_region(&self) -> Option<&str> {
        self.optional("aws.default_region")
    }

    pub fn code_default_open(&self) -> &str {
        self.text("code.default_open")
    }

    pub fn code_web_url(&self) -> Option<&str> {
        self.optional("code.web_url")
    }

    pub fn run_pause_after(&self) -> bool {
        self.flag("run.pause_after")
    }

    pub fn git_auto_commit(&self) -> bool {
        self.flag("git.auto_commit")
    }

    pub fn git_commit_template(&self) -> &str {
        self.text("git.commit_template")
    }

    pub fn sync_strategy(&self) -> SyncStrategy {
        match self.text("sync.strategy") {
            "merge" => SyncStrategy::Merge,
            _ => SyncStrategy::Rebase,
        }
    }

    pub fn sync_timeout(&self) -> u64 {
        self.text("sync.timeout").parse().unwrap_or(30)
    }

    pub fn sync_fetch_on_start(&self) -> bool {
        self.flag("sync.fetch_on_start")
    }

    pub fn code_search_paths(&self) -> &[String] {
        &self.code_search_paths
    }

    pub fn code_search_paths_mut(&mut self) -> &mut Vec<String> {
        &mut self.code_search_paths
    }

    pub fn code_repos(&self) -> &BTreeMap<String, String> {
        &self.code_repos
    }

    pub fn code_repos_mut(&mut self) -> &mut BTreeMap<String, String> {
        &mut self.code_repos
    }

    pub fn run_destructive_patterns(&self) -> &[String] {
        &self.run_destructive_patterns
    }

    pub fn custom_actions(&self) -> &[CustomActionConfig] {
        &self.custom_actions
    }

    /// The template for new entry IDs; the default when unset (an invalid value was already
    /// refused with a warning when the settings were loaded).
    pub fn path_template(&self) -> PathTemplate {
        self.optional("path_template")
            .and_then(|t| PathTemplate::parse(t).ok())
            .unwrap_or_default()
    }
}

/// One piece of a [`PathTemplate`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Type,
    Slug,
    Tag,
}

/// Where `kb add` puts a new entry: literal text with `{type}`, `{slug}` and `{tag}` (the first
/// tag, or the type when there are none). Not a template engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTemplate {
    parts: Vec<Part>,
}

impl Default for PathTemplate {
    fn default() -> PathTemplate {
        PathTemplate {
            parts: vec![Part::Type, Part::Literal("/".into()), Part::Slug],
        }
    }
}

impl PathTemplate {
    /// Parses a template. It must contain `{slug}`; `{type}`, `{slug}` and `{tag}` are the only
    /// placeholders. An empty string is the default, `{type}/{slug}`.
    pub fn parse(s: &str) -> Result<PathTemplate, String> {
        if s.is_empty() {
            return Ok(PathTemplate::default());
        }
        let mut parts = Vec::new();
        let mut rest = s;
        while !rest.is_empty() {
            match rest.find(['{', '}']) {
                None => {
                    parts.push(Part::Literal(rest.to_string()));
                    break;
                }
                Some(at) => {
                    if at > 0 {
                        parts.push(Part::Literal(rest[..at].to_string()));
                    }
                    let tail = &rest[at..];
                    let Some(end) = tail.strip_prefix('{').and_then(|t| t.find('}')) else {
                        return Err(format!("unbalanced brace in {s:?}"));
                    };
                    let name = &tail[1..=end];
                    parts.push(match name {
                        "type" => Part::Type,
                        "slug" => Part::Slug,
                        "tag" => Part::Tag,
                        other => {
                            return Err(format!(
                                "unknown placeholder {{{other}}}; use {{type}}, {{slug}} or {{tag}}"
                            ));
                        }
                    });
                    rest = &tail[end + 2..];
                }
            }
        }
        if !parts.contains(&Part::Slug) {
            return Err("the template must contain {slug}".into());
        }
        Ok(PathTemplate { parts })
    }

    /// The ID for an entry of type `ty` with this slug and these tags.
    pub fn render(&self, ty: crate::entry::EntryType, slug: &str, tags: &[String]) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Literal(l) => out.push_str(l),
                Part::Type => out.push_str(ty.name()),
                Part::Slug => out.push_str(slug),
                Part::Tag => out.push_str(tags.first().map_or(ty.name(), String::as_str)),
            }
        }
        out
    }
}

/// A setting found in a file, before it is checked.
struct Leaf {
    key: String,
    raw: Raw,
    line: usize,
}

fn line_of(text: &str, offset: usize) -> usize {
    1 + text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
}

fn raw_of(value: &toml_edit::Value) -> Raw {
    use toml_edit::Value;
    match value {
        Value::String(s) => Raw::Str(s.value().clone()),
        Value::Boolean(b) => Raw::Bool(*b.value()),
        Value::Integer(i) => Raw::Int(*i.value()),
        Value::Float(_) => Raw::Other("a number with decimals"),
        Value::Datetime(_) => Raw::Other("a date"),
        Value::Array(_) => Raw::Other("an array"),
        Value::InlineTable(_) => Raw::Other("a table"),
    }
}

/// Every value under `table`, with its dotted key and line, in file order. Dotted keys, `[tables]`
/// and inline tables all come out the same way.
fn collect(table: &dyn toml_edit::TableLike, prefix: &str, text: &str, out: &mut Vec<Leaf>) {
    use toml_edit::{Item, Value};
    for (name, item) in table.iter() {
        let key = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        };
        if key == "code.search_paths"
            || key == "code.repos"
            || key.starts_with("code.repos.")
            || key == "run.destructive_patterns"
            || key == "actions"
            || key.starts_with("actions.")
        {
            continue;
        }
        match item {
            Item::Table(t) => collect(t, &key, text, out),
            Item::Value(Value::InlineTable(t)) => collect(t, &key, text, out),
            Item::Value(v) => out.push(Leaf {
                line: line_of(text, v.span().map_or(0, |r| r.start)),
                key,
                raw: raw_of(v),
            }),
            Item::ArrayOfTables(a) => out.push(Leaf {
                line: line_of(text, a.span().map_or(0, |r| r.start)),
                key,
                raw: Raw::Other("an array of tables"),
            }),
            Item::None => {}
        }
    }
}

fn is_ext(path: &Path, ext: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ext.contains(&e))
}

struct Loader {
    settings: Settings,
    warnings: Vec<ConfigWarning>,
}

impl Loader {
    fn warn(&mut self, path: Option<&Path>, line: Option<usize>, message: String) {
        self.warnings.push(ConfigWarning {
            path: path.map(Path::to_path_buf),
            line,
            message,
        });
    }

    /// Checks one value and stores it, or warns and keeps what was there.
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &mut self,
        s: &'static Setting,
        raw: &Raw,
        lenient: bool,
        source: Source,
        line: Option<usize>,
        path: Option<&Path>,
        label: &str,
    ) {
        let checked = s.kind.check(raw, lenient).and_then(|v| match s.key {
            "path_template" => PathTemplate::parse(&v).map(|_| v),
            _ => Ok(v),
        });
        match checked {
            Ok(v) => self.settings.put(s, v, source, line),
            Err(expected) => {
                let message = format!(
                    "{label}: {} is not valid ({expected}); {}",
                    raw.shown(),
                    using(s)
                );
                self.warn(path, line, message);
            }
        }
    }

    fn toml_file(&mut self, path: &Path, text: &str) {
        use toml_edit::Item;
        let doc = match toml_edit::Document::parse(text) {
            Ok(d) => d,
            Err(e) => {
                let line = e.span().map(|r| line_of(text, r.start));
                self.warn(
                    Some(path),
                    line,
                    format!("could not parse the file: {}; using defaults", e.message()),
                );
                return;
            }
        };

        // 1. code.search_paths
        if let Some(code_item) = doc.get("code")
            && let Some(code_tbl) = code_item.as_table_like()
        {
            if let Some(sp_item) = code_tbl.get("search_paths") {
                if let Some(arr) = sp_item.as_array() {
                    for val in arr.iter() {
                        if let Some(s) = val.as_str() {
                            self.settings.code_search_paths.push(s.to_string());
                        } else {
                            let line = val.span().map(|r| line_of(text, r.start));
                            self.warn(
                                Some(path),
                                line,
                                "code.search_paths: expected string elements".into(),
                            );
                        }
                    }
                } else {
                    let line = sp_item.span().map(|r| line_of(text, r.start));
                    self.warn(
                        Some(path),
                        line,
                        "code.search_paths: expected an array of strings".into(),
                    );
                }
            }

            if let Some(repos_item) = code_tbl.get("repos") {
                if let Some(repos_tbl) = repos_item.as_table_like() {
                    for (repo_name, val_item) in repos_tbl.iter() {
                        if let Some(val_str) = val_item.as_str() {
                            self.settings
                                .code_repos
                                .insert(repo_name.to_string(), val_str.to_string());
                        } else {
                            let line = val_item.span().map(|r| line_of(text, r.start));
                            self.warn(
                                Some(path),
                                line,
                                format!("code.repos.{repo_name}: expected a string path"),
                            );
                        }
                    }
                } else {
                    let line = repos_item.span().map(|r| line_of(text, r.start));
                    self.warn(Some(path), line, "code.repos: expected a table".into());
                }
            }
        }

        // 2. run.destructive_patterns
        if let Some(run_item) = doc.get("run")
            && let Some(run_tbl) = run_item.as_table_like()
            && let Some(dp_item) = run_tbl.get("destructive_patterns")
        {
            if let Some(arr) = dp_item.as_array() {
                for val in arr.iter() {
                    if let Some(s) = val.as_str() {
                        self.settings.run_destructive_patterns.push(s.to_string());
                    } else {
                        let line = val.span().map(|r| line_of(text, r.start));
                        self.warn(
                            Some(path),
                            line,
                            "run.destructive_patterns: expected string elements".into(),
                        );
                    }
                }
            } else {
                let line = dp_item.span().map(|r| line_of(text, r.start));
                self.warn(
                    Some(path),
                    line,
                    "run.destructive_patterns: expected an array of strings".into(),
                );
            }
        }

        // 3. [[actions.<type>]]
        if let Some(actions_item) = doc.get("actions")
            && let Some(actions_tbl) = actions_item.as_table_like()
        {
            for (entry_type, type_item) in actions_tbl.iter() {
                if let Item::ArrayOfTables(arr) = type_item {
                    for table in arr.iter() {
                        let line = table.span().map(|r| line_of(text, r.start));
                        let name = table.get("name").and_then(|v| v.as_str());
                        let key = table.get("key").and_then(|v| v.as_str());
                        let command = table.get("command").and_then(|v| v.as_str());
                        let input_str = table
                            .get("input")
                            .and_then(|v| v.as_str())
                            .unwrap_or("stdin");

                        if name.is_none() || key.is_none() || command.is_none() {
                            self.warn(
                                Some(path),
                                line,
                                format!(
                                    "actions.{entry_type}: custom action requires name, key, and command"
                                ),
                            );
                            continue;
                        }
                        let key_chars: Vec<char> = key.unwrap().chars().collect();
                        if key_chars.len() != 1 {
                            self.warn(
                                Some(path),
                                line,
                                format!(
                                    "actions.{entry_type}: key must be a single character, got {:?}",
                                    key.unwrap()
                                ),
                            );
                            continue;
                        }
                        let input = match input_str {
                            "arg" => CustomActionInput::Arg,
                            "stdin" => CustomActionInput::Stdin,
                            other => {
                                self.warn(
                                    Some(path),
                                    line,
                                    format!(
                                        "actions.{entry_type}: invalid input {other:?}, expected stdin or arg"
                                    ),
                                );
                                CustomActionInput::Stdin
                            }
                        };
                        self.settings.custom_actions.push(CustomActionConfig {
                            entry_type: entry_type.to_string(),
                            name: name.unwrap().to_string(),
                            key: key_chars[0],
                            command: command.unwrap().to_string(),
                            input,
                        });
                    }
                } else {
                    let line = type_item.span().map(|r| line_of(text, r.start));
                    self.warn(
                        Some(path),
                        line,
                        format!("actions.{entry_type}: expected an array of tables"),
                    );
                }
            }
        }

        let mut leaves = Vec::new();
        collect(doc.as_table(), "", text, &mut leaves);
        for leaf in leaves {
            match setting(&leaf.key) {
                None => self.warn(
                    Some(path),
                    Some(leaf.line),
                    format!("unknown key {:?} (ignored)", leaf.key),
                ),
                Some(s) => self.apply(
                    s,
                    &leaf.raw,
                    false,
                    Source::File(path.to_path_buf()),
                    Some(leaf.line),
                    Some(path),
                    s.key,
                ),
            }
        }
    }

    /// The pre-M4 `config.yml`: `root`, `theme` and `mouse` (now `tui.mouse`).
    fn legacy_file(&mut self, path: &Path, text: &str) {
        self.warn(
            Some(path),
            None,
            "deprecated; run `kb config init` and move your settings to config.toml".into(),
        );
        let parsed = crate::yaml::parse(text, 0);
        if let Some(e) = parsed.errors.first() {
            self.warn(
                Some(path),
                None,
                format!("could not parse the file: {}; using defaults", e.message),
            );
            return;
        }
        let Some(root) = parsed.root else { return };
        for (old, new) in [("root", "root"), ("theme", "theme"), ("mouse", "tui.mouse")] {
            let Some(node) = root.get(old).filter(|n| !n.is_null()) else {
                continue;
            };
            let Some(s) = setting(new) else { continue };
            let raw = Raw::Str(node.scalar().unwrap_or_default().to_string());
            self.apply(
                s,
                &raw,
                true,
                Source::File(path.to_path_buf()),
                None,
                Some(path),
                old,
            );
        }
    }
}

/// Loads the settings. `files` is the platform's read order (`config.toml` candidates first, then
/// the legacy `config.yml` ones). Never fails: problems become warnings and the default is used.
pub fn load(files: &[PathBuf], get_env: &dyn Fn(&str) -> Option<String>, flags: &Flags) -> Loaded {
    let mut l = Loader {
        settings: Settings::defaults(),
        warnings: Vec::new(),
    };
    let toml: Vec<&PathBuf> = files
        .iter()
        .filter(|p| is_ext(p, &["toml"]) && p.is_file())
        .collect();
    let yml: Vec<&PathBuf> = files
        .iter()
        .filter(|p| is_ext(p, &["yml", "yaml"]) && p.is_file())
        .collect();
    let (file, legacy) = match (toml.first(), yml.first()) {
        (Some(p), _) => (Some((*p).clone()), false),
        (None, Some(p)) => (Some((*p).clone()), true),
        (None, None) => (None, false),
    };
    let ignored: Vec<PathBuf> = toml.iter().skip(1).map(|p| (*p).clone()).collect();
    if let Some(path) = &file {
        match std::fs::read_to_string(path) {
            Ok(text) if legacy => l.legacy_file(path, &text),
            Ok(text) => l.toml_file(path, &text),
            Err(e) => l.warn(
                Some(path),
                None,
                format!("could not read the file: {e}; using defaults"),
            ),
        }
    }
    for s in SETTINGS {
        if let Some(v) = get_env(s.env).filter(|v| !v.is_empty()) {
            l.apply(s, &Raw::Str(v), true, Source::Env(s.env), None, None, s.env);
        }
    }
    if let Some(v) = &flags.root
        && let Some(s) = setting("root")
    {
        l.apply(
            s,
            &Raw::Str(v.clone()),
            true,
            Source::Flag("--root"),
            None,
            None,
            "--root",
        );
    }
    if let Some(v) = &flags.clipboard_method
        && let Some(s) = setting("clipboard.method")
    {
        l.apply(
            s,
            &Raw::Str(v.clone()),
            true,
            Source::Flag("--method"),
            None,
            None,
            "--method",
        );
    }
    Loaded {
        settings: l.settings,
        warnings: l.warnings,
        file,
        legacy,
        ignored,
    }
}

// ---- editing the file ----

/// Why a setting could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigEditError {
    UnknownKey(String),
    /// The value does not fit the setting; the text says what was expected.
    Invalid {
        key: String,
        message: String,
    },
    /// The existing file is not valid TOML, so it cannot be edited safely.
    NotToml(String),
}

impl fmt::Display for ConfigEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigEditError::UnknownKey(key) => {
                write!(f, "unknown setting {key:?} (see `kb config list`)")
            }
            ConfigEditError::Invalid { key, message } => write!(f, "{key}: {message}"),
            ConfigEditError::NotToml(message) => {
                write!(
                    f,
                    "the config file is not valid TOML ({message}); fix it by hand first"
                )
            }
        }
    }
}

impl std::error::Error for ConfigEditError {}

/// `text` with `key` set to `value`, keeping every other byte (comments, order, spacing).
pub fn set_value(text: &str, key: &str, value: &str) -> Result<String, ConfigEditError> {
    use toml_edit::{DocumentMut, Item, Table, Value};
    let s = setting(key).ok_or_else(|| ConfigEditError::UnknownKey(key.to_string()))?;
    let canonical = s
        .kind
        .check(&Raw::Str(value.to_string()), true)
        .map_err(|message| ConfigEditError::Invalid {
            key: key.to_string(),
            message: format!(
                "{} is not valid ({message})",
                Raw::Str(value.to_string()).shown()
            ),
        })?;
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| ConfigEditError::NotToml(e.message().to_string()))?;
    let mut new_value: Value = match s.kind {
        Kind::Bool => (canonical == "true").into(),
        Kind::Int => canonical.parse::<i64>().unwrap_or(1).into(),
        _ => canonical.as_str().into(),
    };
    let (table_name, name) = match key.split_once('.') {
        Some((t, n)) => (Some(t), n),
        None => (None, key),
    };
    let was_empty = doc.as_table().is_empty() && text.trim().is_empty();
    let table: &mut dyn toml_edit::TableLike = match table_name {
        None => doc.as_table_mut(),
        Some(t) => {
            let item = doc.entry(t).or_insert_with(|| {
                let mut table = Table::new();
                if !was_empty {
                    table.decor_mut().set_prefix("\n");
                }
                Item::Table(table)
            });
            item.as_table_like_mut()
                .ok_or_else(|| ConfigEditError::NotToml(format!("`{t}` is not a table")))?
        }
    };
    match table.get_mut(name) {
        // keep the spacing and the trailing comment of the value being replaced
        Some(Item::Value(old)) => {
            *new_value.decor_mut() = old.decor().clone();
            *old = new_value;
        }
        Some(other) => *other = Item::Value(new_value),
        None => {
            table.insert(name, Item::Value(new_value));
        }
    }
    Ok(doc.to_string())
}

/// The commented file `kb config init` writes: every setting with its default and its doc line.
/// Nothing is active until a `#` is removed, so a later change to a default reaches the user.
pub fn render_template() -> String {
    let mut out = String::from(
        "# kb configuration. Remove the leading # from a setting to change it.\n\
         # Environment variables (KB_<SECTION>_<KEY>) and command-line flags take precedence\n\
         # over this file; `kb config list` shows where each value comes from.\n",
    );
    let mut section: Option<&str> = None;
    for (i, s) in SETTINGS.iter().enumerate() {
        let (this, name) = match s.key.split_once('.') {
            Some((sec, name)) => (Some(sec), name),
            None => (None, s.key),
        };
        if i == 0 || this != section {
            out.push('\n');
            if let Some(sec) = this {
                out.push_str(&format!("[{sec}]\n"));
            }
            section = this;
        } else {
            out.push('\n');
        }
        let default = match s.kind {
            Kind::Bool | Kind::Int => s.default.to_string(),
            _ => format!("{:?}", s.default),
        };
        out.push_str(&format!("# {}\n# {name} = {default}\n", s.doc));
    }
    out
}

/// Writes `text` to `path` through a temporary file in the same directory and a rename, creating
/// the directory if needed. On failure nothing is left behind.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map_or_else(
        || "config".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config")
            .join(name)
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| (*v).to_string())
        }
    }

    fn load_text(text: &str) -> (tempfile::TempDir, Loaded) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        std::fs::write(&p, text).unwrap();
        let l = load(&[p], &no_env, &Flags::default());
        (d, l)
    }

    #[test]
    fn the_table_is_consistent() {
        let mut keys: Vec<_> = SETTINGS.iter().map(|s| s.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 22, "twenty-two settings, no duplicates");
        for s in SETTINGS {
            let want = format!("KB_{}", s.key.to_uppercase().replace('.', "_"));
            assert_eq!(s.env, want, "{}", s.key);
            assert!(!s.doc.is_empty(), "{} needs a doc line", s.key);
        }
    }

    #[test]
    fn defaults_when_there_is_no_file() {
        let l = load(&[], &no_env, &Flags::default());
        assert!(l.warnings.is_empty() && l.file.is_none() && !l.legacy && l.ignored.is_empty());
        let s = &l.settings;
        assert_eq!(s.root(), None);
        assert_eq!(s.editor(), None);
        assert_eq!(s.theme(), ThemeChoice::Auto);
        assert_eq!(s.tui_enter(), EnterAction::Copy);
        assert!(!s.tui_mouse());
        assert_eq!(s.clipboard_method(), ClipboardChoice::Auto);
        assert_eq!(s.clipboard_command(), None);
        assert_eq!(s.osc52_max_bytes(), 74_000);
        assert!(s.trim_trailing_newline());
        assert_eq!(s.opener_command(), None);
        for (key, r) in s.iter() {
            assert_eq!(r.source, Source::Default, "{key}");
        }
        assert!(s.vars_remember());
        assert_eq!(s.sumo_base_url(), None);
        assert_eq!(s.aws_default_region(), None);
        assert_eq!(s.code_default_open(), "web");
        assert_eq!(s.code_web_url(), None);
        assert!(s.run_pause_after());
        assert_eq!(s.iter().count(), 22);
        assert_eq!(s.path_template(), PathTemplate::default());
    }

    #[test]
    fn a_missing_file_is_the_same_as_no_file() {
        let d = tempfile::tempdir().unwrap();
        let l = load(&[d.path().join("nope.toml")], &no_env, &Flags::default());
        assert!(l.warnings.is_empty() && l.file.is_none());
    }

    #[test]
    fn the_good_fixture_loads_every_value_from_the_file() {
        let p = fixture("good.toml");
        let l = load(std::slice::from_ref(&p), &no_env, &Flags::default());
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        assert_eq!(l.file.as_deref(), Some(p.as_path()));
        let s = &l.settings;
        assert_eq!(s.root(), Some("~/work/kb"));
        assert_eq!(s.editor(), Some("nvim"));
        assert_eq!(s.theme(), ThemeChoice::Light);
        let r = s.get("root").unwrap();
        assert_eq!(r.source, Source::File(p.clone()));
        assert_eq!(r.line, Some(4));
        assert_eq!(s.get("clipboard.method").unwrap().line, Some(15));
    }

    #[test]
    fn precedence_is_flag_then_env_then_file_then_default() {
        let p = fixture("good.toml");
        let files = std::slice::from_ref(&p);
        let l = load(files, &no_env, &Flags::default());
        assert_eq!(
            l.settings.get("theme").unwrap().source,
            Source::File(p.clone())
        );

        let pairs = [("KB_THEME", "dark"), ("KB_CLIPBOARD_METHOD", "native")];
        let env = env_of(&pairs);
        let l = load(files, &env, &Flags::default());
        assert_eq!(l.settings.theme(), ThemeChoice::Dark);
        assert_eq!(
            l.settings.get("theme").unwrap().source,
            Source::Env("KB_THEME")
        );
        assert_eq!(l.settings.clipboard_method(), ClipboardChoice::Native);

        let flags = Flags {
            clipboard_method: Some("osc52".into()),
            root: Some("/flag/kb".into()),
        };
        let l = load(files, &env, &flags);
        assert_eq!(l.settings.clipboard_method(), ClipboardChoice::Osc52);
        assert_eq!(
            l.settings.get("clipboard.method").unwrap().source,
            Source::Flag("--method")
        );
        assert_eq!(l.settings.root(), Some("/flag/kb"));
        assert_eq!(
            l.settings.get("root").unwrap().source,
            Source::Flag("--root")
        );
        // an untouched key keeps its file source
        assert_eq!(l.settings.get("editor").unwrap().source, Source::File(p));
    }

    #[test]
    fn an_empty_environment_variable_counts_as_unset() {
        let pairs = [("KB_THEME", ""), ("KB_ROOT", "")];
        let env = env_of(&pairs);
        let l = load(&[], &env, &Flags::default());
        assert_eq!(l.settings.get("theme").unwrap().source, Source::Default);
        assert_eq!(l.settings.root(), None);
        assert!(l.warnings.is_empty());
    }

    #[test]
    fn the_bad_fixture_gives_exactly_two_warnings_with_lines() {
        let p = fixture("bad.toml");
        let l = load(std::slice::from_ref(&p), &no_env, &Flags::default());
        assert_eq!(l.warnings.len(), 2, "{:#?}", l.warnings);
        let unknown = &l.warnings[0];
        assert_eq!(unknown.line, Some(3));
        assert!(unknown.message.contains("clipbaord.method"), "{unknown}");
        assert!(unknown.message.contains("unknown key"), "{unknown}");
        let bad = &l.warnings[1];
        assert_eq!(bad.line, Some(6));
        assert!(bad.message.contains("clipboard.method"), "{bad}");
        assert!(
            bad.message
                .contains("expected one of auto, native, osc52, stdout"),
            "{bad}"
        );
        assert!(bad.message.contains("using \"auto\""), "{bad}");
        assert!(
            bad.to_string().starts_with(&format!("{}:6: ", p.display())),
            "{bad}"
        );
        // the bad value falls back to the default, the good ones are kept
        assert_eq!(l.settings.clipboard_method(), ClipboardChoice::Auto);
        assert_eq!(l.settings.theme(), ThemeChoice::Light);
    }

    #[test]
    fn loading_twice_gives_the_same_warnings() {
        let p = fixture("bad.toml");
        let a = load(std::slice::from_ref(&p), &no_env, &Flags::default());
        let b = load(std::slice::from_ref(&p), &no_env, &Flags::default());
        assert_eq!(a.warnings, b.warnings);
        assert_eq!(a.warnings.len(), 2);
    }

    #[test]
    fn a_file_that_is_not_toml_gives_one_warning_and_all_defaults() {
        let p = fixture("syntax-error.toml");
        let l = load(std::slice::from_ref(&p), &no_env, &Flags::default());
        assert_eq!(l.warnings.len(), 1, "{:?}", l.warnings);
        assert!(
            l.warnings[0].message.contains("could not parse"),
            "{}",
            l.warnings[0]
        );
        assert!(
            l.warnings[0].message.contains("using defaults"),
            "{}",
            l.warnings[0]
        );
        assert_eq!(l.settings.theme(), ThemeChoice::Auto);
        assert!(l.settings.iter().all(|(_, r)| r.source == Source::Default));
    }

    #[test]
    fn booleans_in_the_file_must_be_toml_booleans() {
        let (_d, l) = load_text("[tui]\nmouse = \"true\"\n");
        assert_eq!(l.warnings.len(), 1, "{:?}", l.warnings);
        assert!(l.warnings[0].message.contains("tui.mouse"));
        assert!(l.warnings[0].message.contains("true or false"));
        assert!(!l.settings.tui_mouse());
        let (_d, l) = load_text("[tui]\nmouse = true\n");
        assert!(l.warnings.is_empty() && l.settings.tui_mouse());
    }

    #[test]
    fn environment_booleans_accept_the_usual_spellings() {
        for (text, want) in [
            ("1", true),
            ("yes", true),
            ("TRUE", true),
            ("0", false),
            ("no", false),
            ("false", false),
        ] {
            let pairs = [("KB_TUI_MOUSE", text)];
            let env = env_of(&pairs);
            let l = load(&[], &env, &Flags::default());
            assert!(l.warnings.is_empty(), "{text}: {:?}", l.warnings);
            assert_eq!(l.settings.tui_mouse(), want, "{text}");
        }
        let pairs = [("KB_TUI_MOUSE", "maybe")];
        let env = env_of(&pairs);
        let l = load(&[], &env, &Flags::default());
        assert_eq!(l.warnings.len(), 1);
        assert!(
            l.warnings[0].message.contains("KB_TUI_MOUSE"),
            "{}",
            l.warnings[0]
        );
        assert!(l.warnings[0].path.is_none());
        assert!(!l.settings.tui_mouse());
    }

    #[test]
    fn osc52_max_bytes_must_be_a_positive_integer() {
        for text in ["0", "-5", "\"big\"", "1.5"] {
            let (_d, l) = load_text(&format!("[clipboard]\nosc52_max_bytes = {text}\n"));
            assert_eq!(l.warnings.len(), 1, "{text}: {:?}", l.warnings);
            assert!(
                l.warnings[0].message.contains("at least 1"),
                "{text}: {}",
                l.warnings[0]
            );
            assert_eq!(l.settings.osc52_max_bytes(), 74_000);
        }
        let (_d, l) = load_text("[clipboard]\nosc52_max_bytes = 100\n");
        assert_eq!(l.settings.osc52_max_bytes(), 100);
    }

    #[test]
    fn tui_enter_and_theme_name_their_choices() {
        let (_d, l) = load_text("theme = \"blue\"\n[tui]\nenter = \"maybe\"\n");
        assert_eq!(l.warnings.len(), 2);
        assert!(
            l.warnings[0]
                .message
                .contains("expected one of auto, dark, light"),
            "{}",
            l.warnings[0]
        );
        assert!(
            l.warnings[1]
                .message
                .contains("expected one of copy, print"),
            "{}",
            l.warnings[1]
        );
        let (_d, l) = load_text("[tui]\nenter = \"print\"\n");
        assert_eq!(l.settings.tui_enter(), EnterAction::Print);
    }

    #[test]
    fn inline_tables_and_dotted_keys_are_understood() {
        let (_d, l) =
            load_text("clipboard = { method = \"osc52\", bogus = 1 }\ntui.mouse = true\n");
        assert_eq!(l.settings.clipboard_method(), ClipboardChoice::Osc52);
        assert!(l.settings.tui_mouse());
        assert_eq!(l.warnings.len(), 1, "{:?}", l.warnings);
        assert!(l.warnings[0].message.contains("clipboard.bogus"));
    }

    #[test]
    fn strings_and_paths_are_taken_as_written() {
        let (_d, l) = load_text(
            "root = \"\"\neditor = \"code -w\"\n[clipboard]\ncommand = \"xclip -selection clipboard\"\n[opener]\ncommand = \"firefox\"\n",
        );
        assert!(l.warnings.is_empty());
        assert_eq!(l.settings.root(), None, "an empty root means the default");
        assert_eq!(l.settings.editor(), Some("code -w"));
        assert_eq!(
            l.settings.clipboard_command(),
            Some("xclip -selection clipboard")
        );
        assert_eq!(l.settings.opener_command(), Some("firefox"));
    }

    #[test]
    fn a_wrong_type_is_a_warning_not_a_crash() {
        let (_d, l) = load_text("editor = 5\ntheme = [\"dark\"]\n");
        assert_eq!(l.warnings.len(), 2, "{:?}", l.warnings);
        assert_eq!(l.settings.editor(), None);
        assert_eq!(l.settings.theme(), ThemeChoice::Auto);
    }

    #[test]
    fn the_legacy_yaml_file_is_read_with_a_deprecation_warning() {
        let p = fixture("legacy/config.yml");
        let l = load(std::slice::from_ref(&p), &no_env, &Flags::default());
        assert!(l.legacy);
        assert_eq!(l.file.as_deref(), Some(p.as_path()));
        assert_eq!(l.settings.root(), Some("~/legacy-kb"));
        assert_eq!(l.settings.theme(), ThemeChoice::Dark);
        assert!(l.settings.tui_mouse(), "legacy `mouse` is `tui.mouse`");
        assert_eq!(l.warnings.len(), 1, "{:?}", l.warnings);
        assert!(
            l.warnings[0].message.contains("deprecated"),
            "{}",
            l.warnings[0]
        );
        assert!(
            l.warnings[0].message.contains("kb config init"),
            "{}",
            l.warnings[0]
        );
    }

    #[test]
    fn a_toml_file_hides_the_legacy_one() {
        let toml = fixture("good.toml");
        let yml = fixture("legacy/config.yml");
        let l = load(&[toml.clone(), yml], &no_env, &Flags::default());
        assert!(!l.legacy);
        assert_eq!(l.file.as_deref(), Some(toml.as_path()));
        assert_eq!(l.settings.root(), Some("~/work/kb"));
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    }

    #[test]
    fn a_second_existing_toml_is_reported_as_ignored() {
        let a = fixture("good.toml");
        let b = fixture("bad.toml");
        let l = load(&[a.clone(), b.clone()], &no_env, &Flags::default());
        assert_eq!(l.file.as_deref(), Some(a.as_path()));
        assert_eq!(l.ignored, vec![b]);
        assert!(l.warnings.is_empty(), "the ignored file is not even parsed");
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    fn good() -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config/good.toml"),
        )
        .unwrap()
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn load_text(text: &str) -> Loaded {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        std::fs::write(&p, text).unwrap();
        load(&[p], &no_env, &Flags::default())
    }

    /// `original` with line `n` (1-based) replaced by `with`.
    fn replace_line(original: &str, n: usize, with: &str) -> String {
        let mut lines: Vec<&str> = original.split_inclusive('\n').collect();
        let ending = if lines[n - 1].ends_with('\n') {
            "\n"
        } else {
            ""
        };
        let new = format!("{with}{ending}");
        lines[n - 1] = &new;
        lines.concat()
    }

    #[test]
    fn setting_a_value_changes_only_that_value() {
        let original = good();
        // line 6 is `theme = "light"`, line 11 is `enter = "copy"`, line 12 `mouse = false`
        assert_eq!(
            set_value(&original, "theme", "dark").unwrap(),
            replace_line(&original, 6, "theme = \"dark\"")
        );
        assert_eq!(
            set_value(&original, "tui.enter", "print").unwrap(),
            replace_line(&original, 11, "enter = \"print\"")
        );
        assert_eq!(
            set_value(&original, "tui.mouse", "true").unwrap(),
            replace_line(&original, 12, "mouse = true")
        );
        assert_eq!(
            set_value(&original, "clipboard.osc52_max_bytes", "1000").unwrap(),
            replace_line(&original, 17, "osc52_max_bytes = 1000")
        );
    }

    #[test]
    fn a_trailing_comment_and_odd_spacing_on_the_edited_line_survive() {
        let original = good();
        let edited = set_value(&original, "root", "~/other").unwrap();
        let line = edited.lines().nth(3).unwrap();
        assert!(
            line.starts_with("root") && line.contains("\"~/other\""),
            "{line}"
        );
        assert!(line.ends_with("# trailing comment on root"), "{line}");
        // everything else is byte for byte the same
        let mut want: Vec<&str> = original.lines().collect();
        let mut got: Vec<&str> = edited.lines().collect();
        want.remove(3);
        got.remove(3);
        assert_eq!(want, got);
        assert_eq!(edited.ends_with('\n'), original.ends_with('\n'));
    }

    #[test]
    fn the_edited_file_loads_with_the_new_value_and_no_warnings() {
        let edited = set_value(&good(), "clipboard.method", "osc52").unwrap();
        let l = load_text(&edited);
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        assert_eq!(l.settings.clipboard_method(), ClipboardChoice::Osc52);
        assert_eq!(
            l.settings.theme(),
            ThemeChoice::Light,
            "the rest is untouched"
        );
    }

    #[test]
    fn a_missing_key_is_added_to_its_table_and_a_missing_table_is_created() {
        let d = set_value("theme = \"dark\"\n", "tui.mouse", "true").unwrap();
        assert!(d.starts_with("theme = \"dark\"\n"), "{d}");
        assert!(d.contains("[tui]") && d.contains("mouse = true"), "{d}");
        assert!(load_text(&d).settings.tui_mouse());
        // into an existing table, next to its other keys
        let t = "[clipboard]\nmethod = \"native\"\n";
        let d = set_value(t, "clipboard.command", "xclip -in").unwrap();
        assert_eq!(
            d,
            "[clipboard]\nmethod = \"native\"\ncommand = \"xclip -in\"\n"
        );
        // a top-level key into an empty file
        let d = set_value("", "editor", "nvim").unwrap();
        assert_eq!(d, "editor = \"nvim\"\n");
    }

    #[test]
    fn init_then_set_puts_the_key_right_under_its_table_and_keeps_the_comments() {
        let template = render_template();
        let edited = set_value(&template, "clipboard.method", "osc52").unwrap();
        let lines: Vec<&str> = edited.lines().collect();
        let header = lines.iter().position(|l| *l == "[clipboard]").unwrap();
        assert_eq!(lines[header + 1], "method = \"osc52\"", "{edited}");
        // the commented defaults are all still there
        assert!(edited.contains("# method = \"auto\""), "{edited}");
        assert!(edited.contains("# command = \"\""), "{edited}");
        assert_eq!(edited.lines().count(), template.lines().count() + 1);
        let l = load_text(&edited);
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        assert_eq!(l.settings.clipboard_method(), ClipboardChoice::Osc52);
        // and a top-level key goes before the first table
        let edited = set_value(&template, "theme", "dark").unwrap();
        let theme_at = edited
            .lines()
            .position(|l| l == "theme = \"dark\"")
            .unwrap();
        let first_table = edited.lines().position(|l| l.starts_with('[')).unwrap();
        assert!(theme_at < first_table, "{edited}");
    }

    #[test]
    fn values_are_checked_against_the_setting_before_anything_is_written() {
        let original = good();
        for (key, value, wanted) in [
            ("theme", "blue", "expected one of auto, dark, light"),
            ("tui.enter", "maybe", "expected one of copy, print, primary"),
            ("tui.mouse", "perhaps", "expected true or false"),
            ("clipboard.osc52_max_bytes", "0", "at least 1"),
            ("clipboard.osc52_max_bytes", "lots", "at least 1"),
            (
                "clipboard.method",
                "xclipp",
                "expected one of auto, native, osc52, stdout",
            ),
        ] {
            match set_value(&original, key, value) {
                Err(ConfigEditError::Invalid { key: k, message }) => {
                    assert_eq!(k, key);
                    assert!(message.contains(wanted), "{key}={value}: {message}");
                }
                other => panic!("{key}={value}: {other:?}"),
            }
        }
        assert_eq!(
            set_value(&original, "colour", "red"),
            Err(ConfigEditError::UnknownKey("colour".into()))
        );
    }

    #[test]
    fn booleans_and_integers_are_written_as_toml_booleans_and_integers() {
        let d = set_value("", "tui.mouse", "yes").unwrap();
        assert!(d.contains("mouse = true"), "{d}");
        let d = set_value("", "clipboard.osc52_max_bytes", "2048").unwrap();
        assert!(
            d.contains("osc52_max_bytes = 2048") && !d.contains('"'),
            "{d}"
        );
        // strings with quotes or backslashes are escaped
        let d = set_value("", "clipboard.command", "sh -c \"cat > f\"").unwrap();
        assert_eq!(
            load_text(&d).settings.clipboard_command(),
            Some("sh -c \"cat > f\"")
        );
    }

    #[test]
    fn a_file_that_is_not_toml_is_not_edited() {
        match set_value("theme = [\n", "theme", "dark") {
            Err(ConfigEditError::NotToml(m)) => assert!(!m.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn edit_errors_read_well() {
        assert_eq!(
            ConfigEditError::UnknownKey("x".into()).to_string(),
            "unknown setting \"x\" (see `kb config list`)"
        );
        let e = set_value("", "theme", "blue").unwrap_err();
        assert_eq!(
            e.to_string(),
            "theme: \"blue\" is not valid (expected one of auto, dark, light)"
        );
    }

    #[test]
    fn the_template_lists_every_setting_once_commented_out() {
        let t = render_template();
        // collect `section.name` for every commented `# name = value` line
        let mut section = String::new();
        let mut found: Vec<String> = Vec::new();
        for line in t.lines() {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = format!("{name}.");
            } else if let Some(rest) = line.strip_prefix("# ")
                && let Some((name, _)) = rest.split_once(" = ")
                && !name.contains(' ')
            {
                found.push(format!("{section}{name}"));
            }
        }
        let mut want: Vec<String> = SETTINGS.iter().map(|s| s.key.to_string()).collect();
        found.sort();
        want.sort();
        assert_eq!(found, want, "{t}");
        for s in SETTINGS {
            assert!(t.contains(&format!("# {}", s.doc)), "doc for {}", s.key);
        }
        // with nothing uncommented, it changes nothing
        let l = load_text(&t);
        assert!(
            l.warnings.is_empty() && l.settings.iter().all(|(_, r)| r.source == Source::Default)
        );
    }

    #[test]
    fn uncommenting_the_template_gives_the_defaults_without_warnings() {
        let uncommented: String = render_template()
            .lines()
            .map(|l| match l.strip_prefix("# ") {
                Some(rest) if rest.contains(" = ") => format!("{rest}\n"),
                _ => format!("{l}\n"),
            })
            .collect();
        let l = load_text(&uncommented);
        assert!(l.warnings.is_empty(), "{:?}\n{uncommented}", l.warnings);
        for (key, r) in l.settings.iter() {
            let setting = SETTINGS.iter().find(|s| s.key == key).unwrap();
            assert_eq!(r.value, setting.default, "{key}");
            assert!(matches!(r.source, Source::File(_)), "{key}");
        }
    }

    #[test]
    fn write_atomic_creates_directories_and_replaces_the_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a/b/config.toml");
        write_atomic(&p, "one = 1\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "one = 1\n");
        write_atomic(&p, "two = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two = 2\n");
        let names: Vec<_> = std::fs::read_dir(p.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["config.toml"], "no temporary file is left");
    }

    #[test]
    fn write_atomic_failure_leaves_nothing_behind() {
        let d = tempfile::tempdir().unwrap();
        // the target is a directory: the rename fails after the temporary file was written
        let target = d.path().join("config.toml");
        std::fs::create_dir(&target).unwrap();
        assert!(write_atomic(&target, "x = 1\n").is_err());
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["config.toml"], "{names:?}");
        // the parent is a file: the directory cannot be created
        let file = d.path().join("plain");
        std::fs::write(&file, "x").unwrap();
        assert!(write_atomic(&file.join("sub/config.toml"), "x = 1\n").is_err());
    }

    #[test]
    fn path_template_default_and_placeholders() {
        use crate::entry::EntryType;
        let tags = vec!["ecs".to_string(), "api".to_string()];
        let default = PathTemplate::parse("").unwrap();
        assert_eq!(default, PathTemplate::default());
        assert_eq!(default.render(EntryType::Cw, "p99", &tags), "cw/p99");
        let by_tag = PathTemplate::parse("{tag}/{slug}").unwrap();
        assert_eq!(by_tag.render(EntryType::Cw, "p99", &tags), "ecs/p99");
        assert_eq!(by_tag.render(EntryType::Cw, "p99", &[]), "cw/p99");
        let both = PathTemplate::parse("team/{type}-{slug}").unwrap();
        assert_eq!(both.render(EntryType::Sql, "x", &[]), "team/sql-x");
    }

    #[test]
    fn path_template_refuses_what_cannot_work() {
        for bad in ["{type}/x", "{slug", "slug}", "{nope}/{slug}", "{type}"] {
            assert!(PathTemplate::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn path_template_setting_loads_and_a_bad_value_warns() {
        let l = load_text("path_template = \"{tag}/{slug}\"\n");
        assert!(l.warnings.is_empty());
        assert_eq!(
            l.settings.path_template(),
            PathTemplate::parse("{tag}/{slug}").unwrap()
        );
        let l = load_text("path_template = \"{type}/nope\"\n");
        assert_eq!(l.warnings.len(), 1, "{:?}", l.warnings);
        assert!(l.warnings[0].message.contains("{slug}"));
        assert_eq!(l.settings.path_template(), PathTemplate::default());
        assert!(setting("path_template").is_some());
    }

    #[test]
    fn actions_and_code_links_config_additions() {
        let toml = r#"
[tui]
enter = "primary"

[sumo]
base_url = "https://service.us2.sumologic.com"

[aws]
default_region = "us-east-1"

[code]
default_open = "vscode"
search_paths = ["~/repos", "~/code"]
web_url = "https://source.corp.internal/{repo}/blob/{ref}/{path}#L{line}"

[code.repos]
"auth0/auth0-java" = "~/repos/auth0-java"
"cortiz/kb" = "~/repos/jmpeax/kb"

[run]
pause_after = false
destructive_patterns = [
    "ansible-playbook.*--extra-vars.*prod",
    "prisma migrate reset",
]

[[actions.sql]]
name = "psql"
key = "p"
command = "psql -d {{database}} -h {{host}}"
input = "stdin"

[[actions.cmd]]
name = "dry-run"
key = "d"
command = "echo [DRY RUN]: {{payload}}"
input = "arg"
"#;
        let l = load_text(toml);
        assert!(l.warnings.is_empty(), "warnings: {:?}", l.warnings);
        assert_eq!(l.settings.tui_enter(), EnterAction::Primary);
        assert_eq!(
            l.settings.sumo_base_url(),
            Some("https://service.us2.sumologic.com")
        );
        assert_eq!(l.settings.aws_default_region(), Some("us-east-1"));
        assert_eq!(l.settings.code_default_open(), "vscode");
        assert_eq!(l.settings.code_search_paths(), &["~/repos", "~/code"]);
        assert_eq!(
            l.settings.code_web_url(),
            Some("https://source.corp.internal/{repo}/blob/{ref}/{path}#L{line}")
        );
        assert_eq!(
            l.settings
                .code_repos()
                .get("auth0/auth0-java")
                .map(|s| s.as_str()),
            Some("~/repos/auth0-java")
        );
        assert_eq!(
            l.settings.code_repos().get("cortiz/kb").map(|s| s.as_str()),
            Some("~/repos/jmpeax/kb")
        );
        assert!(!l.settings.run_pause_after());
        assert_eq!(
            l.settings.run_destructive_patterns(),
            &[
                "ansible-playbook.*--extra-vars.*prod",
                "prisma migrate reset"
            ]
        );
        assert_eq!(l.settings.custom_actions().len(), 2);
        let sql_act = &l.settings.custom_actions()[0];
        assert_eq!(sql_act.entry_type, "sql");
        assert_eq!(sql_act.name, "psql");
        assert_eq!(sql_act.key, 'p');
        assert_eq!(sql_act.command, "psql -d {{database}} -h {{host}}");
        assert_eq!(sql_act.input, CustomActionInput::Stdin);

        let cmd_act = &l.settings.custom_actions()[1];
        assert_eq!(cmd_act.entry_type, "cmd");
        assert_eq!(cmd_act.name, "dry-run");
        assert_eq!(cmd_act.key, 'd');
        assert_eq!(cmd_act.command, "echo [DRY RUN]: {{payload}}");
        assert_eq!(cmd_act.input, CustomActionInput::Arg);
    }

    #[test]
    fn git_and_sync_config_defaults_and_parsing() {
        let defaults = Settings::defaults();
        assert!(defaults.git_auto_commit());
        assert_eq!(defaults.git_commit_template(), "kb: {action} {id}");
        assert_eq!(defaults.sync_strategy(), SyncStrategy::Rebase);
        assert_eq!(defaults.sync_timeout(), 30);
        assert!(!defaults.sync_fetch_on_start());

        let toml = r#"
[git]
auto_commit = false
commit_template = "custom: {action} -> {id}"

[sync]
strategy = "merge"
timeout = 45
fetch_on_start = true
"#;
        let l = load_text(toml);
        assert!(l.warnings.is_empty(), "warnings: {:?}", l.warnings);
        assert!(!l.settings.git_auto_commit());
        assert_eq!(l.settings.git_commit_template(), "custom: {action} -> {id}");
        assert_eq!(l.settings.sync_strategy(), SyncStrategy::Merge);
        assert_eq!(l.settings.sync_timeout(), 45);
        assert!(l.settings.sync_fetch_on_start());

        // Test invalid strategy warns and falls back to default
        let bad = load_text("[sync]\nstrategy = \"invalid\"\n");
        assert_eq!(bad.warnings.len(), 1);
        assert_eq!(bad.settings.sync_strategy(), SyncStrategy::Rebase);
    }
}
