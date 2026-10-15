//! Creating, saving, renaming and removing entry files (FR-017 to FR-024, research R7).
//!
//! Nothing here starts a process: `git mv` / `git rm` come in as closures from the `kb` crate.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::author::Clock;
use crate::author::render::{Check, rfc3339_utc};
use crate::edit::{FieldValue, write_entry};
use crate::entry::parse_entry;
use crate::lint::{Finding, check_text, rules::MAX_FILE_BYTES};
use crate::search::{Kb, valid_id};
use crate::usage::{State, content_hash};

/// What a file looked like when it was read; the conflict test for the write that follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub hash: u64,
    pub len: u64,
    /// Informational; equality of `hash` and `len` is the test.
    pub mtime_ns: i64,
}

#[derive(Debug)]
pub enum FsError {
    BadId(String),
    NotFound(String),
    Exists(String),
    /// The file changed on disk since it was read.
    Conflict(String),
    /// The bytes contain likely secrets (constitution VI); nothing was written.
    Secret(Vec<Finding>),
    TooLarge,
    /// The edit can't be made in place (for example an unsupported `kb_format`).
    Unsupported(String),
    /// The entry is referenced by runbooks and cannot be deleted without force.
    ReferencedByRunbooks {
        id: String,
        runbooks: Vec<String>,
    },
    Io {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::BadId(id) => write!(f, "\"{id}\" is not a valid entry ID"),
            FsError::NotFound(id) => write!(f, "no entry with ID \"{id}\""),
            FsError::Exists(id) => write!(f, "{id} already exists"),
            FsError::Conflict(_) => write!(
                f,
                "the file changed while it was being saved; run kb edit again"
            ),
            FsError::Secret(found) => write!(
                f,
                "refusing to write an entry with {} likely secret(s)",
                found.len()
            ),
            FsError::TooLarge => write!(f, "entries are limited to {MAX_FILE_BYTES} bytes"),
            FsError::Unsupported(why) => write!(f, "{why}"),
            FsError::ReferencedByRunbooks { id, runbooks } => write!(
                f,
                "cannot delete '{id}': referenced by runbook(s) ({})",
                runbooks.join(", ")
            ),
            FsError::Io { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for FsError {}

/// The result of a rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renamed {
    pub from: String,
    pub to: String,
    /// Usage or remembered values were carried to the new ID.
    pub carried: bool,
}

/// The result of `save_edit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub id: String,
    pub updated: String,
}

fn io_err(path: &Path, e: &io::Error) -> FsError {
    FsError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    }
}

/// A valid ID that doesn't reach into hidden folders such as `.kb`.
fn usable_id(id: &str) -> bool {
    valid_id(id) && !id.split('/').any(|c| c.starts_with('.'))
}

impl Kb {
    fn id_path(&self, id: &str) -> Result<PathBuf, FsError> {
        if !usable_id(id) {
            return Err(FsError::BadId(id.to_string()));
        }
        Ok(self.root().join(format!("{id}.md")))
    }

    /// Whether an entry with this ID exists, ignoring case.
    pub fn id_taken(&self, id: &str) -> bool {
        if !usable_id(id) {
            return false;
        }
        let parts: Vec<&str> = id.split('/').collect();
        let (last, dirs) = parts.split_last().expect("a valid ID has a part");
        let mut dir = self.root().to_path_buf();
        for part in dirs {
            match find_ignoring_case(&dir, part, true) {
                Some(found) => dir = found,
                None => return false,
            }
        }
        find_ignoring_case(&dir, &format!("{last}.md"), false).is_some()
    }

    pub fn version(&self, id: &str) -> Result<Version, FsError> {
        let path = self.id_path(id)?;
        let bytes = read_entry(&path, id)?;
        let mtime_ns = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
        Ok(Version {
            hash: content_hash(&bytes),
            len: bytes.len() as u64,
            mtime_ns,
        })
    }

