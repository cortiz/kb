//! The query language: free-text terms, phrases and filters (see
//! `specs/002-index-search/contracts/query-syntax.md`).

use crate::entry::EntryType;
use crate::fold::{fold, words};

/// Results shown when `limit` is not given.
pub const DEFAULT_LIMIT: usize = 20;

/// Words dropped from free-text terms (kept inside quoted phrases).
pub const STOP_WORDS: [&str; 17] = [
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "in", "is", "it", "of", "on", "or",
    "the", "to",
];

/// The filter keys, for hints.
pub const FILTER_KEYS: &str = "type, tag, -tag, var, repo, in, unreviewed";

/// An indexed field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Title,
    Tags,
    Id,
    Meta,
    Payload,
    Notes,
}

impl Field {
    pub const ALL: [Field; 6] = [
        Field::Title,
        Field::Tags,
        Field::Id,
        Field::Meta,
        Field::Payload,
        Field::Notes,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Tags => "tags",
            Field::Id => "id",
            Field::Meta => "meta",
            Field::Payload => "payload",
            Field::Notes => "notes",
        }
    }
}

/// Something worth telling the user about a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    UnknownFilter(String),
    UnknownType(String),
}

/// A structured query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Free-text terms as typed, stop words removed.
    pub terms: Vec<String>,
    pub phrases: Vec<String>,
    /// OR between types.
    pub types: Vec<EntryType>,
    /// Type names that matched no type; if no valid type is given, nothing matches.
    pub bad_types: Vec<String>,
    /// AND.
    pub tags: Vec<String>,
    pub not_tags: Vec<String>,
    pub vars: Vec<String>,
    pub repos: Vec<String>,
    pub scope: Option<Field>,
    /// Filter by review status (agent-authored entries).
    pub unreviewed: Option<bool>,
    /// `0` means no limit.
    pub limit: usize,
    /// All free text as typed (stop words included), for the exact-title rule.
    pub raw_text: String,
    pub hints: Vec<Hint>,
}

/// A flag value that isn't valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError(pub String);

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for QueryError {}

fn push_unique<T: PartialEq>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
    }
}

fn norm_tag(s: &str) -> String {
    s.trim_start_matches('#').to_lowercase()
}

impl Default for Query {
    fn default() -> Self {
        Query {
            terms: Vec::new(),
            phrases: Vec::new(),
            types: Vec::new(),
            bad_types: Vec::new(),
            tags: Vec::new(),
            not_tags: Vec::new(),
            vars: Vec::new(),
            repos: Vec::new(),
            scope: None,
            unreviewed: None,
            limit: DEFAULT_LIMIT,
            raw_text: String::new(),
            hints: Vec::new(),
        }
    }
}

