//! Usage statistics in `.kb/state.json`: per-entry count, last-used time and content hash.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use crate::index::ensure_kb_dir;
use crate::search::Notice;

/// The `state.json` format version this build reads and writes.
pub const STATE_VERSION: u64 = 1;

/// FNV-1a 64-bit hash of a file's bytes. Stable across Rust versions and platforms, because the
/// value is stored on disk. It follows renames; it is not a security hash.
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Usage of one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    pub count: u64,
    /// Unix seconds, UTC.
    pub last_used: i64,
    /// [`content_hash`] of the entry file, refreshed when the file changes.
    pub hash: u64,
}

/// Seconds since the Unix epoch (negative before it).
pub fn unix_secs(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// How much an entry's use should raise its ranking, in `[0, 1)`: saturating in how often it was
/// used, halving with every 30 days since the last use.
pub fn frecency(record: Option<&UsageRecord>, now: SystemTime) -> f64 {
    let Some(r) = record else { return 0.0 };
    let age_days = ((unix_secs(now) - r.last_used).max(0) as f64) / 86_400.0;
    (1.0 - (-(r.count as f64) / 5.0).exp()) * 0.5_f64.powf(age_days / 30.0)
}

/// How many recent values are kept per variable name.
pub const RECENT_VALUES: usize = 20;

/// Remembered variable values (spec 006): the last value used per entry, and recent values per
/// variable name across the KB. Never holds a `secret` variable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VarMemory {
    pub entries: BTreeMap<String, BTreeMap<String, String>>,
    pub recent: BTreeMap<String, Vec<String>>,
}

impl VarMemory {
    /// Remembers `values` (name, value) as used in `id`. A value that looks like a credential is
    /// not stored; its name is returned.
    pub fn remember(&mut self, id: &str, values: &[(String, String)]) -> Vec<String> {
        let mut skipped = Vec::new();
        for (name, value) in values {
            if !crate::lint::secrets::scan_secrets(value).hits.is_empty() {
                skipped.push(name.clone());
                continue;
            }
            self.entries
                .entry(id.to_string())
                .or_default()
                .insert(name.clone(), value.clone());
            let recent = self.recent.entry(name.clone()).or_default();
            recent.retain(|v| v != value);
            recent.insert(0, value.clone());
            recent.truncate(RECENT_VALUES);
        }
        skipped
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.recent.is_empty()
    }

    fn to_json(&self) -> Value {
        serde_json::json!({ "entries": self.entries, "recent": self.recent })
    }

    /// Reads what it can; anything malformed is dropped rather than resetting the state.
    fn from_json(v: Value) -> VarMemory {
        let strings = |v: &Value| -> BTreeMap<String, String> {
            v.as_object()
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                        .collect()
                })
                .unwrap_or_default()
        };
        let entries = v
            .get("entries")
            .and_then(Value::as_object)
            .map(|o| o.iter().map(|(id, m)| (id.clone(), strings(m))).collect())
            .unwrap_or_default();
        let recent = v
            .get("recent")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(name, list)| {
                        let list = list
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(str::to_string))
                                    .take(RECENT_VALUES)
                                    .collect()
                            })
                            .unwrap_or_default();
                        (name.clone(), list)
                    })
                    .collect()
            })
            .unwrap_or_default();
        VarMemory { entries, recent }
    }
}

/// The contents of `state.json`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub usage: BTreeMap<String, UsageRecord>,
    pub vars: VarMemory,
    /// Top-level keys this version doesn't know; kept on write for later milestones.
    extra: Map<String, Value>,
}

fn path(root: &Path) -> PathBuf {
    root.join(".kb").join("state.json")
}

impl State {
    /// Reads the state. A missing file is empty. An unreadable, invalid or newer file is copied to
    /// `state.json.bak` and replaced in memory by an empty state, with a notice.
    pub fn load(root: &Path) -> (State, Option<Notice>) {
        let p = path(root);
        let Ok(text) = std::fs::read(&p) else {
            return (State::default(), None);
        };
        match parse(&text) {
            Some(state) => (state, None),
            None => {
                // Warn once: when the backup already holds these bytes, an earlier run reported it.
                let bak = p.with_file_name("state.json.bak");
                if std::fs::read(&bak).is_ok_and(|b| b == text) {
                    return (State::default(), None);
                }
                let _ = std::fs::copy(&p, &bak);
                (State::default(), Some(Notice::StateReset))
            }
        }
    }

