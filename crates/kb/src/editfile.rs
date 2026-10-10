//! Editing in the user's editor outside the UI: the template of `kb add --editor` and the prompt
//! both it and `kb edit` end with. Temp files come from the platform layer, are private, live in
//! the system temp directory and are deleted when dropped, on every path.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use kb_core::author::fsops::FsError;
use kb_core::author::render::Check;
use kb_core::author::{Draft, DraftOptions, PathChoice};
use kb_core::config::Loaded;
use kb_core::entry::{EntryType, parse_entry};
use kb_core::entry_json::entry_json;
use kb_core::lint::{Finding, Severity, check_text, rules::MAX_FILE_BYTES};
use kb_core::search::Kb;

use crate::author::{AddArgs, SECRET_WAYS_OUT, clock, finding_line};
use crate::output::CliError;
use crate::platform::Platform;
use crate::tui::terminal::{Signals, exit_code_for_signal};

/// What the user chose after seeing the problems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Edit,
    Save,
    Discard,
}

/// Maps what was typed to a choice. `s` is only taken when saving is allowed.
pub fn parse_choice(answer: &str, allow_save: bool) -> Option<Choice> {
    match answer.trim().to_ascii_lowercase().as_str() {
        "e" | "edit" => Some(Choice::Edit),
        "s" | "save" if allow_save => Some(Choice::Save),
        "d" | "discard" => Some(Choice::Discard),
        _ => None,
    }
}

/// Asks `[e]dit again  [s]ave anyway  [d]iscard:` on the terminal until the answer is one of
/// them. End of input and a signal are a discard. The line is read on a helper thread so that a
/// signal is noticed while waiting.
pub fn prompt_choice(tty: &File, signals: &Signals, allow_save: bool) -> Choice {
    let prompt = if allow_save {
        "[e]dit again  [s]ave anyway  [d]iscard: "
    } else {
        "[e]dit again  [d]iscard: "
    };
    loop {
        match ask_line(tty, signals, prompt) {
            None => return Choice::Discard,
            Some(line) => {
                if let Some(choice) = parse_choice(&line, allow_save) {
                    return choice;
                }
            }
        }
    }
}

/// Writes `prompt` on the terminal and reads one line. `None` at end of input or at a signal.
pub fn ask_line(tty: &File, signals: &Signals, prompt: &str) -> Option<String> {
    let mut out = tty.try_clone().ok()?;
    let _ = write!(out, "{prompt}");
    let _ = out.flush();
    let input = tty.try_clone().ok()?;
    let (tx, rx) = mpsc::channel();
    // ends when a line arrives; at a signal the process is about to exit anyway
    std::thread::spawn(move || {
        let mut line = String::new();
        let n = BufReader::new(input).read_line(&mut line).unwrap_or(0);
        let _ = tx.send((n, line));
    });
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((0, _)) => return None,
            Ok((_, line)) => return Some(line),
            Err(mpsc::RecvTimeoutError::Timeout) if signals.received().is_none() => {}
            Err(_) => return None,
        }
    }
}

/// The errors and warnings of `findings` on stderr, then the way out for secrets.
pub fn print_findings(findings: &[Finding]) {
    for f in findings.iter().filter(|f| f.severity != Severity::Info) {
        eprintln!("{}", finding_line(f));
    }
    if findings
        .iter()
        .any(|f| (60..=65).contains(&f.code.number()))
    {
        eprintln!("kb: {SECRET_WAYS_OUT}");
    }
}

fn is_secret(f: &Finding) -> bool {
    (60..=65).contains(&f.code.number())
}

/// The ID a hand-written file would get: `--path`, else the template rule on its type and title.
fn id_for(path: &Option<String>, bytes: &[u8], kb: &Kb, loaded: &Loaded) -> Result<String, String> {
    let taken = |id: &str| kb.id_taken(id);
    let parsed = parse_entry("new", "new.md", bytes.to_vec()).entry;
    let opts = DraftOptions {
        title: parsed.as_ref().and_then(|e| e.title()).map(str::to_string),
        ty: parsed.as_ref().and_then(|e| e.entry_type()),
        tags: parsed
            .as_ref()
            .map(|e| e.tags().to_vec())
            .unwrap_or_default(),
        path: path.clone(),
        path_template: loaded.settings.path_template(),
    };
    let mut draft = Draft::empty(
        kb_core::author::DraftSource::Editor,
        Some(opts.ty.unwrap_or(EntryType::Cmd)),
        &opts,
    );
    if let Some(p) = path {
        draft.path = PathChoice::Fixed(p.clone());
    }
    draft.target_id(&taken).map_err(|e| match e {
        kb_core::author::DraftError::NeedsPath => {
            "cannot tell where to save it: the type or title is missing (use --path)".to_string()
        }
        other => other.to_string(),
    })
}