    /// Writes a new entry. Atomic and never over an existing file; creates folders; refreshes the
    /// index. Scans the bytes for secrets itself, whatever `check` says.
    pub fn create_entry(&mut self, check: &Check) -> Result<PathBuf, FsError> {
        let path = self.id_path(&check.id)?;
        if check.too_large || check.bytes.len() > MAX_FILE_BYTES {
            return Err(FsError::TooLarge);
        }
        let secrets: Vec<Finding> = check_text(&check.id, check.bytes.clone())
            .into_iter()
            .filter(|f| (60..=65).contains(&f.code.number()))
            .collect();
        if !secrets.is_empty() {
            return Err(FsError::Secret(secrets));
        }
        if self.id_taken(&check.id) {
            return Err(FsError::Exists(check.id.clone()));
        }
        let dir = path.parent().expect("an entry path has a folder");
        std::fs::create_dir_all(dir).map_err(|e| io_err(dir, &e))?;
        let result = create_new_file(&path, &check.bytes, &check.id);
        if result.is_err() {
            remove_empty_dirs(self.root(), dir);
        }
        result?;
        let _ = self.refresh();
        Ok(path)
    }

    /// Bumps `updated` in an entry edited elsewhere (format-preserving) and writes it atomically,
    /// unless the file is no longer what `based_on` describes.
    pub fn save_edit(
        &mut self,
        id: &str,
        based_on: &Version,
        clock: &dyn Clock,
    ) -> Result<Saved, FsError> {
        let path = self.id_path(id)?;
        let bytes = read_entry(&path, id)?;
        if content_hash(&bytes) != based_on.hash || bytes.len() as u64 != based_on.len {
            return Err(FsError::Conflict(id.to_string()));
        }
        let mut entry = parse_entry(id, &format!("{id}.md"), bytes)
            .entry
            .ok_or_else(|| FsError::Unsupported(format!("{id} can't be read as an entry")))?;
        let updated = rfc3339_utc(clock.now());
        entry
            .set_field(&["updated"], FieldValue::Str(updated.clone()))
            .map_err(|e| FsError::Unsupported(e.to_string()))?;
        write_entry(&path, &entry).map_err(|e| match e {
            crate::edit::WriteError::Secret(found) => FsError::Secret(found),
            crate::edit::WriteError::UnsupportedFormat => FsError::Unsupported(e.to_string()),
            crate::edit::WriteError::Io(io) => io_err(&path, &io),
        })?;
        let _ = self.refresh();
        Ok(Saved {
            id: id.to_string(),
            updated,
        })
    }