    /// Writes the state atomically (temp file in `.kb/`, then rename).
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let dir = ensure_kb_dir(root)?;
        let mut obj = self.extra.clone();
        obj.insert("state_version".into(), Value::from(STATE_VERSION));
        let mut usage = Map::new();
        for (id, r) in &self.usage {
            usage.insert(
                id.clone(),
                serde_json::json!({
                    "count": r.count,
                    "last_used": r.last_used,
                    "hash": format!("{:016x}", r.hash),
                }),
            );
        }
        obj.insert("usage".into(), Value::Object(usage));
        if !self.vars.is_empty() {
            obj.insert("vars".into(), self.vars.to_json());
        }
        let mut text = serde_json::to_string_pretty(&Value::Object(obj)).unwrap_or_default();
        text.push('\n');
        let tmp = dir.join(format!("state.json.tmp-{}", std::process::id()));
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, dir.join("state.json")).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }

    /// Counts one use of `id` whose file currently hashes to `hash`.
    pub fn record(&mut self, id: &str, hash: u64, now: SystemTime) {
        let r = self.usage.entry(id.to_string()).or_insert(UsageRecord {
            count: 0,
            last_used: 0,
            hash,
        });
        r.count += 1;
        r.last_used = unix_secs(now);
        r.hash = hash;
    }

    /// Updates the stored hash of `id` after its file changed. Returns whether anything changed.
    pub fn refresh_hash(&mut self, id: &str, hash: u64) -> bool {
        match self.usage.get_mut(id) {
            Some(r) if r.hash != hash => {
                r.hash = hash;
                true
            }
            _ => false,
        }
    }

    /// Moves records of IDs that no longer exist (`missing`) to new IDs (`added`: ID and hash)
    /// with the same content. Ties go to the first ID in sorted order. Returns whether any moved.
    pub fn carry_renames(
        &mut self,
        missing: &BTreeSet<String>,
        added: &BTreeMap<String, u64>,
    ) -> bool {
        let mut claimed = BTreeSet::new();
        let mut moves = Vec::new();
        for old in missing {
            let Some(rec) = self.usage.get(old) else {
                continue;
            };
            let target = added
                .iter()
                .find(|(id, h)| **h == rec.hash && !claimed.contains(*id))
                .map(|(id, _)| id.clone());
            if let Some(new) = target {
                claimed.insert(new.clone());
                moves.push((old.clone(), new));
            }
        }
        for (old, new) in &moves {
            if let Some(mem) = self.vars.entries.remove(old) {
                self.vars.entries.entry(new.clone()).or_insert(mem);
            }
            if let Some(rec) = self.usage.remove(old) {
                self.usage.entry(new.clone()).or_insert(rec);
            }
        }
        !moves.is_empty()
    }

    /// Moves the usage record and the remembered values of `old` to `new`, whatever the content
    /// (`kb mv`, FR-022). A stale record at `new` is overwritten. Returns whether anything moved.
    pub fn move_entry(&mut self, old: &str, new: &str) -> bool {
        let mut moved = false;
        if let Some(rec) = self.usage.remove(old) {
            self.usage.insert(new.to_string(), rec);
            moved = true;
        }
        if let Some(mem) = self.vars.entries.remove(old) {
            self.vars.entries.insert(new.to_string(), mem);
            moved = true;
        }
        moved
    }

    /// Drops the usage record and remembered values of `id` (`kb rm`, FR-023). Returns whether
    /// anything was removed.
    pub fn forget(&mut self, id: &str) -> bool {
        let usage = self.usage.remove(id).is_some();
        let vars = self.vars.entries.remove(id).is_some();
        usage || vars
    }

    /// Drops records whose ID is not in `existing` (done at a full rebuild).
    pub fn prune(&mut self, existing: &BTreeSet<String>) -> bool {
        let before = self.usage.len();
        self.usage.retain(|id, _| existing.contains(id));
        let before_vars = self.vars.entries.len();
        self.vars.entries.retain(|id, _| existing.contains(id));
        self.usage.len() != before || self.vars.entries.len() != before_vars
    }

    /// Whether `id` is recorded as an unreviewed agent change.
    pub fn is_unreviewed(&self, id: &str) -> bool {
        if let Some(Value::Object(map)) = self.extra.get("unreviewed")
            && let Some(Value::Bool(b)) = map.get(id)
        {
            return *b;
        }
        false
    }

    /// Count of entries marked unreviewed.
    pub fn unreviewed_count(&self) -> usize {
        if let Some(Value::Object(map)) = self.extra.get("unreviewed") {
            return map.values().filter(|v| v.as_bool() == Some(true)).count();
        }
        0
    }
}