/// `kb add --editor`: the template for `draft` in the editor, checked when the editor closes.
pub fn add_with_editor(
    platform: &impl Platform,
    kb: &mut Kb,
    loaded: &Loaded,
    draft: &Draft,
    args: &AddArgs,
) -> Result<ExitCode, CliError> {
    let tty = platform
        .open_tty()
        .map_err(|_| CliError::new("usage", "no terminal to run the editor on"))?;
    let signals = Signals::register();
    let clock = clock();
    let template = draft.template(clock.as_ref());
    let temp = platform
        .private_temp_file("md", template.as_bytes())
        .map_err(|e| CliError::new("io", format!("could not make a temporary file: {e}")))?;
    let editor = platform.resolve_editor(loaded.settings.editor());
    let path = args
        .path
        .clone()
        .map(|p| p.strip_suffix(".md").map_or(p.clone(), str::to_string));
    loop {
        let status = platform
            .run_editor(&editor, temp.path(), &tty, &|| signals.received().is_some())
            .map_err(|e| CliError::new("io", format!("could not start the editor: {e}")))?;
        let Some(status) = status else {
            return Ok(ExitCode::from(
                u8::try_from(exit_code_for_signal(signals.received().unwrap_or(15))).unwrap_or(130),
            ));
        };
        if !status.success() {
            eprintln!("kb: the editor exited with an error; nothing was saved");
            return Ok(ExitCode::from(130));
        }
        let bytes = temp
            .read_bytes()
            .map_err(|e| CliError::new("io", format!("could not read the edited file: {e}")))?;
        if bytes == template.as_bytes() {
            eprintln!("kb: nothing changed; nothing was saved");
            return Ok(ExitCode::from(130));
        }
        let id = id_for(&path, &bytes, kb, loaded);
        let shown_id = id.clone().unwrap_or_else(|_| "untitled".into());
        let findings = check_text(&shown_id, bytes.clone());
        let too_large = bytes.len() > MAX_FILE_BYTES;
        let secrets = findings.iter().any(is_secret);
        let errors = findings.iter().any(|f| f.severity == Severity::Error);
        if let (Ok(id), false, false, false) = (&id, errors, secrets, too_large) {
            print_findings(&findings);
            return finish(kb, &tty, id, bytes, findings, args.json);
        }
        print_findings(&findings);
        if too_large {
            eprintln!(
                "kb: the entry is {} bytes; entries are limited to {MAX_FILE_BYTES}",
                bytes.len()
            );
        }
        if let Err(why) = &id {
            eprintln!("kb: {why}");
        }
        let allow_save = id.is_ok() && !secrets && !too_large;
        match prompt_choice(&tty, &signals, allow_save) {
            Choice::Edit => {}
            Choice::Discard => return Ok(ExitCode::from(130)),
            Choice::Save => {
                let id = id.expect("saving is only offered with an ID");
                eprintln!(
                    "kb: saved {id} with errors; it stays out of search until they are fixed"
                );
                return finish(kb, &tty, &id, bytes, findings, args.json);
            }
        }
    }
}

