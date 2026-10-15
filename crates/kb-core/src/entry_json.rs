//! What picking an entry prints (`kb show`, Enter in the TUI) and the JSON form of an entry.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::entry::{Entry, EntryType, Shell, TypeFields};
use crate::vars::{Escape, VarKind};

/// What a pick writes to stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrintMode {
    /// The payload (the URL for a link, `path:line` for a code entry).
    #[default]
    Payload,
    Id,
    Path,
    Json,
}

impl PrintMode {
    pub const NAMES: [&'static str; 4] = ["payload", "id", "path", "json"];

    pub fn parse(s: &str) -> Option<PrintMode> {
        match s {
            "payload" => Some(PrintMode::Payload),
            "id" => Some(PrintMode::Id),
            "path" => Some(PrintMode::Path),
            "json" => Some(PrintMode::Json),
            _ => None,
        }
    }
}

/// The entry has no payload to print (its payload block is missing or the type is unknown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickError;

impl std::fmt::Display for PickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the entry has no payload (run kb lint)")
    }
}

impl std::error::Error for PickError {}

fn shell_name(s: Shell) -> &'static str {
    match s {
        Shell::Sh => "sh",
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Fish => "fish",
    }
}

fn kind_name(k: VarKind) -> &'static str {
    match k {
        VarKind::Text => "text",
        VarKind::Number => "number",
        VarKind::Enum => "enum",
        VarKind::Secret => "secret",
    }
}

fn fields_json(entry: &Entry) -> Value {
    fn put(m: &mut Map<String, Value>, k: &str, v: &Option<String>) {
        if let Some(v) = v {
            m.insert(k.into(), json!(v));
        }
    }
    let mut m = Map::new();
    match entry.fields() {
        TypeFields::Sql { dialect, database } => {
            put(&mut m, "dialect", dialect);
            put(&mut m, "database", database);
        }
        TypeFields::Sumo { range } => put(&mut m, "range", range),
        TypeFields::Cw {
            log_groups,
            range,
            region,
        } => {
            put(&mut m, "range", range);
            put(&mut m, "region", region);
            if !log_groups.is_empty() {
                m.insert("log_groups".into(), json!(log_groups));
            }
        }
        TypeFields::Cmd { shell, cwd } => {
            m.insert("shell".into(), json!(shell_name(*shell)));
            put(&mut m, "cwd", cwd);
        }
        TypeFields::Link { url } => put(&mut m, "url", url),
        TypeFields::Code { code: Some(c) } => {
            let mut code = Map::new();
            code.insert("repo".into(), json!(c.repo));
            code.insert("path".into(), json!(c.path));
            code.insert("line".into(), json!(c.line));
            code.insert("ref".into(), json!(c.git_ref));
            if let Some(s) = &c.symbol {
                code.insert("symbol".into(), json!(s));
            }
            if let Some(e) = c.end_line {
                code.insert("end_line".into(), json!(e));
            }
            m.insert("code".into(), Value::Object(code));
        }
        TypeFields::Code { code: None }
        | TypeFields::Prompt
        | TypeFields::Note
        | TypeFields::Unknown => {}
        TypeFields::Runbook { steps } => {
            m.insert("steps".into(), json!(steps));
        }
    }
    Value::Object(m)
}

fn escape_name(e: Escape) -> &'static str {
    Escape::NAMES[e as usize]
}

/// The entry as the document in `specs/003-tui-core/contracts/entry.schema.json`.
pub fn entry_json(entry: &Entry, abs_path: &Path) -> Value {
    let vars: Vec<Value> = entry
        .vars()
        .iter()
        .map(|v| {
            let mut m = Map::new();
            m.insert("name".into(), json!(v.name));
            m.insert("kind".into(), json!(kind_name(v.kind)));
            m.insert("declared".into(), json!(v.declared));
            if v.kind != VarKind::Secret {
                m.insert("default".into(), json!(v.default));
            }
            m.insert("options".into(), json!(v.options));
            m.insert("description".into(), json!(v.description));
            m.insert("env".into(), json!(v.env));
            m.insert("escape".into(), json!(escape_name(v.escape)));
            Value::Object(m)
        })
        .collect();
    json!({
        "id": entry.id(),
        "path": entry.path(),
        "file": abs_path.to_string_lossy(),
        "type": entry.entry_type().map(EntryType::name),
        "title": entry.title(),
        "tags": entry.tags(),
        "created": entry.created(),
        "updated": entry.updated(),
        "vars": vars,
        "payload": entry.payload().map(|p| p.text.clone()),
        "notes": entry.notes(),
        "fields": fields_json(entry),
    })
}