fn parse(text: &[u8]) -> Option<State> {
    let Value::Object(mut obj) = serde_json::from_slice(text).ok()? else {
        return None;
    };
    let version = obj.remove("state_version")?.as_u64()?;
    if version == 0 || version > STATE_VERSION {
        return None;
    }
    let vars = obj
        .remove("vars")
        .map(VarMemory::from_json)
        .unwrap_or_default();
    let Value::Object(raw) = obj.remove("usage")? else {
        return None;
    };
    let mut usage = BTreeMap::new();
    for (id, v) in raw {
        let count = v.get("count")?.as_u64()?;
        let last_used = v.get("last_used")?.as_i64()?;
        let hash = u64::from_str_radix(v.get("hash")?.as_str()?, 16).ok()?;
        usage.insert(
            id,
            UsageRecord {
                count,
                last_used,
                hash,
            },
        );
    }
    Some(State {
        usage,
        vars,
        extra: obj,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn hash_matches_fnv1a_vectors() {
        assert_eq!(content_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(content_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn missing_file_is_empty_without_notice() {
        let d = tempfile::tempdir().unwrap();
        let (s, n) = State::load(d.path());
        assert!(s.usage.is_empty() && n.is_none());
    }

    #[test]
    fn invalid_states_reset_with_backup_and_one_notice() {
        for bad in [
            "{",
            "[]",
            r#"{"state_version":2,"usage":{}}"#,
            r#"{"state_version":1,"usage":{"a":{"count":"x"}}}"#,
            r#"{"usage":{}}"#,
        ] {
            let d = tempfile::tempdir().unwrap();
            std::fs::create_dir(d.path().join(".kb")).unwrap();
            std::fs::write(d.path().join(".kb/state.json"), bad).unwrap();
            let (s, n) = State::load(d.path());
            assert!(s.usage.is_empty(), "{bad}");
            assert_eq!(n, Some(Notice::StateReset), "{bad}");
            let bak = std::fs::read_to_string(d.path().join(".kb/state.json.bak")).unwrap();
            assert_eq!(bak, bad);
            // the second load is silent: it was already reported
            let (s, n) = State::load(d.path());
            assert!(s.usage.is_empty() && n.is_none(), "{bad}");
        }
    }

    #[test]
    fn round_trip_keeps_unknown_keys_and_leaves_no_temp_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join(".kb")).unwrap();
        std::fs::write(
            d.path().join(".kb/state.json"),
            r#"{"state_version":1,"usage":{},"future":{"region":"eu"}}"#,
        )
        .unwrap();
        let (mut s, n) = State::load(d.path());
        assert!(n.is_none());
        s.record("a/b", 7, at(1000));
        s.save(d.path()).unwrap();
        let (s2, _) = State::load(d.path());
        assert_eq!(s2.usage["a/b"].count, 1);
        assert_eq!(s2.extra["future"]["region"], "eu");
        let names: Vec<_> = std::fs::read_dir(d.path().join(".kb"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.contains("tmp")), "{names:?}");
    }

    #[test]
    fn frecency_is_bounded_monotonic_and_decays() {
        let now = at(10_000_000);
        let rec = |count, last_used| UsageRecord {
            count,
            last_used,
            hash: 0,
        };
        assert_eq!(frecency(None, now), 0.0);
        let f = |c| frecency(Some(&rec(c, 10_000_000)), now);
        for c in [1, 2, 10, 1000, 1_000_000] {
            assert!((0.0..1.0 + 1e-12).contains(&f(c)));
        }
        assert!(f(1) < f(2) && f(2) < f(10) && f(10) <= f(1000));
        let old = frecency(Some(&rec(5, 10_000_000 - 30 * 86_400)), now);
        assert!((old - f(5) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn renames_carry_over_deterministically() {
        let mut s = State::default();
        s.usage.insert(
            "a/old".into(),
            UsageRecord {
                count: 3,
                last_used: 9,
                hash: 42,
            },
        );
        s.usage.insert(
            "a/old2".into(),
            UsageRecord {
                count: 1,
                last_used: 1,
                hash: 42,
            },
        );
        let missing: BTreeSet<String> = ["a/old".into(), "a/old2".into()].into();
        let added: BTreeMap<String, u64> = [("a/new".to_string(), 42)].into();
        assert!(s.carry_renames(&missing, &added));
        assert_eq!(s.usage["a/new"].count, 3);
        assert!(!s.usage.contains_key("a/old"));
        assert!(s.usage.contains_key("a/old2"));
        // different content does not carry
        let mut s = State::default();
        s.record("x", 1, at(5));
        let added: BTreeMap<String, u64> = [("y".to_string(), 2)].into();
        assert!(!s.carry_renames(&["x".to_string()].into(), &added));
    }

    #[test]
    fn prune_and_refresh() {
        let mut s = State::default();
        s.record("keep", 1, at(5));
        s.record("gone", 2, at(5));
        assert!(s.refresh_hash("keep", 9) && !s.refresh_hash("keep", 9));
        assert!(s.prune(&["keep".to_string()].into()));
        assert_eq!(s.usage.keys().collect::<Vec<_>>(), vec!["keep"]);
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn memory_round_trips_and_keeps_usage() {
        let d = tempfile::tempdir().unwrap();
        let mut s = State::default();
        s.record("a", 1, at(5));
        assert!(
            s.vars
                .remember("a", &pairs(&[("env", "prod"), ("n", "30")]))
                .is_empty()
        );
        s.save(d.path()).unwrap();
        let (s2, n) = State::load(d.path());
        assert!(n.is_none());
        assert_eq!(s2.vars, s.vars);
        assert_eq!(s2.vars.entries["a"]["env"], "prod");
        assert_eq!(s2.vars.recent["n"], vec!["30"]);
        assert_eq!(s2.usage["a"].count, 1);
        assert_eq!(STATE_VERSION, 1);
    }

    #[test]
    fn no_memory_means_no_vars_key() {
        let d = tempfile::tempdir().unwrap();
        let mut s = State::default();
        s.record("a", 1, at(5));
        s.save(d.path()).unwrap();
        let text = std::fs::read_to_string(d.path().join(".kb/state.json")).unwrap();
        assert!(!text.contains("vars"), "{text}");
    }

    #[test]
    fn malformed_memory_is_dropped_not_fatal() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join(".kb")).unwrap();
        for bad in [
            r#"{"state_version":1,"usage":{},"vars":"x"}"#,
            r#"{"state_version":1,"usage":{},"vars":{"entries":[],"recent":5}}"#,
            r#"{"state_version":1,"usage":{},"vars":{"entries":{"a":{"n":3}},"recent":{"n":[1,"ok"]}}}"#,
        ] {
            std::fs::write(d.path().join(".kb/state.json"), bad).unwrap();
            let (s, n) = State::load(d.path());
            assert!(n.is_none(), "{bad}");
            assert!(s.vars.entries.values().all(|m| m.is_empty()), "{bad}");
        }
        let (s, _) = State::load(d.path());
        assert_eq!(s.vars.recent["n"], vec!["ok"]);
    }

    #[test]
    fn recent_values_are_most_recent_first_unique_and_capped() {
        let mut m = VarMemory::default();
        for i in 0..25 {
            m.remember("a", &pairs(&[("n", &i.to_string())]));
        }
        assert_eq!(m.recent["n"].len(), RECENT_VALUES);
        assert_eq!(m.recent["n"][0], "24");
        assert!(!m.recent["n"].contains(&"4".to_string()));
        m.remember("b", &pairs(&[("n", "10")]));
        assert_eq!(m.recent["n"][0], "10");
        assert_eq!(m.recent["n"].iter().filter(|v| *v == "10").count(), 1);
        assert_eq!(m.entries["a"]["n"], "24");
        assert_eq!(m.entries["b"]["n"], "10");
    }

    #[test]
    fn credential_looking_values_are_not_remembered() {
        let mut m = VarMemory::default();
        let skipped = m.remember(
            "a",
            &pairs(&[("k", "AKIAIOSFODNN7EXAMPLE"), ("env", "prod")]),
        );
        assert_eq!(skipped, vec!["k"]);
        assert!(!m.entries["a"].contains_key("k") && m.entries["a"]["env"] == "prod");
        assert!(!m.recent.contains_key("k"));
    }

    #[test]
    fn memory_follows_renames_and_is_pruned() {
        let mut s = State::default();
        s.record("old", 42, at(5));
        s.vars.remember("old", &pairs(&[("env", "prod")]));
        let added: BTreeMap<String, u64> = [("new".to_string(), 42)].into();
        assert!(s.carry_renames(&["old".to_string()].into(), &added));
        assert_eq!(s.vars.entries["new"]["env"], "prod");
        assert!(!s.vars.entries.contains_key("old"));
        s.vars.remember("gone", &pairs(&[("env", "dev")]));
        assert!(s.prune(&["new".to_string()].into()));
        assert!(!s.vars.entries.contains_key("gone"));
        assert!(s.vars.recent.contains_key("env"));
    }

    fn state_with(id: &str) -> State {
        let mut s = State::default();
        s.record(id, 7, at(100));
        s.record(id, 7, at(200));
        s.vars.remember(id, &pairs(&[("env", "prod")]));
        s
    }

    #[test]
    fn move_entry_carries_usage_and_values() {
        let mut s = state_with("cw/a");
        s.record("cw/b", 1, at(5));
        s.vars.remember("cw/b", &pairs(&[("env", "stale")]));
        assert!(s.move_entry("cw/a", "cw/b"));
        assert!(!s.usage.contains_key("cw/a") && !s.vars.entries.contains_key("cw/a"));
        assert_eq!(s.usage["cw/b"].count, 2);
        assert_eq!(s.usage["cw/b"].last_used, 200);
        assert_eq!(s.vars.entries["cw/b"]["env"], "prod");
        // recent values are keyed by variable name and stay
        assert_eq!(s.vars.recent["env"][0], "stale");
    }

    #[test]
    fn move_entry_of_an_unknown_id_moves_nothing() {
        let mut s = state_with("cw/a");
        assert!(!s.move_entry("cw/zzz", "cw/b"));
        assert!(s.usage.contains_key("cw/a"));
    }

    #[test]
    fn forget_drops_both_records() {
        let mut s = state_with("cw/a");
        assert!(s.forget("cw/a"));
        assert!(s.usage.is_empty() && s.vars.entries.is_empty());
        assert!(!s.forget("cw/a"));
    }

    #[test]
    fn moves_keep_unknown_keys_and_the_version() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".kb")).unwrap();
        std::fs::write(
            d.path().join(".kb/state.json"),
            r#"{"state_version":1,"usage":{"a":{"count":1,"last_used":5,"hash":"0000000000000001"}},"future":{"x":1}}"#,
        )
        .unwrap();
        let (mut s, _) = State::load(d.path());
        assert!(s.move_entry("a", "b"));
        s.save(d.path()).unwrap();
        let text = std::fs::read_to_string(d.path().join(".kb/state.json")).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["state_version"], STATE_VERSION);
        assert_eq!(STATE_VERSION, 1);
        assert_eq!(v["future"]["x"], 1);
        assert!(v["usage"]["b"].is_object() && v["usage"]["a"].is_null());
    }
}
