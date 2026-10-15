//! The draft of a new entry.

/// Fields that depend on the entry type, as typed values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DraftFields {
    #[default]
    None,
    Sql {
        dialect: Option<String>,
        database: Option<String>,
    },
    Sumo {
        range: Option<String>,
    },
    Cw {
        log_groups: Vec<String>,
        range: Option<String>,
        region: Option<String>,
    },
    Cmd {
        shell: Option<String>,
        cwd: Option<String>,
    },
    Link {
        url: String,
    },
    Code {
        repo: String,
        git_ref: String,
        path: String,
        line: u32,
        end_line: Option<u32>,
        symbol: Option<String>,
    },
}

use crate::author::detect::{Confidence, Detection, detect, normalize_clipboard};
use crate::author::suggest::{VariableSuggestion, suggest_vars};
use crate::config::PathTemplate;
use crate::entry::{EntryType, MAX_TITLE_CHARS};
use crate::vars::VarKind;

/// Where the body came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftSource {
    Clipboard,
    Stdin,
    Empty,
    Editor,
}

/// Where the new entry goes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PathChoice {
    /// Derived from the template, the title and the tags; follows them when they change.
    #[default]
    Auto,
    /// Typed by the user or given with `--path`.
    Fixed(String),
}

/// A variable declared in the saved entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDecl {
    pub name: String,
    pub kind: VarKind,
    pub default: Option<String>,
    pub options: Vec<String>,
}

/// Why a draft setting or the target path was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {
    /// The title gives no usable path; the user has to type one.
    NeedsPath,
    /// The path is not a valid entry ID.
    BadId(String),
    /// An entry with this ID exists (any case).
    Taken(String),
    /// A `secret` variable can't have a default.
    SecretDefault,
    /// An `enum` default has to be one of the options.
    DefaultNotInOptions,
    /// No variable has this name.
    UnknownVar(String),
}

impl std::fmt::Display for DraftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DraftError::NeedsPath => {
                write!(f, "a path is required (the title has no usable characters)")
            }
            DraftError::BadId(id) => write!(f, "\"{id}\" is not a valid entry path"),
            DraftError::Taken(id) => write!(f, "{id} already exists"),
            DraftError::SecretDefault => write!(f, "a secret variable can't have a default"),
            DraftError::DefaultNotInOptions => write!(f, "the default must be one of the options"),
            DraftError::UnknownVar(name) => write!(f, "there is no variable \"{name}\""),
        }
    }
}

impl std::error::Error for DraftError {}

/// What the caller already knows about the entry (flags, config).
#[derive(Debug, Clone, Default)]
pub struct DraftOptions {
    pub title: Option<String>,
    /// `--type` wins over detection.
    pub ty: Option<EntryType>,
    pub tags: Vec<String>,
    /// `--path`
    pub path: Option<String>,
    pub path_template: PathTemplate,
}

/// An entry being created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub source: DraftSource,
    pub entry_type: EntryType,
    pub detection: Detection,
    pub title: String,
    pub tags: Vec<String>,
    pub fields: DraftFields,
    /// The payload without its fence; empty for `link` and `code`.
    pub body: String,
    pub notes: String,
    pub vars: Vec<VarDecl>,
    pub suggestions: Vec<VariableSuggestion>,
    pub path: PathChoice,
    pub path_template: PathTemplate,
    /// The type was chosen by the user (flag, chip), so redetection doesn't change it lightly.
    pub type_chosen: bool,
}