impl Query {
    /// Parses a raw query string. Never fails: every string is a valid query.
    pub fn parse(raw: &str) -> Query {
        let mut q = Query::default();
        let mut raw_parts: Vec<String> = Vec::new();
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i].is_whitespace() {
                i += 1;
                continue;
            }
            if chars[i] == '"' {
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && chars[j] != '"' {
                    j += 1;
                }
                let text: String = chars[start..j].iter().collect();
                i = (j + 1).min(chars.len() + 1);
                if !words(&text).is_empty() {
                    raw_parts.push(text.clone());
                    q.phrases.push(text);
                }
                continue;
            }
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            q.add_token(&token, &mut raw_parts);
        }
        q.raw_text = raw_parts.join(" ");
        q
    }

    fn add_token(&mut self, token: &str, raw_parts: &mut Vec<String>) {
        if let Some((key, value)) = token.split_once(':')
            && !key.is_empty()
            && !value.is_empty()
        {
            let k = key.to_lowercase();
            match k.as_str() {
                "type" => {
                    self.add_type(value);
                    return;
                }
                "tag" => {
                    push_unique(&mut self.tags, norm_tag(value));
                    return;
                }
                "-tag" => {
                    push_unique(&mut self.not_tags, norm_tag(value));
                    return;
                }
                "var" => {
                    push_unique(&mut self.vars, value.to_lowercase());
                    return;
                }
                "repo" => {
                    push_unique(&mut self.repos, fold(value));
                    return;
                }
                "in" => {
                    let f = match fold(value).as_str() {
                        "title" => Some(Field::Title),
                        "payload" => Some(Field::Payload),
                        "notes" => Some(Field::Notes),
                        _ => None,
                    };
                    match f {
                        Some(f) => self.scope = Some(f),
                        None => self.hints.push(Hint::UnknownFilter(format!("in:{value}"))),
                    }
                    return;
                }
                "unreviewed" => {
                    match fold(value).as_str() {
                        "true" | "1" | "yes" => self.unreviewed = Some(true),
                        "false" | "0" | "no" => self.unreviewed = Some(false),
                        _ => self.hints.push(Hint::UnknownFilter(format!("unreviewed:{value}"))),
                    }
                    return;
                }
                _ => {
                    let plausible = key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                        && !value.starts_with('/');
                    if plausible {
                        push_unique(&mut self.hints, Hint::UnknownFilter(k));
                    }
                }
            }
        }
        let text = token.trim_start_matches('-');
        let ws = words(text);
        if ws.is_empty() {
            return;
        }
        raw_parts.push(text.to_string());
        if ws.len() == 1 && STOP_WORDS.contains(&ws[0].text.as_str()) {
            return;
        }
        self.terms.push(text.to_string());
    }

    fn add_type(&mut self, name: &str) {
        match EntryType::from_name(&fold(name)) {
            Some(t) => push_unique(&mut self.types, t),
            None => {
                push_unique(&mut self.bad_types, name.to_string());
                push_unique(&mut self.hints, Hint::UnknownType(name.to_string()));
            }
        }
    }

    /// Builds the query from a raw string and the CLI flags. Flags and inline filters combine.
    pub fn from_parts(
        raw: &str,
        types: &[String],
        tags: &[String],
        not_tags: &[String],
        limit: Option<usize>,
    ) -> Result<Query, QueryError> {
        let mut q = Query::parse(raw);
        for t in types {
            match EntryType::from_name(&fold(t)) {
                Some(t) => push_unique(&mut q.types, t),
                None => {
                    let valid: Vec<_> = EntryType::ALL.iter().map(|t| t.name()).collect();
                    return Err(QueryError(format!(
                        "unknown type \"{t}\"; expected one of {}",
                        valid.join(", ")
                    )));
                }
            }
        }
        for t in tags {
            push_unique(&mut q.tags, norm_tag(t));
        }
        for t in not_tags {
            push_unique(&mut q.not_tags, norm_tag(t));
        }
        if let Some(l) = limit {
            q.limit = l;
        }
        Ok(q)
    }

    /// No terms and no phrases: filters (if any) narrow the list, which is ordered by frecency.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.phrases.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_and_phrases() {
        let q = Query::parse("5xx auth");
        assert_eq!(q.terms, vec!["5xx", "auth"]);
        assert_eq!(q.limit, 20);
        let q = Query::parse("\"role cache\"");
        assert_eq!(q.phrases, vec!["role cache"]);
        assert!(q.terms.is_empty());
        let q = Query::parse("\"role cache");
        assert_eq!(q.phrases, vec!["role cache"]);
        let q = Query::parse("the \"the cache\" of foo");
        assert_eq!(q.terms, vec!["foo"]);
        assert_eq!(q.phrases, vec!["the cache"]);
        assert_eq!(q.raw_text, "the the cache of foo");
    }

    #[test]
    fn empty_queries() {
        for s in ["", "  ", "the of a", "???", "-", ":", "\"???\"", "- -"] {
            assert!(Query::parse(s).is_empty(), "{s:?}");
        }
        assert!(!Query::parse("foo").is_empty());
    }

    #[test]
    fn leading_dash_is_ignored_for_words() {
        assert_eq!(Query::parse("-stage").terms, vec!["stage"]);
    }

    #[test]
    fn filters() {
        let q = Query::parse("latency type:cw tag:prod");
        assert_eq!(q.terms, vec!["latency"]);
        assert_eq!(q.types, vec![EntryType::Cw]);
        assert_eq!(q.tags, vec!["prod"]);
        let q = Query::parse(
            "TYPE:cw type:sql tag:#Prod tag:auth -tag:Stage var:Region repo:Acme/Api in:title",
        );
        assert_eq!(q.types, vec![EntryType::Cw, EntryType::Sql]);
        assert_eq!(q.tags, vec!["prod", "auth"]);
        assert_eq!(q.not_tags, vec!["stage"]);
        assert_eq!(q.vars, vec!["region"]);
        assert_eq!(q.repos, vec!["acme/api"]);
        assert_eq!(q.scope, Some(Field::Title));
        assert!(q.is_empty());
        let q = Query::parse("in:title in:notes x");
        assert_eq!(q.scope, Some(Field::Notes));
    }

    #[test]
    fn odd_filters_become_text_or_hints() {
        let q = Query::parse("tag:");
        assert_eq!(q.terms, vec!["tag:"]);
        let q = Query::parse("foo:bar");
        assert_eq!(q.terms, vec!["foo:bar"]);
        assert_eq!(q.hints, vec![Hint::UnknownFilter("foo".into())]);
        let q = Query::parse("type:nonsense");
        assert_eq!(q.bad_types, vec!["nonsense"]);
        assert!(q.types.is_empty());
        assert_eq!(q.hints, vec![Hint::UnknownType("nonsense".into())]);
        assert!(Query::parse("https://x.y/z").hints.is_empty());
    }

    #[test]
    fn flags_equal_syntax() {
        let a = Query::from_parts("latency", &["cw".into()], &["prod".into()], &[], None).unwrap();
        assert_eq!(a, Query::parse("latency type:cw tag:prod"));
        let b = Query::from_parts("x", &[], &[], &["stage".into()], Some(3)).unwrap();
        let mut c = Query::parse("x -tag:stage");
        c.limit = 3;
        assert_eq!(b, c);
        let d = Query::from_parts("type:cw", &["sql".into()], &[], &[], None).unwrap();
        assert_eq!(d.types, vec![EntryType::Cw, EntryType::Sql]);
        assert!(Query::from_parts("", &["nope".into()], &[], &[], None).is_err());
    }

    #[test]
    fn never_panics_on_random_input() {
        let alphabet: Vec<char> = "ab:\"- #é?*()ñ\t".chars().collect();
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        for _ in 0..1000 {
            let mut s = String::new();
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let mut y = x;
            for _ in 0..(y % 24) {
                y = y
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                s.push(alphabet[(y >> 33) as usize % alphabet.len()]);
            }
            let _ = Query::parse(&s);
        }
    }
}