fn finish(
    kb: &mut Kb,
    _tty: &File,
    id: &str,
    bytes: Vec<u8>,
    findings: Vec<Finding>,
    json: bool,
) -> Result<ExitCode, CliError> {
    let check = Check {
        id: id.to_string(),
        bytes,
        findings,
        too_large: false,
    };
    match kb.create_entry(&check) {
        Ok(path) => {
            if json {
                let doc = kb
                    .entry(id)
                    .map(|e| entry_json(&e, &path))
                    .unwrap_or_else(|_| serde_json::json!({ "id": id }));
                println!("{doc}");
            } else {
                println!("{id}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(FsError::Exists(id)) => {
            eprintln!("kb: {id} already exists");
            Ok(ExitCode::from(1))
        }
        Err(FsError::Secret(_)) => {
            eprintln!("kb: {SECRET_WAYS_OUT}");
            Ok(ExitCode::from(1))
        }
        Err(e) => Err(CliError::new("io", e.to_string())),
    }
}

/// An entry for `kb edit`: an exact ID, else the one entry a query finds.
fn resolve_for_edit(kb: &mut Kb, query: &str) -> Result<String, String> {
    let id = query.strip_suffix(".md").unwrap_or(query);
    if kb.entry_path(id).is_ok() {
        return Ok(id.to_string());
    }
    let outcome = kb
        .search(
            &kb_core::query::Query::parse(query),
            std::time::SystemTime::now(),
        )
        .map_err(|e| e.to_string())?;
    match outcome.results.as_slice() {
        [] => Err(format!("no entry matches \"{query}\"")),
        [only] => Ok(only.id.clone()),
        many => {
            let ids: Vec<&str> = many.iter().take(5).map(|r| r.id.as_str()).collect();
            Err(format!(
                "{} entries match \"{query}\"; say which: {}",
                outcome.total,
                ids.join(", ")
            ))
        }
    }
}

fn edit_json(id: &str, status: &str, findings: &[Finding]) {
    println!(
        "{}",
        serde_json::json!({ "id": id, "status": status, "findings": findings })
    );
}

/// `kb edit <id|query>`: the entry's file in the editor, then the same checks as a new entry.
/// A clean result gets a new `updated`; problems get the edit / save anyway / discard prompt,
/// where discard puts the old bytes back.
pub fn edit_entry(
    platform: &impl Platform,
    root: &Path,
    loaded: &Loaded,
    query: &str,
    json: bool,
) -> Result<ExitCode, CliError> {
    let mut kb = Kb::open(root).map_err(|e| CliError::new("io", e.to_string()))?;
    let id = match resolve_for_edit(&mut kb, query) {
        Ok(id) => id,
        Err(message) => {
            eprintln!("kb: {message}");
            return Ok(ExitCode::from(1));
        }
    };
    let path = kb
        .entry_path(&id)
        .map_err(|e| CliError::new("io", e.to_string()))?;
    let tty = platform
        .open_tty()
        .map_err(|e| CliError::new("io", format!("no terminal to run the editor on: {e}")))?;
    let signals = Signals::register();
    let editor = platform.resolve_editor(loaded.settings.editor());
    let snapshot = std::fs::read(&path).map_err(|e| CliError::new("io", e.to_string()))?;
    let restore = |kb: &mut Kb| {
        let _ = kb.restore(&id, &snapshot);
    };
    let clock = clock();
    loop {
        let status = platform
            .run_editor(&editor, &path, &tty, &|| signals.received().is_some())
            .map_err(|e| CliError::new("io", format!("could not start the editor: {e}")))?;
        let bytes = std::fs::read(&path).unwrap_or_default();
        let Some(status) = status else {
            if bytes != snapshot {
                restore(&mut kb);
            }
            return Ok(ExitCode::from(
                u8::try_from(exit_code_for_signal(signals.received().unwrap_or(15))).unwrap_or(130),
            ));
        };
        if !status.success() {
            if bytes != snapshot {
                restore(&mut kb);
            }
            eprintln!("kb: the editor exited with an error; the entry is unchanged");
            if json {
                edit_json(&id, "discarded", &[]);
            }
            return Ok(ExitCode::from(130));
        }
        if bytes == snapshot {
            if json {
                edit_json(&id, "unchanged", &[]);
            }
            return Ok(ExitCode::SUCCESS);
        }
        let findings = check_text(&id, bytes.clone());
        let secrets = findings.iter().any(is_secret);
        let errors = findings.iter().any(|f| f.severity == Severity::Error);
        let too_large = bytes.len() > MAX_FILE_BYTES;
        let save = |kb: &mut Kb| -> Result<ExitCode, CliError> {
            let version = kb
                .version(&id)
                .map_err(|e| CliError::new("io", e.to_string()))?;
            match kb.save_edit(&id, &version, clock.as_ref()) {
                Ok(_) => Ok(ExitCode::SUCCESS),
                Err(FsError::Conflict(_)) => {
                    eprintln!("kb: the file changed while it was being saved; run kb edit again");
                    Ok(ExitCode::from(1))
                }
                Err(FsError::Secret(_)) => {
                    eprintln!("kb: {SECRET_WAYS_OUT}");
                    Ok(ExitCode::from(1))
                }
                Err(e) => Err(CliError::new("io", e.to_string())),
            }
        };
        if !errors && !secrets && !too_large {
            let code = save(&mut kb)?;
            if json {
                edit_json(&id, "saved", &findings);
            }
            return Ok(code);
        }
        print_findings(&findings);
        if too_large {
            eprintln!(
                "kb: the entry is {} bytes; entries are limited to {MAX_FILE_BYTES}",
                bytes.len()
            );
        }
        if json {
            // no prompts in a scripted run: the file stays as the editor left it
            edit_json(&id, "invalid", &findings);
            return Ok(ExitCode::from(1));
        }
        let parses = parse_entry(&id, &format!("{id}.md"), bytes.clone())
            .entry
            .is_some();
        match prompt_choice(&tty, &signals, !secrets && !too_large) {
            Choice::Edit => {}
            Choice::Discard => {
                restore(&mut kb);
                return Ok(ExitCode::from(130));
            }
            Choice::Save => {
                eprintln!(
                    "kb: saved {id} with errors; it stays out of search until they are fixed"
                );
                return if parses {
                    save(&mut kb)
                } else {
                    Ok(ExitCode::SUCCESS)
                };
            }
        }
    }
}

/// True when `path` is inside `root` (used to assert temp files never land in the KB).
pub fn inside(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_map_to_choices() {
        assert_eq!(parse_choice("e\n", true), Some(Choice::Edit));
        assert_eq!(parse_choice(" S ", true), Some(Choice::Save));
        assert_eq!(parse_choice("discard", false), Some(Choice::Discard));
        assert_eq!(parse_choice("s", false), None, "save is not offered");
        assert_eq!(parse_choice("", true), None);
        assert_eq!(parse_choice("x", true), None);
    }
}