/// A title on one line, as the format wants it.
pub fn clean_title(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One tag as it is stored: trimmed, no leading `#`, lowercase, spaces as `-`. `None` when empty.
pub fn clean_tag(s: &str) -> Option<String> {
    let tag = s.trim().trim_start_matches('#').trim().to_lowercase();
    let tag = tag.split_whitespace().collect::<Vec<_>>().join("-");
    (!tag.is_empty()).then_some(tag)
}

/// `Authorizer p99 latency` becomes `authorizer-p99-latency`: ASCII, lowercase, at most 60 characters.
pub fn slugify(title: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    use unicode_normalization::char::is_combining_mark;
    let mut slug = String::new();
    for c in title.nfkd() {
        if is_combining_mark(c) {
            continue;
        }
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut slug = slug.trim_end_matches('-').to_string();
    if slug.len() > MAX_SLUG {
        let cut = if slug.as_bytes()[MAX_SLUG] == b'-' {
            MAX_SLUG
        } else {
            slug[..MAX_SLUG]
                .rfind('-')
                .filter(|i| *i > 0)
                .unwrap_or(MAX_SLUG)
        };
        slug.truncate(cut);
    }
    slug.trim_end_matches('-').to_string()
}

/// Longest slug, in characters (FR-008).
const MAX_SLUG: usize = 60;

/// The title suggested by the content (FR-007); empty when nothing applies.
pub fn suggest_title(ty: EntryType, body: &str, fields: &DraftFields) -> String {
    match fields {
        DraftFields::Link { url } if !url.is_empty() => return link_title(url),
        DraftFields::Code {
            symbol: Some(symbol),
            ..
        } if !symbol.is_empty() => return symbol.clone(),
        DraftFields::Code { path, line, .. } if !path.is_empty() => {
            let file = path.rsplit('/').next().unwrap_or(path);
            return format!("{file}:{line}");
        }
        _ => {}
    }
    if !ty.has_payload() {
        return String::new();
    }
    for line in body.lines().take(3) {
        let line = line.trim_start();
        let text = ["-- ", "# ", "// "]
            .iter()
            .find_map(|marker| line.strip_prefix(marker))
            .map(str::trim);
        if let Some(text) = text {
            let chars = text.chars().count();
            return if (3..=MAX_TITLE_CHARS).contains(&chars) {
                clean_title(text)
            } else {
                String::new()
            };
        }
    }
    String::new()
}

/// `https://github.com/kb-cli/kb` becomes `github.com kb`.
fn link_title(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.split(['?', '#']).next().unwrap_or("");
    match path.split('/').rfind(|s| !s.is_empty()) {
        Some(last) => format!("{host} {last}"),
        None => host.split(['?', '#']).next().unwrap_or(host).to_string(),
    }
}

/// The kind a new variable gets from its name (FR-013).
pub fn default_kind(name: &str) -> VarKind {
    const NUMBERS: [&str; 5] = ["minutes", "limit", "days", "count", "port"];
    const SECRETS: [&str; 5] = ["token", "password", "secret", "key", "api_key"];
    const SECRET_SUFFIXES: [&str; 4] = ["_token", "_password", "_secret", "_key"];
    if NUMBERS.contains(&name) {
        VarKind::Number
    } else if SECRETS.contains(&name) || SECRET_SUFFIXES.iter().any(|s| name.ends_with(s)) {
        VarKind::Secret
    } else {
        VarKind::Text
    }
}

/// Empty type fields for `ty`.
fn default_fields(ty: EntryType) -> DraftFields {
    match ty {
        EntryType::Sql => DraftFields::Sql {
            dialect: None,
            database: None,
        },
        EntryType::Sumo => DraftFields::Sumo { range: None },
        EntryType::Cw => DraftFields::Cw {
            log_groups: Vec::new(),
            range: None,
            region: None,
        },
        EntryType::Cmd => DraftFields::Cmd {
            shell: None,
            cwd: None,
        },
        EntryType::Prompt | EntryType::Note => DraftFields::None,
        EntryType::Link => DraftFields::Link { url: String::new() },
        EntryType::Code => DraftFields::Code {
            repo: String::new(),
            git_ref: String::new(),
            path: String::new(),
            line: 1,
            end_line: None,
            symbol: None,
        },
    }
}

/// The fields for `ty`, taking what `detection` found when it fits.
fn fields_for(ty: EntryType, detection: &Detection, text: &str) -> DraftFields {
    match (ty, &detection.fields) {
        (EntryType::Link, f @ DraftFields::Link { .. })
        | (EntryType::Code, f @ DraftFields::Code { .. }) => f.clone(),
        (EntryType::Link, _) => {
            let line = text.trim();
            let single = !line.contains(char::is_whitespace) && line.starts_with("http");
            DraftFields::Link {
                url: if single {
                    line.to_string()
                } else {
                    String::new()
                },
            }
        }
        _ => default_fields(ty),
    }
}

impl Draft {
    pub fn from_text(text: &str, source: DraftSource, opts: &DraftOptions) -> Draft {
        let mut detection = detect(text);
        let normalized = normalize_clipboard(text);
        let (ty, type_chosen) = match opts.ty {
            Some(ty) => (ty, true),
            None => (detection.ty, false),
        };
        let fields = fields_for(ty, &detection, &normalized);
        let note_text = (ty == EntryType::Note).then(|| normalized.clone());
        let body = if !ty.has_payload() {
            String::new()
        } else if detection.body.is_empty() {
            normalized
        } else {
            detection.body.clone()
        };
        if type_chosen {
            detection.ty = ty;
            detection.confidence = Confidence::High;
        }
        let mut draft = Draft::new(source, ty, detection, opts);
        draft.fields = fields;
        draft.body = body;
        if let Some(text) = note_text {
            draft.notes = text;
        }
        draft.type_chosen = type_chosen;
        if draft.title.is_empty() {
            draft.title = suggest_title(ty, &draft.body, &draft.fields);
        }
        if draft.title.is_empty() && ty == EntryType::Note {
            draft.title = crate::entry::note_title_from_body(&draft.notes).unwrap_or_default();
        }
        draft.refresh();
        draft
    }

    pub fn empty(source: DraftSource, ty: Option<EntryType>, opts: &DraftOptions) -> Draft {
        let mut detection = detect("");
        if let Some(ty) = ty {
            detection.ty = ty;
            detection.confidence = Confidence::High;
        }
        let ty = detection.ty;
        let mut draft = Draft::new(source, ty, detection, opts);
        draft.fields = default_fields(ty);
        draft.type_chosen = opts.ty.is_some();
        draft
    }

    fn new(source: DraftSource, ty: EntryType, detection: Detection, opts: &DraftOptions) -> Draft {
        let mut tags: Vec<String> = Vec::new();
        for tag in opts.tags.iter().filter_map(|t| clean_tag(t)) {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        Draft {
            source,
            entry_type: ty,
            detection,
            title: opts.title.as_deref().map(clean_title).unwrap_or_default(),
            tags,
            fields: DraftFields::None,
            body: String::new(),
            notes: String::new(),
            vars: Vec::new(),
            suggestions: Vec::new(),
            path: match opts
                .path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                Some(p) => PathChoice::Fixed(p.strip_suffix(".md").unwrap_or(p).to_string()),
                None => PathChoice::Auto,
            },
            path_template: opts.path_template.clone(),
            type_chosen: false,
        }
    }

    /// Rebuilds the declarations and suggestions from the body: every `{{name}}` is declared once
    /// in order of first appearance; decisions are kept by name, declarations whose variable is gone
    /// are dropped.
    fn refresh(&mut self) {
        let mut seen: Vec<String> = Vec::new();
        for brace in crate::vars::scan_refs(&self.body) {
            if let crate::vars::Brace::Ref { name, .. } = brace
                && !seen.contains(&name)
            {
                seen.push(name);
            }
        }
        let mut old = std::mem::take(&mut self.vars);
        self.vars = seen
            .into_iter()
            .map(|name| match old.iter().position(|v| v.name == name) {
                Some(i) => old.swap_remove(i),
                None => VarDecl {
                    kind: default_kind(&name),
                    default: None,
                    options: Vec::new(),
                    name,
                },
            })
            .collect();
        self.suggestions = if self.entry_type.has_payload() {
            suggest_vars(&self.body, &self.vars)
        } else {
            Vec::new()
        };
    }

    /// Chooses the type (the user did): fields that don't apply to it are dropped; title, tags,
    /// body, notes and variables stay.
    pub fn set_type(&mut self, ty: EntryType) {
        self.type_chosen = true;
        if ty == self.entry_type {
            return;
        }
        self.entry_type = ty;
        self.fields = if std::mem::discriminant(&self.fields)
            == std::mem::discriminant(&default_fields(ty))
        {
            std::mem::take(&mut self.fields)
        } else {
            fields_for(ty, &self.detection, &self.body)
        };
        self.refresh();
    }

    /// Replaces the body. With `redetect`, detection runs again and the type may change (only if
    /// the user didn't choose it, or the new detection is `High` and different); returns whether it did.
    pub fn set_body(&mut self, body: String, redetect: bool) -> bool {
        let text = normalize_clipboard(&body);
        let mut type_changed = false;
        if redetect {
            let found = detect(&text);
            type_changed = if self.type_chosen {
                found.confidence == Confidence::High && found.ty != self.entry_type
            } else {
                found.ty != self.entry_type
            };
            if type_changed {
                self.entry_type = found.ty;
                self.fields = fields_for(found.ty, &found, &text);
            }
            self.body = if self.entry_type == found.ty {
                found.body.clone()
            } else {
                text
            };
            self.detection = found;
        } else {
            self.body = text;
        }
        self.refresh();
        type_changed
    }

    fn var_mut(&mut self, name: &str) -> Result<&mut VarDecl, DraftError> {
        self.vars
            .iter_mut()
            .find(|v| v.name == name)
            .ok_or_else(|| DraftError::UnknownVar(name.to_string()))
    }

    /// Changing the kind drops what the new kind can't have: a secret's default, an enum's options.
    pub fn set_var_kind(&mut self, name: &str, kind: VarKind) -> Result<(), DraftError> {
        let var = self.var_mut(name)?;
        var.kind = kind;
        if kind == VarKind::Secret {
            var.default = None;
        }
        if kind != VarKind::Enum {
            var.options.clear();
        }
        Ok(())
    }

    pub fn set_var_default(
        &mut self,
        name: &str,
        default: Option<String>,
    ) -> Result<(), DraftError> {
        let var = self.var_mut(name)?;
        if let Some(d) = &default {
            match var.kind {
                VarKind::Secret => return Err(DraftError::SecretDefault),
                VarKind::Enum if !var.options.contains(d) => {
                    return Err(DraftError::DefaultNotInOptions);
                }
                _ => {}
            }
        }
        var.default = default;
        Ok(())
    }

    /// Sets the options; a default that is no longer one of them is dropped.
    pub fn set_var_options(&mut self, name: &str, options: Vec<String>) -> Result<(), DraftError> {
        let var = self.var_mut(name)?;
        if var.default.as_ref().is_some_and(|d| !options.contains(d)) {
            var.default = None;
        }
        var.options = options;
        Ok(())
    }

    /// Writes `{{name}}` as `\{{name}}` everywhere in the body and drops the declaration.
    pub fn mark_literal(&mut self, name: &str) {
        let spans: Vec<_> = crate::vars::scan_refs(&self.body)
            .into_iter()
            .filter_map(|b| match b {
                crate::vars::Brace::Ref { name: n, span } if n == name => Some(span),
                _ => None,
            })
            .collect();
        for span in spans.into_iter().rev() {
            self.body.insert(span.start, '\\');
        }
        self.refresh();
    }

    /// Accepts suggestion `index`: every occurrence of its value becomes a reference, and the
    /// variable is declared with the original value as its default, so the entry still resolves to
    /// the pasted text.
    pub fn accept(&mut self, index: usize) {
        let Some(s) = self.suggestions.get(index).cloned() else {
            return;
        };
        for range in s.positions.iter().rev() {
            self.body
                .replace_range(range.clone(), &format!("{{{{{}}}}}", s.name));
        }
        self.refresh();
        if let Some(var) = self.vars.iter_mut().find(|v| v.name == s.name) {
            var.kind = s.kind;
            var.options = s.options;
            var.default = Some(s.default);
        }
    }

    /// The entry ID. `taken` says whether an ID exists already (any case).
    pub fn target_id(&self, taken: &dyn Fn(&str) -> bool) -> Result<String, DraftError> {
        let usable =
            |id: &str| crate::search::valid_id(id) && !id.split('/').any(|c| c.starts_with('.'));
        match &self.path {
            PathChoice::Fixed(path) => {
                let id = path.strip_suffix(".md").unwrap_or(path);
                if !usable(id) {
                    Err(DraftError::BadId(path.clone()))
                } else if taken(id) {
                    Err(DraftError::Taken(id.to_string()))
                } else {
                    Ok(id.to_string())
                }
            }
            PathChoice::Auto => {
                let slug = slugify(&self.title);
                if slug.is_empty() {
                    return Err(DraftError::NeedsPath);
                }
                let base = self
                    .path_template
                    .render(self.entry_type, &slug, &self.tags);
                if !usable(&base) {
                    return Err(DraftError::BadId(base));
                }
                (1..=999)
                    .map(|n| {
                        if n == 1 {
                            base.clone()
                        } else {
                            format!("{base}-{n}")
                        }
                    })
                    .find(|id| !taken(id))
                    .ok_or(DraftError::Taken(base))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(text: &str) -> Draft {
        Draft::from_text(text, DraftSource::Stdin, &DraftOptions::default())
    }

    fn with_title(text: &str, title: &str) -> Draft {
        let opts = DraftOptions {
            title: Some(title.into()),
            ..Default::default()
        };
        Draft::from_text(text, DraftSource::Stdin, &opts)
    }

    fn none(_: &str) -> bool {
        false
    }

    // ---- titles and slugs ----

    #[test]
    fn title_comes_from_the_flag_then_the_first_comment() {
        assert_eq!(
            with_title("-- from comment\nselect 1 from t;", "Flag").title,
            "Flag"
        );
        assert_eq!(
            draft("-- active connections\nselect 1 from t;").title,
            "active connections"
        );
        assert_eq!(
            draft("# restart the api\nkubectl rollout restart deploy/api").title,
            "restart the api"
        );
        assert_eq!(
            draft("// count errors\n_sourceCategory=a | count by x").title,
            "count errors"
        );
    }

    #[test]
    fn only_a_comment_in_the_first_three_lines_counts() {
        assert_eq!(
            draft("select 1 from t\nwhere a\nand b\n-- late comment").title,
            ""
        );
        assert_eq!(
            draft("select 1 from t\nwhere a\n-- third line\n").title,
            "third line"
        );
    }

    #[test]
    fn a_shebang_is_not_a_title() {
        assert_eq!(draft("#!/bin/bash\nkubectl get pods -n prod").title, "");
        assert_eq!(
            draft("#!/bin/bash\n# list pods\nkubectl get pods -n prod").title,
            "list pods"
        );
    }

    #[test]
    fn comments_that_are_too_short_or_long_are_not_titles() {
        assert_eq!(draft("-- hi\nselect 1 from t;").title, "");
        let long = "x".repeat(121);
        assert_eq!(draft(&format!("-- {long}\nselect 1 from t;")).title, "");
    }

    #[test]
    fn link_and_code_titles() {
        assert_eq!(draft("https://github.com/kb-cli/kb").title, "github.com kb");
        assert_eq!(
            draft("https://example.com/runbooks/auth?x=1#top").title,
            "example.com auth"
        );
        assert_eq!(draft("https://example.com/").title, "example.com");
        assert_eq!(
            draft("https://github.com/acme/api/blob/main/src/server.rs#L10-L20").title,
            "server.rs:10"
        );
    }

    #[test]
    fn slugs_are_lowercase_ascii() {
        assert_eq!(slugify("Authorizer p99 latency"), "authorizer-p99-latency");
        assert_eq!(slugify("  Café — déjà vu!  "), "cafe-deja-vu");
        assert_eq!(slugify("¿¡!?"), "");
        assert_eq!(slugify("a__b..c"), "a-b-c");
    }

    #[test]
    fn slugs_are_cut_at_60_on_a_word_boundary() {
        let title = "one two three four five six seven eight nine ten eleven twelve thirteen";
        let s = slugify(title);
        assert!(s.len() <= 60, "{s}");
        assert!(title.replace(' ', "-").starts_with(&s));
        assert!(!s.ends_with('-'));
        assert_eq!(s, "one-two-three-four-five-six-seven-eight-nine-ten-eleven");
        let one = "x".repeat(80);
        assert_eq!(slugify(&one).len(), 60);
    }

    #[test]
    fn target_id_uses_the_template_and_resolves_clashes() {
        let d = with_title("select 1 from t;", "Active connections");
        assert_eq!(d.target_id(&none).unwrap(), "sql/active-connections");
        let taken = |id: &str| id == "sql/active-connections" || id == "sql/active-connections-2";
        assert_eq!(d.target_id(&taken).unwrap(), "sql/active-connections-3");
        // case-insensitive clash is the callback's business: any answer is respected
        let upper = |id: &str| id.eq_ignore_ascii_case("SQL/Active-Connections");
        assert_eq!(d.target_id(&upper).unwrap(), "sql/active-connections-2");
    }

    #[test]
    fn target_id_follows_the_path_template() {
        let opts = DraftOptions {
            title: Some("P99".into()),
            tags: vec!["ECS".into(), "api".into()],
            path_template: PathTemplate::parse("{tag}/{slug}").unwrap(),
            ..Default::default()
        };
        let d = Draft::from_text("select 1 from t;", DraftSource::Stdin, &opts);
        assert_eq!(d.tags, ["ecs", "api"]);
        assert_eq!(d.target_id(&none).unwrap(), "ecs/p99");
    }

    #[test]
    fn an_empty_slug_needs_a_path_and_a_fixed_path_is_validated() {
        let d = with_title("select 1 from t;", "¿¿??");
        assert_eq!(d.target_id(&none), Err(DraftError::NeedsPath));
        let mut d = with_title("select 1 from t;", "x y z");
        d.path = PathChoice::Fixed("sql/mine.md".into());
        assert_eq!(d.target_id(&none).unwrap(), "sql/mine");
        d.path = PathChoice::Fixed("../escape".into());
        assert!(matches!(d.target_id(&none), Err(DraftError::BadId(_))));
        d.path = PathChoice::Fixed("sql/mine".into());
        assert_eq!(
            d.target_id(&|id| id == "sql/mine"),
            Err(DraftError::Taken("sql/mine".into()))
        );
    }

    #[test]
    fn options_set_type_tags_and_path() {
        let opts = DraftOptions {
            ty: Some(EntryType::Prompt),
            path: Some("mine/x".into()),
            tags: vec!["a b".into(), "#A".into(), "".into()],
            ..Default::default()
        };
        let d = Draft::from_text("select 1 from t;", DraftSource::Stdin, &opts);
        assert_eq!(
            (d.entry_type, d.detection.confidence),
            (EntryType::Prompt, Confidence::High)
        );
        assert_eq!(d.path, PathChoice::Fixed("mine/x".into()));
        assert_eq!(d.tags, ["a-b", "a"]);
        assert!(d.type_chosen);
    }

    // ---- variables ----

    #[test]
    fn every_reference_is_declared_once_in_order() {
        let d = draft("kubectl -n {{ns}} get {{kind}} {{ns}} --limit {{limit}}");
        let names: Vec<_> = d.vars.iter().map(|v| (v.name.as_str(), v.kind)).collect();
        assert_eq!(
            names,
            [
                ("ns", VarKind::Text),
                ("kind", VarKind::Text),
                ("limit", VarKind::Number)
            ]
        );
    }

    #[test]
    fn kinds_come_from_names() {
        for n in ["minutes", "limit", "days", "count", "port"] {
            assert_eq!(default_kind(n), VarKind::Number, "{n}");
        }
        for n in [
            "token",
            "password",
            "secret",
            "key",
            "api_key",
            "gh_token",
            "db_password",
            "x_secret",
            "ssh_key",
        ] {
            assert_eq!(default_kind(n), VarKind::Secret, "{n}");
        }
        for n in ["env", "monkey", "keyboard", "tokens", "account", "ports"] {
            assert_eq!(default_kind(n), VarKind::Text, "{n}");
        }
        let d = draft("curl -H \"Authorization: {{api_token}}\" https://x.example.com/{{path}}");
        assert_eq!(d.vars[0].kind, VarKind::Secret);
        assert_eq!(d.vars[0].default, None);
    }

    #[test]
    fn defaults_are_checked_when_set() {
        let mut d = draft("echo {{token}} {{env}} {{minutes}}");
        assert_eq!(
            d.set_var_default("token", Some("x".into())),
            Err(DraftError::SecretDefault)
        );
        d.set_var_kind("env", VarKind::Enum).unwrap();
        d.set_var_options("env", vec!["dev".into(), "prod".into()])
            .unwrap();
        assert_eq!(
            d.set_var_default("env", Some("qa".into())),
            Err(DraftError::DefaultNotInOptions)
        );
        d.set_var_default("env", Some("prod".into())).unwrap();
        assert_eq!(d.vars[1].default.as_deref(), Some("prod"));
        assert_eq!(
            d.set_var_default("nope", None),
            Err(DraftError::UnknownVar("nope".into()))
        );
        // becoming secret drops the default; leaving enum drops the options
        d.set_var_kind("env", VarKind::Secret).unwrap();
        assert_eq!(
            (d.vars[1].default.clone(), d.vars[1].options.clone()),
            (None, vec![])
        );
        // narrowing the options drops a default that is no longer allowed
        d.set_var_kind("env", VarKind::Enum).unwrap();
        d.set_var_options("env", vec!["dev".into(), "prod".into()])
            .unwrap();
        d.set_var_default("env", Some("prod".into())).unwrap();
        d.set_var_options("env", vec!["dev".into()]).unwrap();
        assert_eq!(d.vars[1].default, None);
    }

    #[test]
    fn marking_literal_escapes_the_braces_and_drops_the_declaration() {
        let mut d = draft("echo {{name}} and {{name}} then {{other}}");
        d.mark_literal("name");
        assert_eq!(d.body, "echo \\{{name}} and \\{{name}} then {{other}}");
        assert_eq!(
            d.vars.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            ["other"]
        );
    }

    #[test]
    fn set_body_keeps_decisions_by_name_and_drops_vanished_variables() {
        let mut d = draft("echo {{a}} {{b}}");
        d.set_var_kind("a", VarKind::Number).unwrap();
        d.set_var_default("a", Some("5".into())).unwrap();
        d.set_body("echo {{c}} {{a}}".into(), false);
        let names: Vec<_> = d.vars.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["c", "a"]);
        assert_eq!(d.vars[1].kind, VarKind::Number);
        assert_eq!(d.vars[1].default.as_deref(), Some("5"));
    }

    // ---- types ----

    #[test]
    fn changing_the_type_keeps_title_tags_body_notes_and_vars() {
        let opts = DraftOptions {
            title: Some("T".into()),
            tags: vec!["x".into()],
            ..Default::default()
        };
        let mut d = Draft::from_text("select {{a}} from t;", DraftSource::Stdin, &opts);
        d.notes = "n".into();
        d.fields = DraftFields::Sql {
            dialect: Some("postgres".into()),
            database: None,
        };
        d.set_type(EntryType::Cmd);
        assert_eq!(d.entry_type, EntryType::Cmd);
        assert!(
            matches!(d.fields, DraftFields::Cmd { .. }),
            "{:?}",
            d.fields
        );
        assert_eq!(
            (
                d.title.as_str(),
                d.tags.clone(),
                d.body.as_str(),
                d.notes.as_str()
            ),
            ("T", vec!["x".to_string()], "select {{a}} from t;", "n")
        );
        assert_eq!(d.vars.len(), 1);
        assert!(d.type_chosen);
        // the same type keeps its fields
        d.set_type(EntryType::Sql);
        d.fields = DraftFields::Sql {
            dialect: Some("mysql".into()),
            database: None,
        };
        d.set_type(EntryType::Sql);
        assert_eq!(
            d.fields,
            DraftFields::Sql {
                dialect: Some("mysql".into()),
                database: None
            }
        );
    }

    #[test]
    fn link_and_code_have_their_fields_and_no_body() {
        let d = draft("https://example.com/x");
        assert_eq!(d.entry_type, EntryType::Link);
        assert_eq!(d.body, "");
        assert_eq!(
            d.fields,
            DraftFields::Link {
                url: "https://example.com/x".into()
            }
        );
        // forcing a type that the content can't fill keeps the text as the body
        let opts = DraftOptions {
            ty: Some(EntryType::Prompt),
            ..Default::default()
        };
        let p = Draft::from_text("https://example.com/x", DraftSource::Stdin, &opts);
        assert_eq!(p.body, "https://example.com/x");
        // forcing link on a URL fills the field
        let opts = DraftOptions {
            ty: Some(EntryType::Link),
            ..Default::default()
        };
        let l = Draft::from_text("https://example.com/x", DraftSource::Stdin, &opts);
        assert_eq!(
            l.fields,
            DraftFields::Link {
                url: "https://example.com/x".into()
            }
        );
    }

    #[test]
    fn set_body_redetects_unless_the_user_chose_the_type() {
        let mut d = draft("kubectl get pods -n prod");
        assert_eq!(d.entry_type, EntryType::Cmd);
        assert!(d.set_body("select 1 from t;".into(), true));
        assert_eq!(d.entry_type, EntryType::Sql);
        // a chosen type is kept for a weaker detection...
        d.set_type(EntryType::Prompt);
        assert!(!d.set_body("kubectl get pods".into(), true));
        assert_eq!(d.entry_type, EntryType::Prompt);
        // ...and replaced by a High one that differs
        assert!(d.set_body("select a, b from t where c = 1;".into(), true));
        assert_eq!(d.entry_type, EntryType::Sql);
    }

    #[test]
    fn empty_draft_has_no_body_and_low_confidence() {
        let d = Draft::empty(DraftSource::Empty, None, &DraftOptions::default());
        assert_eq!(d.body, "");
        assert_eq!(d.detection.confidence, Confidence::Low);
        assert_eq!(d.entry_type, EntryType::Cmd);
        let d = Draft::empty(
            DraftSource::Empty,
            Some(EntryType::Sql),
            &DraftOptions::default(),
        );
        assert_eq!(d.entry_type, EntryType::Sql);
        assert_eq!(d.detection.confidence, Confidence::High);
    }

    #[test]
    fn crlf_in_the_input_is_normalized_in_the_body() {
        assert_eq!(draft("select a\r\nfrom t;\r\n").body, "select a\nfrom t;");
        let _ = normalize_clipboard("");
    }
}