/// A note's Markdown as written: everything after the frontmatter, minus the blank lines that
/// separate it from the closing `---`.
pub fn note_source(entry: &Entry) -> &str {
    entry.notes().trim_start_matches(['\r', '\n'])
}

/// Exactly the bytes `kb show --print <mode>` writes. Nothing is added to the payload, ID or path;
/// the JSON document ends with one newline.
pub fn pick_output(entry: &Entry, abs_path: &Path, mode: PrintMode) -> Result<String, PickError> {
    match mode {
        PrintMode::Id => Ok(entry.id().to_string()),
        PrintMode::Path => Ok(abs_path.to_string_lossy().into_owned()),
        PrintMode::Json => Ok(format!("{}\n", entry_json(entry, abs_path))),
        PrintMode::Payload => match entry.fields() {
            TypeFields::Link { url: Some(url) } => Ok(url.clone()),
            TypeFields::Code { code: Some(c) } => Ok(format!("{}:{}", c.path, c.line)),
            // a note is all body: its Markdown, without the frontmatter and without anything a
            // terminal would act on
            TypeFields::Note => Ok(crate::terminal::raw(note_source(entry)).into_owned()),
            TypeFields::Runbook { .. } => Ok(crate::runbook::checklist::generate_checklist(entry)),
            _ => entry.payload().map(|p| p.text.clone()).ok_or(PickError),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::parse_entry;

    fn fixture(dir: &str, id: &str) -> (Entry, std::path::PathBuf) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(dir);
        let path = root.join(format!("{id}.md"));
        let bytes = std::fs::read(&path).unwrap();
        (
            parse_entry(id, &format!("{id}.md"), bytes).entry.unwrap(),
            path,
        )
    }

    #[test]
    fn parse_modes() {
        for (s, m) in [
            ("payload", PrintMode::Payload),
            ("id", PrintMode::Id),
            ("path", PrintMode::Path),
            ("json", PrintMode::Json),
        ] {
            assert_eq!(PrintMode::parse(s), Some(m));
        }
        assert_eq!(PrintMode::parse("Payload"), None);
        assert_eq!(PrintMode::parse(""), None);
    }

    #[test]
    fn payload_url_and_code_location() {
        let (e, p) = fixture("search", "sql/orders-by-status");
        assert_eq!(
            pick_output(&e, &p, PrintMode::Payload).unwrap(),
            e.payload().unwrap().text
        );
        assert!(
            !pick_output(&e, &p, PrintMode::Payload)
                .unwrap()
                .ends_with("\n\n")
        );
        let (e, p) = fixture("search", "link/payments-runbook");
        assert_eq!(
            pick_output(&e, &p, PrintMode::Payload).unwrap(),
            "https://wiki.example.com/runbooks/payments"
        );
        let (e, p) = fixture("search", "code/request-handler");
        assert_eq!(
            pick_output(&e, &p, PrintMode::Payload).unwrap(),
            "src/handler.rs:10"
        );
        assert_eq!(
            pick_output(&e, &p, PrintMode::Id).unwrap(),
            "code/request-handler"
        );
        assert_eq!(
            pick_output(&e, &p, PrintMode::Path).unwrap(),
            p.to_string_lossy()
        );
    }

    #[test]
    fn a_note_prints_its_markdown_without_frontmatter_or_escape_sequences() {
        let text = "---\nkb_format: 1\ntype: note\ntitle: N\n---\n\n# N\n\nbody \x1b[2J here\n";
        let e = parse_entry("note/n", "note/n.md", text.as_bytes().to_vec())
            .entry
            .unwrap();
        let out = pick_output(&e, Path::new("/k/note/n.md"), PrintMode::Payload).unwrap();
        assert_eq!(out, "# N\n\nbody  here\n");
        assert_eq!(note_source(&e), "# N\n\nbody \x1b[2J here\n");
        let json = entry_json(&e, Path::new("/k/note/n.md"));
        assert_eq!(json["type"], "note");
        assert_eq!(json["payload"], Value::Null);
        assert_eq!(json["notes"], e.notes());
    }

    #[test]
    fn payload_keeps_its_own_trailing_newline_only() {
        let text =
            "---\nkb_format: 1\ntype: prompt\ntitle: T\n---\n\n```\nline one\nline two\n```\n";
        let e = parse_entry("x", "x.md", text.as_bytes().to_vec())
            .entry
            .unwrap();
        let out = pick_output(&e, Path::new("/k/x.md"), PrintMode::Payload).unwrap();
        assert_eq!(out, e.payload().unwrap().text);
        assert!(out.starts_with("line one\nline two"));
    }

    #[test]
    fn json_has_the_documented_shape_for_every_type() {
        let required = [
            "id", "path", "file", "type", "title", "tags", "vars", "payload", "notes", "fields",
        ];
        let mut seen = std::collections::BTreeSet::new();
        for (dir, id) in [
            ("search", "sql/orders-by-status"),
            ("search", "sumo/5xx-by-authorizer"),
            ("search", "cw/authorizer-latency"),
            ("search", "cmd/kubectl-pods"),
            ("search", "prompt/cache"),
            ("search", "link/payments-runbook"),
            ("search", "code/request-handler"),
            ("tui", "cmd/deploy-with-secret"),
        ] {
            let (e, p) = fixture(dir, id);
            let v = entry_json(&e, &p);
            for k in required {
                assert!(v.get(k).is_some(), "{id}: missing {k}");
            }
            assert!(v["tags"].is_array() && v["vars"].is_array() && v["fields"].is_object());
            assert_eq!(v["id"], id);
            assert_eq!(v["file"], p.to_string_lossy().as_ref());
            seen.insert(v["type"].as_str().unwrap().to_string());
            let out = pick_output(&e, &p, PrintMode::Json).unwrap();
            assert!(out.ends_with("}\n") && serde_json::from_str::<Value>(&out).unwrap() == v);
        }
        assert_eq!(seen.len(), 7);
        let (e, p) = fixture("search", "link/payments-runbook");
        assert_eq!(entry_json(&e, &p)["payload"], Value::Null);
        assert_eq!(
            entry_json(&e, &p)["fields"]["url"],
            "https://wiki.example.com/runbooks/payments"
        );
        let (e, p) = fixture("search", "code/request-handler");
        assert_eq!(
            entry_json(&e, &p)["fields"]["code"]["symbol"],
            "handle_request"
        );
        let (e, p) = fixture("search", "cw/authorizer-latency");
        assert_eq!(
            entry_json(&e, &p)["fields"]["log_groups"],
            json!(["/aws/lambda/authorizer"])
        );
    }

    #[test]
    fn secret_variables_never_carry_a_default() {
        let (e, p) = fixture("tui", "cmd/deploy-with-secret");
        let v = entry_json(&e, &p);
        let vars = v["vars"].as_array().unwrap();
        let token = vars.iter().find(|x| x["name"] == "token").unwrap();
        assert_eq!(token["kind"], "secret");
        assert!(token.get("default").is_none());
        let env = vars.iter().find(|x| x["name"] == "env").unwrap();
        assert_eq!(env["default"], "stage");
        assert_eq!(env["options"], json!(["prod", "stage"]));
    }

    #[test]
    fn a_missing_payload_block_is_an_error() {
        let text = "---\nkb_format: 1\ntype: prompt\ntitle: T\n---\n\nno code block\n";
        let e = parse_entry("x", "x.md", text.as_bytes().to_vec())
            .entry
            .unwrap();
        assert_eq!(
            pick_output(&e, Path::new("/x.md"), PrintMode::Payload),
            Err(PickError)
        );
        assert_eq!(
            pick_output(&e, Path::new("/x.md"), PrintMode::Id).unwrap(),
            "x"
        );
    }
}