    /// Puts `bytes` back as the entry's file (the "discard" of `kb edit`).
    pub fn restore(&mut self, id: &str, bytes: &[u8]) -> Result<(), FsError> {
        let path = self.id_path(id)?;
        let dir = path.parent().expect("an entry path has a folder");
        std::fs::create_dir_all(dir).map_err(|e| io_err(dir, &e))?;
        let tmp = temp_path(&path);
        write_temp(&tmp, bytes)
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                io_err(&path, &e)
            })?;
        let _ = self.refresh();
        Ok(())
    }

    /// Renames an entry with `mover` (a plain rename, or `git mv`), carrying usage and remembered
    /// values to the new ID whatever the content. If `mover` fails nothing else happens.
    pub fn rename_entry(
        &mut self,
        old: &str,
        new: &str,
        mover: &dyn Fn(&Path, &Path) -> io::Result<()>,
    ) -> Result<Renamed, FsError> {
        let old_path = self.id_path(old)?;
        let new_path = self.id_path(new)?;
        if !old_path.is_file() {
            return Err(FsError::NotFound(old.to_string()));
        }
        let case_only = old != new && old.eq_ignore_ascii_case(new);
        if old == new || (!case_only && self.id_taken(new)) {
            return Err(FsError::Exists(new.to_string()));
        }
        let new_dir = new_path.parent().expect("an entry path has a folder");
        std::fs::create_dir_all(new_dir).map_err(|e| io_err(new_dir, &e))?;
        if let Err(e) = mover(&old_path, &new_path) {
            remove_empty_dirs(self.root(), new_dir);
            return Err(io_err(&old_path, &e));
        }
        let (mut state, _) = State::load(self.root());
        let carried = state.move_entry(old, new);
        if carried {
            let _ = state.save(self.root());
        }
        remove_empty_dirs(self.root(), old_path.parent().expect("a folder"));
        let _ = self.refresh();
        Ok(Renamed {
            from: old.to_string(),
            to: new.to_string(),
            carried,
        })
    }

    /// Points the `[[old]]` links of the notes `sources` at `new`, changing nothing else in them
    /// (not even `updated`). Returns the notes that changed.
    pub fn retarget_links(
        &mut self,
        sources: &[String],
        old: &str,
        new: &str,
    ) -> Result<Vec<String>, FsError> {
        let mut changed = Vec::new();
        for id in sources {
            let path = self.id_path(id)?;
            let bytes = read_entry(&path, id)?;
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let Some(entry) = parse_entry(id, &format!("{id}.md"), text.clone().into_bytes()).entry
            else {
                continue;
            };
            // only links in the body count: the frontmatter is left alone
            let body = entry.split.body.clone();
            let Some(body_text) = crate::entry::link::retarget(&text[body.clone()], old, new)
            else {
                continue;
            };
            let rewritten = format!("{}{}{}", &text[..body.start], body_text, &text[body.end..]);
            let tmp = temp_path(&path);
            write_temp(&tmp, rewritten.as_bytes()).map_err(|e| io_err(&tmp, &e))?;
            if let Err(e) = std::fs::rename(&tmp, &path) {
                let _ = std::fs::remove_file(&tmp);
                return Err(io_err(&path, &e));
            }
            changed.push(id.clone());
        }
        if !changed.is_empty() {
            let _ = self.refresh();
        }
        Ok(changed)
    }

    /// Points `entry: old` in runbooks `sources` at `new`, changing nothing else in them
    /// (not even `updated`). Returns the runbooks that changed.
    pub fn retarget_runbook_steps(
        &mut self,
        sources: &[String],
        old: &str,
        new: &str,
    ) -> Result<Vec<String>, FsError> {
        let mut changed = Vec::new();
        for id in sources {
            let path = self.id_path(id)?;
            let bytes = read_entry(&path, id)?;
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let Some(rewritten) = crate::runbook::retarget_runbook_step_references(&text, old, new)
            else {
                continue;
            };
            let tmp = temp_path(&path);
            write_temp(&tmp, rewritten.as_bytes()).map_err(|e| io_err(&tmp, &e))?;
            if let Err(e) = std::fs::rename(&tmp, &path) {
                let _ = std::fs::remove_file(&tmp);
                return Err(io_err(&path, &e));
            }
            changed.push(id.clone());
        }
        if !changed.is_empty() {
            let _ = self.refresh();
        }
        Ok(changed)
    }

    /// Deletes an entry with `remover` (a plain remove, or `git rm`) and forgets its usage.
    pub fn remove_entry(
        &mut self,
        id: &str,
        remover: &dyn Fn(&Path) -> io::Result<()>,
    ) -> Result<(), FsError> {
        let path = self.id_path(id)?;
        if !path.is_file() {
            return Err(FsError::NotFound(id.to_string()));
        }
        remover(&path).map_err(|e| io_err(&path, &e))?;
        let (mut state, _) = State::load(self.root());
        if state.forget(id) {
            let _ = state.save(self.root());
        }
        remove_empty_dirs(self.root(), path.parent().expect("a folder"));
        let _ = self.refresh();
        Ok(())
    }
}

/// The entry in `dir` named `name` in any case, as a path. `dirs_only` limits it to folders.
fn find_ignoring_case(dir: &Path, name: &str, dirs_only: bool) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() && exact.is_dir() == dirs_only {
        return Some(exact);
    }
    let lower = name.to_lowercase();
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let matches = e.file_name().to_string_lossy().to_lowercase() == lower
            && e.path().is_dir() == dirs_only;
        matches.then(|| e.path())
    })
}

fn read_entry(path: &Path, id: &str) -> Result<Vec<u8>, FsError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(FsError::NotFound(id.to_string())),
        Err(e) => Err(io_err(path, &e)),
    }
}

/// `.name.md.kb-tmp-<pid>-<n>` next to `path`: same folder, so the rename or link stays on one
/// file system, and not an `.md` file, so nothing mistakes it for an entry.
fn temp_path(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(
        ".{name}.kb-tmp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn write_temp(tmp: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Writes `bytes` to `path` so that it appears whole and only if nothing was there: a temp file,
/// then a hard link (which fails if the target exists), then the temp file is removed. Where hard
/// links are not supported, an existence check and a rename.
fn create_new_file(path: &Path, bytes: &[u8], id: &str) -> Result<(), FsError> {
    let tmp = temp_path(path);
    write_temp(&tmp, bytes).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io_err(path, &e)
    })?;
    let linked = match std::fs::hard_link(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(FsError::Exists(id.to_string())),
        Err(_) if path.exists() => Err(FsError::Exists(id.to_string())),
        Err(_) => std::fs::rename(&tmp, path).map_err(|e| io_err(path, &e)),
    };
    let _ = std::fs::remove_file(&tmp);
    linked
}

/// Removes `dir` and its parents while they are empty, never `root` itself.
fn remove_empty_dirs(root: &Path, dir: &Path) {
    let mut dir = dir;
    while dir != root && dir.starts_with(root) {
        if std::fs::remove_dir(dir).is_err() {
            break;
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::author::FixedClock;
    use crate::author::draft::{Draft, DraftOptions, DraftSource};
    use crate::entry::EntryType;
    use crate::query::Query;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn clock() -> FixedClock {
        FixedClock(UNIX_EPOCH + Duration::from_secs(1_791_547_200))
    }

    fn entry_text(title: &str, tags: &str) -> String {
        format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: {title}\ntags: [{tags}]\ncreated: 2026-01-01\nupdated: 2026-01-01\n---\n\n```sh\nls\n```\n"
        )
    }

    fn kb_with(files: &[(&str, String)]) -> (tempfile::TempDir, Kb) {
        let dir = tempfile::tempdir().unwrap();
        for (id, text) in files {
            let p = dir.path().join(format!("{id}.md"));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let kb = Kb::open(dir.path()).unwrap();
        (dir, kb)
    }

    fn check_for(id: &str, body: &str) -> Check {
        let opts = DraftOptions {
            title: Some("Made by test".into()),
            ty: Some(EntryType::Sql),
            ..Default::default()
        };
        Draft::from_text(body, DraftSource::Stdin, &opts).check(id, &clock())
    }

    fn files_under(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
                }
            }
        }
        out.sort();
        out
    }

    fn search_ids(kb: &mut Kb) -> Vec<String> {
        kb.search(&Query::parse(""), SystemTime::now())
            .unwrap()
            .results
            .into_iter()
            .map(|r| r.id)
            .collect()
    }

    // ---- create ----

    #[test]
    fn create_writes_the_bytes_makes_folders_and_is_searchable() {
        let (dir, mut kb) = kb_with(&[("cmd/a", entry_text("A", "x"))]);
        let check = check_for("sql/deep/new", "select 1 from t;");
        let path = kb.create_entry(&check).unwrap();
        assert_eq!(path, kb.root().join("sql/deep/new.md"));
        assert_eq!(std::fs::read(&path).unwrap(), check.bytes);
        assert_eq!(search_ids(&mut kb), ["cmd/a", "sql/deep/new"]);
        assert_eq!(
            files_under(dir.path())
                .iter()
                .filter(|f| !f.starts_with(".kb"))
                .count(),
            2
        );
    }

    #[test]
    fn create_refuses_an_existing_path_in_any_case_and_touches_nothing() {
        let original = entry_text("A", "x");
        let (dir, mut kb) = kb_with(&[("cmd/a", original.clone())]);
        for id in ["cmd/a", "cmd/A", "CMD/a"] {
            let err = kb
                .create_entry(&check_for(id, "select 1 from t;"))
                .unwrap_err();
            assert!(matches!(err, FsError::Exists(_)), "{id}: {err}");
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("cmd/a.md")).unwrap(),
            original
        );
        assert!(
            files_under(dir.path())
                .iter()
                .all(|f| !f.contains("kb-tmp"))
        );
    }

    #[test]
    fn create_scans_for_secrets_itself() {
        let (dir, mut kb) = kb_with(&[]);
        let mut check = check_for("sql/x", "select 'AKIAIOSFODNN7EXAMPLE' from t;");
        check.findings.clear(); // a caller that forgot to look
        assert!(check.writable(false));
        let err = kb.create_entry(&check).unwrap_err();
        assert!(
            matches!(err, FsError::Secret(ref f) if !f.is_empty()),
            "{err}"
        );
        assert!(files_under(dir.path()).is_empty());
    }

    #[test]
    fn create_refuses_oversize_and_bad_ids() {
        let (dir, mut kb) = kb_with(&[]);
        let mut big = check_for("sql/x", "select 1 from t;");
        big.too_large = true;
        assert!(matches!(kb.create_entry(&big), Err(FsError::TooLarge)));
        for id in ["../x", "/abs", ".kb/state", "a//b", ""] {
            let err = kb
                .create_entry(&check_for(id, "select 1 from t;"))
                .unwrap_err();
            assert!(matches!(err, FsError::BadId(_)), "{id}");
        }
        assert!(files_under(dir.path()).is_empty());
    }

    #[test]
    fn create_leaves_no_temp_file_when_it_fails() {
        let (dir, mut kb) = kb_with(&[]);
        // `blocker` is a plain file, so `blocker/b` can't be created
        std::fs::write(dir.path().join("blocker"), "x").unwrap();
        let err = kb
            .create_entry(&check_for("blocker/b", "select 1 from t;"))
            .unwrap_err();
        assert!(matches!(err, FsError::Io { .. }), "{err}");
        assert_eq!(files_under(dir.path()), ["blocker"]);
    }

    #[test]
    fn id_taken_ignores_case() {
        let (_d, kb) = kb_with(&[("cmd/Foo", entry_text("A", "x"))]);
        assert!(kb.id_taken("cmd/Foo") && kb.id_taken("cmd/foo") && kb.id_taken("CMD/FOO"));
        assert!(!kb.id_taken("cmd/bar") && !kb.id_taken("sql/foo"));
    }

    // ---- rename and remove ----

    fn plain_move(from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    #[test]
    fn rename_moves_the_file_the_state_and_cleans_empty_folders() {
        let (dir, mut kb) = kb_with(&[
            ("old/deep/a", entry_text("A", "x")),
            ("keep/b", entry_text("B", "x")),
        ]);
        kb.record_use("old/deep/a", SystemTime::now()).unwrap();
        kb.record_resolved_use(
            "old/deep/a",
            SystemTime::now(),
            &[("env".into(), "prod".into())],
        )
        .unwrap();
        let r = kb
            .rename_entry("old/deep/a", "new/place/a", &plain_move)
            .unwrap();
        assert_eq!(
            (r.from.as_str(), r.to.as_str(), r.carried),
            ("old/deep/a", "new/place/a", true)
        );
        assert!(dir.path().join("new/place/a.md").is_file());
        assert!(
            !dir.path().join("old").exists(),
            "emptied folders are removed"
        );
        assert!(dir.path().join("keep/b.md").is_file());
        assert!(dir.path().is_dir() && dir.path().join(".kb").is_dir());
        assert_eq!(kb.usage("new/place/a").unwrap().count, 2);
        assert!(kb.usage("old/deep/a").is_none());
        assert_eq!(kb.remembered("new/place/a").0["env"], "prod");
        let mut ids = search_ids(&mut kb);
        ids.sort();
        assert_eq!(ids, ["keep/b", "new/place/a"]);
    }

    #[test]
    fn rename_carries_state_even_when_the_content_was_edited() {
        let (dir, mut kb) = kb_with(&[("cmd/a", entry_text("A", "x"))]);
        kb.record_use("cmd/a", SystemTime::now()).unwrap();
        std::fs::write(dir.path().join("cmd/a.md"), entry_text("A edited", "x")).unwrap();
        kb.rename_entry("cmd/a", "cmd/b", &plain_move).unwrap();
        assert_eq!(kb.usage("cmd/b").unwrap().count, 1);
        search_ids(&mut kb);
        assert_eq!(kb.usage("cmd/b").unwrap().count, 1);
    }

    #[test]
    fn rename_refuses_an_existing_target_and_unknown_or_bad_ids() {
        let (dir, mut kb) = kb_with(&[
            ("cmd/a", entry_text("A", "x")),
            ("cmd/b", entry_text("B", "x")),
        ]);
        for to in ["cmd/b", "cmd/B", "cmd/a"] {
            assert!(
                matches!(
                    kb.rename_entry("cmd/a", to, &plain_move),
                    Err(FsError::Exists(_))
                ),
                "{to}"
            );
        }
        assert!(matches!(
            kb.rename_entry("cmd/zzz", "cmd/c", &plain_move),
            Err(FsError::NotFound(_))
        ));
        assert!(matches!(
            kb.rename_entry("cmd/a", "../c", &plain_move),
            Err(FsError::BadId(_))
        ));
        assert_eq!(
            files_under(dir.path())
                .iter()
                .filter(|f| !f.starts_with(".kb"))
                .count(),
            2
        );
    }

    #[test]
    fn a_failing_mover_changes_nothing() {
        let (dir, mut kb) = kb_with(&[("cmd/a", entry_text("A", "x"))]);
        kb.record_use("cmd/a", SystemTime::now()).unwrap();
        let failing = |_: &Path, _: &Path| -> io::Result<()> { Err(io::Error::other("nope")) };
        let err = kb
            .rename_entry("cmd/a", "other/dir/a", &failing)
            .unwrap_err();
        assert!(matches!(err, FsError::Io { .. }));
        assert!(dir.path().join("cmd/a.md").is_file());
        assert!(
            !dir.path().join("other").exists(),
            "folders made for the move are removed again"
        );
        assert_eq!(kb.usage("cmd/a").unwrap().count, 1);
    }

    #[test]
    fn a_case_only_rename_is_allowed() {
        let (dir, mut kb) = kb_with(&[("cmd/foo", entry_text("A", "x"))]);
        kb.rename_entry("cmd/foo", "cmd/Foo", &plain_move).unwrap();
        assert!(files_under(dir.path()).contains(&"cmd/Foo.md".to_string()));
    }

    #[test]
    fn remove_deletes_forgets_and_cleans_folders() {
        let (dir, mut kb) = kb_with(&[
            ("solo/a", entry_text("A", "x")),
            ("cmd/b", entry_text("B", "x")),
        ]);
        kb.record_use("solo/a", SystemTime::now()).unwrap();
        kb.record_resolved_use(
            "solo/a",
            SystemTime::now(),
            &[("env".into(), "prod".into())],
        )
        .unwrap();
        let remover = |p: &Path| std::fs::remove_file(p);
        kb.remove_entry("solo/a", &remover).unwrap();
        assert!(!dir.path().join("solo").exists());
        assert!(kb.usage("solo/a").is_none() && kb.remembered("solo/a").0.is_empty());
        assert_eq!(search_ids(&mut kb), ["cmd/b"]);
        assert!(matches!(
            kb.remove_entry("solo/a", &remover),
            Err(FsError::NotFound(_))
        ));
        let failing = |_: &Path| -> io::Result<()> { Err(io::Error::other("no")) };
        kb.record_use("cmd/b", SystemTime::now()).unwrap();
        assert!(kb.remove_entry("cmd/b", &failing).is_err());
        assert_eq!(kb.usage("cmd/b").unwrap().count, 1);
    }

    // ---- edits ----

    #[test]
    fn save_edit_bumps_updated_only_and_detects_conflicts() {
        let (dir, mut kb) = kb_with(&[("cmd/a", entry_text("A", "x"))]);
        let before = std::fs::read_to_string(dir.path().join("cmd/a.md")).unwrap();
        let v = kb.version("cmd/a").unwrap();
        let saved = kb.save_edit("cmd/a", &v, &clock()).unwrap();
        assert_eq!(saved.updated, "2026-10-09T12:00:00Z");
        let after = std::fs::read_to_string(dir.path().join("cmd/a.md")).unwrap();
        assert_eq!(
            after,
            before.replace("updated: 2026-01-01", "updated: 2026-10-09T12:00:00Z")
        );
        // the file changes behind our back
        let theirs = entry_text("Someone else", "x");
        std::fs::write(dir.path().join("cmd/a.md"), &theirs).unwrap();
        let err = kb.save_edit("cmd/a", &v, &clock()).unwrap_err();
        assert!(matches!(err, FsError::Conflict(_)), "{err}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("cmd/a.md")).unwrap(),
            theirs
        );
        assert!(
            files_under(dir.path())
                .iter()
                .all(|f| !f.contains("kb-tmp"))
        );
    }

    #[test]
    fn save_edit_inserts_a_missing_updated_and_refuses_secrets() {
        let text = "---\nkb_format: 1\ntype: cmd\ntitle: A\n---\n\n```sh\nls\n```\n";
        let (dir, mut kb) = kb_with(&[("cmd/a", text.into())]);
        let v = kb.version("cmd/a").unwrap();
        kb.save_edit("cmd/a", &v, &clock()).unwrap();
        let after = std::fs::read_to_string(dir.path().join("cmd/a.md")).unwrap();
        assert!(
            after.contains("title: A\nupdated: 2026-10-09T12:00:00Z\n---\n"),
            "{after}"
        );
        let leaky = text.replace("ls", "export K=AKIAIOSFODNN7EXAMPLE");
        std::fs::write(dir.path().join("cmd/a.md"), &leaky).unwrap();
        let v = kb.version("cmd/a").unwrap();
        assert!(matches!(
            kb.save_edit("cmd/a", &v, &clock()),
            Err(FsError::Secret(_))
        ));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("cmd/a.md")).unwrap(),
            leaky
        );
    }

    #[test]
    fn restore_puts_the_snapshot_back_atomically() {
        let original = entry_text("A", "x");
        let (dir, mut kb) = kb_with(&[("cmd/a", original.clone())]);
        std::fs::write(dir.path().join("cmd/a.md"), "garbage").unwrap();
        kb.restore("cmd/a", original.as_bytes()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("cmd/a.md")).unwrap(),
            original
        );
        std::fs::remove_file(dir.path().join("cmd/a.md")).unwrap();
        kb.restore("cmd/a", original.as_bytes()).unwrap();
        assert!(dir.path().join("cmd/a.md").is_file());
        assert!(
            files_under(dir.path())
                .iter()
                .all(|f| !f.contains("kb-tmp"))
        );
    }

    #[test]
    fn version_changes_with_the_content() {
        let (dir, kb) = kb_with(&[("cmd/a", entry_text("A", "x"))]);
        let v1 = kb.version("cmd/a").unwrap();
        assert_eq!(kb.version("cmd/a").unwrap(), v1);
        std::fs::write(dir.path().join("cmd/a.md"), entry_text("B", "x")).unwrap();
        assert_ne!(kb.version("cmd/a").unwrap().hash, v1.hash);
        assert!(matches!(kb.version("cmd/nope"), Err(FsError::NotFound(_))));
    }

    // ---- tags ----

    #[test]
    fn tag_counts_order_by_count_then_name_from_files_or_index() {
        let (_d, mut kb) = kb_with(&[
            ("cmd/a", entry_text("A", "lambda, auth")),
            ("cmd/b", entry_text("B", "lambda, zeta")),
            ("cmd/c", entry_text("C", "auth, lambda, beta")),
        ]);
        let want = vec![
            ("lambda".to_string(), 3),
            ("auth".into(), 2),
            ("beta".into(), 1),
            ("zeta".into(), 1),
        ];
        assert_eq!(kb.tag_counts(), want, "scanned from the files");
        kb.refresh().unwrap();
        assert_eq!(kb.tag_counts(), want, "read from the index");
    }

    #[test]
    fn content_hash_is_what_version_uses() {
        let text = entry_text("A", "x");
        let (_d, kb) = kb_with(&[("cmd/a", text.clone())]);
        assert_eq!(
            kb.version("cmd/a").unwrap().hash,
            content_hash(text.as_bytes())
        );
    }
}
