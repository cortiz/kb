#![allow(clippy::single_range_in_vec_init)]
use super::*;
use std::time::UNIX_EPOCH;

const NOW_SECS: u64 = 1_790_000_000;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOW_SECS)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn fixture() -> (tempfile::TempDir, Kb) {
    let d = tempfile::tempdir().unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/search");
    copy_dir(&src, d.path());
    let kb = Kb::open(d.path()).unwrap();
    (d, kb)
}

fn entry(title: &str, tags: &str, body: &str, notes: &str) -> String {
    format!(
        "---\nkb_format: 1\ntype: prompt\ntitle: {title}\ntags: [{tags}]\n---\n\n```\n{body}\n```\n\n{notes}\n"
    )
}

fn kb_of(files: &[(&str, String)]) -> (tempfile::TempDir, Kb) {
    let d = tempfile::tempdir().unwrap();
    for (id, text) in files {
        let p = d.path().join(format!("{id}.md"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let kb = Kb::open(d.path()).unwrap();
    (d, kb)
}

fn run(kb: &mut Kb, s: &str) -> SearchOutcome {
    kb.search(&Query::parse(s), now()).unwrap()
}

fn ids(o: &SearchOutcome) -> Vec<&str> {
    o.results.iter().map(|r| r.id.as_str()).collect()
}

// ---- open / entry ----

#[test]
fn open_checks_the_root_and_touches_nothing() {
    assert!(matches!(
        Kb::open(Path::new("/definitely/not/here")),
        Err(KbError::Io { .. })
    ));
    let d = tempfile::tempdir().unwrap();
    Kb::open(d.path()).unwrap();
    assert!(!d.path().join(".kb").exists());
}

#[test]
fn entry_lookup_needs_no_index() {
    let (d, kb) = fixture();
    let e = kb.entry("sumo/5xx-by-authorizer").unwrap();
    assert_eq!(e.title(), Some("5xx by authorizer"));
    assert!(!d.path().join(".kb").exists());
    assert!(matches!(kb.entry("nope/x"), Err(KbError::NotFound(_))));
    for bad in ["../x", "/etc/passwd", "a\\b", "", "a//b", "./a"] {
        assert!(matches!(kb.entry(bad), Err(KbError::BadId(_))), "{bad:?}");
    }
}

// ---- ranking (US1) ----

#[test]
fn headline_query_finds_the_authorizer_first() {
    let (_d, mut kb) = fixture();
    let o = run(&mut kb, "5xx auth");
    assert_eq!(o.results[0].id, "sumo/5xx-by-authorizer");
    assert!(!o.kb_empty);
}

#[test]
fn all_terms_must_match() {
    let (_d, mut kb) = fixture();
    let o = run(&mut kb, "5xx zzzznotaword");
    assert!(o.results.is_empty());
    let o = run(&mut kb, "5xx host");
    assert_eq!(ids(&o), vec!["sumo/5xx-by-host"]);
}

#[test]
fn title_hit_outranks_notes_hit() {
    let (_d, mut kb) = kb_of(&[
        ("a/notes", entry("Plain", "", "x", "zebra zebra zebra")),
        ("a/title", entry("Zebra", "", "x", "nothing")),
    ]);
    let o = run(&mut kb, "zebra");
    assert_eq!(o.results[0].id, "a/title");
}

#[test]
fn exact_title_beats_usage_and_term_frequency() {
    let (_d, mut kb) = kb_of(&[
        ("a/exact", entry("Zebra", "", "x", "y")),
        (
            "a/busy",
            entry(
                "Zebra crossing zebra zebra",
                "zebra",
                "zebra zebra",
                "zebra",
            ),
        ),
    ]);
    for _ in 0..500 {
        kb.record_use("a/busy", now()).unwrap();
    }
    let o = run(&mut kb, "zebra");
    assert_eq!(o.results[0].id, "a/exact");
    assert_eq!(o.results[0].tier, Tier::TitleExact);
    let o = run(&mut kb, "ZEBRA");
    assert_eq!(o.results[0].id, "a/exact");
}

#[test]
fn frecency_lifts_equals_but_never_clear_winners() {
    let same = entry("Needle", "", "x", "y");
    let (_d, mut kb) = kb_of(&[("x/aaa", same.clone()), ("x/zzz", same)]);
    assert_eq!(ids(&run(&mut kb, "needle")), vec!["x/aaa", "x/zzz"]);
    kb.record_use("x/zzz", now()).unwrap();
    kb.record_use("x/zzz", now()).unwrap();
    assert_eq!(ids(&run(&mut kb, "needle")), vec!["x/zzz", "x/aaa"]);

    let (_d, mut kb) = kb_of(&[
        (
            "x/strong",
            entry("Needle needle haystack", "needle", "needle", ""),
        ),
        (
            "x/weak",
            entry("Haystack", "", "x", "a needle appears once here"),
        ),
    ]);
    for _ in 0..10_000 {
        kb.record_use("x/weak", now()).unwrap();
    }
    assert_eq!(ids(&run(&mut kb, "needle"))[0], "x/strong");
}

#[test]
fn ties_break_by_id_and_order_is_stable() {
    let same = entry("Same", "", "x", "y");
    let (_d, mut kb) = kb_of(&[("b", same.clone()), ("a", same.clone()), ("c", same)]);
    let first = ids(&run(&mut kb, "same"))
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    assert_eq!(first, vec!["a", "b", "c"]);
    for _ in 0..3 {
        let again: Vec<String> = ids(&run(&mut kb, "same"))
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(again, first);
    }
}

#[test]
fn empty_query_lists_everything_by_frecency_then_id() {
    let (_d, mut kb) = fixture();
    let o = run(&mut kb, "");
    assert_eq!(o.total, 32);
    assert_eq!(o.results.len(), 20);
    assert_eq!(o.results[0].id, "cmd/cache-role-flush");
    kb.record_use("sql/orders-by-status", now()).unwrap();
    kb.record_use("sql/orders-by-status", now()).unwrap();
    kb.record_use("link/oncall-handbook", now()).unwrap();
    let o = run(&mut kb, "the of");
    assert_eq!(
        &ids(&o)[..3],
        [
            "sql/orders-by-status",
            "link/oncall-handbook",
            "cmd/cache-role-flush"
        ]
    );
}

#[test]
fn limit_caps_and_zero_means_all() {
    let (_d, mut kb) = fixture();
    let mut q = Query::parse("");
    q.limit = 3;
    assert_eq!(kb.search(&q, now()).unwrap().results.len(), 3);
    q.limit = 0;
    assert_eq!(kb.search(&q, now()).unwrap().results.len(), 32);
}

#[test]
fn search_never_records_usage() {
    let (d, mut kb) = fixture();
    run(&mut kb, "auth");
    run(&mut kb, "");
    assert!(!d.path().join(".kb/state.json").exists());
}

#[test]
fn a_file_deleted_after_the_query_is_not_returned() {
    let (d, mut kb) = fixture();
    let victim = d.path().join("sumo/5xx-by-authorizer.md");
    kb.after_query = Some(Box::new(move || {
        let _ = std::fs::remove_file(&victim);
    }));
    let o = run(&mut kb, "5xx auth");
    assert!(
        !ids(&o).contains(&"sumo/5xx-by-authorizer"),
        "{:?}",
        ids(&o)
    );
}

#[test]
fn any_query_string_is_safe() {
    let (_d, mut kb) = fixture();
    for s in [
        "AND", "NEAR", "5xx ???", "foo\"bar", "\"", "*", "a:b:c", "OR OR", "title:x", "(", "-",
        "^x", "x*",
    ] {
        kb.search(&Query::parse(s), now())
            .unwrap_or_else(|e| panic!("{s:?}: {e}"));
    }
    let alphabet: Vec<char> = "abc5xx \"*():-^ANDORNEARé".chars().collect();
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..1000 {
        let mut s = String::new();
        for _ in 0..12 {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s.push(alphabet[(x >> 33) as usize % alphabet.len()]);
        }
        kb.search(&Query::parse(&s), now())
            .unwrap_or_else(|e| panic!("{s:?}: {e}"));
    }
}

// ---- highlight and snippets ----

#[test]
fn highlight_ranges_are_in_characters() {
    let (_d, mut kb) = fixture();
    let o = run(&mut kb, "5xx auth");
    let r = &o.results[0];
    let title = r.matches.iter().find(|m| m.field == Field::Title).unwrap();
    assert_eq!(title.ranges, vec![0..3, 7..17]);
    let tags = r.matches.iter().find(|m| m.field == Field::Tags).unwrap();
    assert_eq!(tags.ranges, vec![0..4]);
    let o = run(&mut kb, "configuracion");
    let r = &o.results[0];
    assert_eq!(r.id, "sql/configuracion-red");
    assert_eq!(r.matches[0].field, Field::Title);
    assert_eq!(r.matches[0].ranges, vec![0..13]);
    let s = r.snippet.as_ref().unwrap();
    assert_eq!(
        (s.field, s.text.as_str()),
        (Field::Title, "Configuración de red")
    );
}

#[test]
fn snippets_are_short_and_centered() {
    let long = format!(
        "{}needle{}",
        "lorem ipsum ".repeat(60),
        " dolor sit".repeat(60)
    );
    let (_d, mut kb) = kb_of(&[("a/long", entry("Plain", "", &long, ""))]);
    let o = run(&mut kb, "needle");
    let s = o.results[0].snippet.clone().unwrap();
    assert_eq!(s.field, Field::Payload);
    assert!(s.text.chars().count() <= 160, "{}", s.text.chars().count());
    assert!(s.text.starts_with('…') && s.text.ends_with('…'));
    let chars: Vec<char> = s.text.chars().collect();
    let r = &s.ranges[0];
    assert_eq!(chars[r.clone()].iter().collect::<String>(), "needle");
    assert!(run(&mut kb, "").results[0].snippet.is_none());
}

#[test]
fn a_file_over_one_megabyte_is_indexed_and_searchable() {
    let filler = "padding line of text\n".repeat(52_000);
    let (_d, mut kb) = kb_of(&[("a/big", entry("Big", "", &format!("{filler}zebrafish"), ""))]);
    let text = std::fs::read(kb.root().join("a/big.md")).unwrap();
    assert!(text.len() > 1_048_576);
    let o = run(&mut kb, "zebrafish");
    assert_eq!(ids(&o), vec!["a/big"]);
    let s = o.results[0].snippet.clone().unwrap();
    assert!(s.text.chars().count() <= 160 && s.text.contains("zebrafish"));
    let t = Instant::now();
    run(&mut kb, "zebrafish");
    assert!(
        t.elapsed() < Duration::from_millis(1500),
        "{:?}",
        t.elapsed()
    );
}

// ---- filters (US3) ----

#[test]
fn filters_narrow_results() {
    let (_d, mut kb) = fixture();
    let o = run(&mut kb, "latency type:cw");
    assert!(o.results.iter().all(|r| r.entry_type == EntryType::Cw));
    assert!(!o.results.is_empty());
    assert_eq!(
        ids(&run(&mut kb, "tag:prod tag:auth")),
        vec!["cw/authorizer-latency", "sumo/login-failures"]
    );
    let o = run(&mut kb, "latency -tag:stage");
    assert!(!ids(&o).contains(&"cw/latency-stage") && ids(&o).contains(&"cw/latency-prod"));
    let o = run(&mut kb, "\"role cache\"");
    assert_eq!(ids(&o), vec!["prompt/role-cache-notes"]);
    assert_eq!(
        ids(&run(&mut kb, "var:region")),
        vec!["cmd/deploy-service", "cmd/kubectl-pods"]
    );
    assert_eq!(
        ids(&run(&mut kb, "repo:acme/api")),
        vec!["code/request-handler", "code/retry-policy"]
    );
    assert_eq!(
        ids(&run(&mut kb, "repo:ACME/Gateway")),
        vec!["code/auth-middleware"]
    );
    let o = run(&mut kb, "type:sql type:link");
    assert!(
        o.results
            .iter()
            .all(|r| matches!(r.entry_type, EntryType::Sql | EntryType::Link))
    );
    assert!(run(&mut kb, "type:nonsense").results.is_empty());
}

#[test]
fn scope_restricts_free_text() {
    let (_d, mut kb) = fixture();
    let plain = run(&mut kb, "gateway");
    assert!(ids(&plain).contains(&"prompt/postmortem-template"));
    let t = run(&mut kb, "in:title gateway");
    assert_eq!(ids(&t), vec!["cw/api-gateway-5xx"]);
    let n = run(&mut kb, "in:notes gateway");
    assert!(ids(&n).contains(&"prompt/postmortem-template"));
    assert!(!ids(&n).contains(&"code/auth-middleware"));
}

#[test]
fn flags_give_the_same_results_as_syntax() {
    let (_d, mut kb) = fixture();
    let a = Query::from_parts("latency", &["cw".into()], &["prod".into()], &[], None).unwrap();
    let b = Query::parse("latency type:cw tag:prod");
    assert_eq!(
        ids(&kb.search(&a, now()).unwrap()),
        ids(&kb.search(&b, now()).unwrap())
    );
}

#[test]
fn unknown_filters_become_notices() {
    let (_d, mut kb) = fixture();
    let o = run(&mut kb, "foo:bar");
    assert!(
        o.notices
            .contains(&Notice::UnknownFilter { key: "foo".into() })
    );
}

// ---- typos and prefixes (US4) ----

#[test]
fn one_edit_helper() {
    assert!(within_one_edit("authorizer", "authroizer"));
    assert!(within_one_edit("authorizer", "authorize"));
    assert!(within_one_edit("authorizer", "authorizers"));
    assert!(within_one_edit("authorizer", "authorizar"));
    assert!(within_one_edit("authorizer", "xuthorizer"));
    assert!(within_one_edit("abcde", "abcde"));
    assert!(!within_one_edit("authorizer", "authrizar"));
    assert!(!within_one_edit("abcde", "abced_"));
    assert!(!within_one_edit("abcde", "edcba"));
}

#[test]
fn typos_match_below_exact() {
    let (_d, mut kb) = fixture();
    let o = run(&mut kb, "authroizer");
    assert!(!o.results.is_empty());
    assert!(o.results.iter().all(|r| r.tier == Tier::Typo));
    assert!(
        ids(&o).contains(&"sumo/5xx-by-authorizer") && ids(&o).contains(&"cw/authorizer-latency")
    );
    let o = run(&mut kb, "authori");
    assert!(o.results.iter().all(|r| r.tier == Tier::Exact));
    assert!(ids(&o).contains(&"sql/authorized-users"));
    // a short word is never fuzzy
    assert!(run(&mut kb, "auht").results.is_empty());
    // not inside phrases
    assert!(run(&mut kb, "\"authroizer latency\"").results.is_empty());
}

#[test]
fn exact_always_outranks_a_typo_only_entry() {
    let (_d, mut kb) = kb_of(&[
        (
            "a/typo",
            entry(
                "Authroizer authroizer authroizer",
                "authroizer",
                "authroizer",
                "authroizer",
            ),
        ),
        (
            "a/exact",
            entry("Plain", "", "x", "an authorizer is mentioned"),
        ),
    ]);
    for _ in 0..50 {
        kb.record_use("a/typo", now()).unwrap();
    }
    let mut q = Query::parse("authorizer");
    q.limit = 0;
    let o = kb.search(&q, now()).unwrap();
    assert_eq!(ids(&o), vec!["a/exact", "a/typo"]);
    assert_eq!(
        (o.results[0].tier, o.results[1].tier),
        (Tier::Exact, Tier::Typo)
    );
}

#[test]
fn accents_and_case_are_ignored_both_ways() {
    let (_d, mut kb) = fixture();
    for q in [
        "configuracion",
        "CONFIGURACION",
        "configuración",
        "Configuración",
    ] {
        assert_eq!(
            run(&mut kb, q).results[0].id,
            "sql/configuracion-red",
            "{q}"
        );
    }
}

#[test]
fn vocabulary_scan_runs_only_when_it_can_matter() {
    let (_d, mut kb) = fixture();
    let mut q = Query::parse("authorizer");
    q.limit = 1;
    kb.search(&q, now()).unwrap();
    assert_eq!(kb.vocab_scans.get(), 0, "an exact match fills the page");
    q.limit = 0;
    kb.search(&q, now()).unwrap();
    assert_eq!(kb.vocab_scans.get(), 1);
    q.limit = 5;
    kb.search(&Query::parse("authroizer"), now()).unwrap();
    assert_eq!(kb.vocab_scans.get(), 2);
}

#[test]
fn rust_folding_agrees_with_sqlite_tokenizer() {
    let (d, mut kb) = fixture();
    run(&mut kb, "x");
    let conn = Connection::open(d.path().join(".kb/index.db")).unwrap();
    let mut stmt = conn.prepare("SELECT term FROM fts_vocab").unwrap();
    let terms: HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut checked = 0;
    for e in std::fs::read_dir(d.path().join("sql")).unwrap() {
        let path = e.unwrap().path();
        let id = format!("sql/{}", path.file_stem().unwrap().to_string_lossy());
        let entry = kb.entry(&id).unwrap();
        let text = format!(
            "{} {} {}",
            entry.title().unwrap(),
            entry.payload().unwrap().text,
            entry.notes()
        );
        for w in words(&text) {
            assert!(
                terms.contains(&w.text),
                "{} is not a vocabulary term",
                w.text
            );
            checked += 1;
        }
    }
    assert!(checked > 50);
}

// ---- table (SC-004) ----

#[test]
fn query_table() {
    let (_d, mut kb) = fixture();
    let table = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/search/queries.tsv"),
    )
    .unwrap();
    let (mut rows, mut first, mut top3, mut failures) = (0, 0, 0, Vec::new());
    for line in table
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let cols: Vec<&str> = line.split('\t').collect();
        let (query, want, must) = (cols[0], cols[1], cols[2]);
        rows += 1;
        let o = run(&mut kb, query);
        let got = ids(&o);
        let ok_first = got.first() == Some(&want);
        let ok_top = must
            .split(',')
            .filter(|s| !s.is_empty())
            .all(|m| got.iter().take(3).any(|g| g == &m));
        first += usize::from(ok_first);
        top3 += usize::from(ok_top);
        if !ok_first || !ok_top {
            failures.push(format!(
                "{query:?}: want {want}, got {:?}",
                &got[..got.len().min(4)]
            ));
        }
    }
    assert!(rows >= 40, "{rows} rows");
    assert!(
        first * 10 >= rows * 9,
        "first {first}/{rows}: {failures:#?}"
    );
    assert_eq!(top3, rows, "top-3 {top3}/{rows}: {failures:#?}");
}

// ---- usage via Kb ----

#[test]
fn record_use_counts_and_validates() {
    let (d, kb) = fixture();
    assert!(kb.usage("sql/orders-by-status").is_none());
    kb.record_use("sql/orders-by-status", now()).unwrap();
    kb.record_use("sql/orders-by-status", now()).unwrap();
    let u = kb.usage("sql/orders-by-status").unwrap();
    assert_eq!((u.count, u.last_used), (2, NOW_SECS as i64));
    let before = std::fs::read(d.path().join(".kb/state.json")).unwrap();
    assert!(matches!(
        kb.record_use("nope/x", now()),
        Err(KbError::NotFound(_))
    ));
    assert!(matches!(
        kb.record_use("../x", now()),
        Err(KbError::BadId(_))
    ));
    assert_eq!(
        std::fs::read(d.path().join(".kb/state.json")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join(".kb/.gitignore")).unwrap(),
        "*\n"
    );
}

// ---- freshness (US2) ----

use std::cell::RefCell;
use std::rc::Rc;

struct Counting(Rc<RefCell<Vec<PathBuf>>>);

impl FileReader for Counting {
    fn read(&self, p: &Path) -> std::io::Result<Vec<u8>> {
        self.0.borrow_mut().push(p.to_path_buf());
        std::fs::read(p)
    }
}

fn backdate(root: &Path) {
    let old = SystemTime::now() - Duration::from_secs(3600);
    for f in crate::walk::walk(root, &[root.to_path_buf()])
        .unwrap()
        .files
    {
        std::fs::File::options()
            .write(true)
            .open(&f.abs_path)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
}

fn counted_fixture() -> (tempfile::TempDir, Kb, Rc<RefCell<Vec<PathBuf>>>) {
    let (d, kb) = fixture();
    backdate(d.path().canonicalize().unwrap().as_path());
    let reads = Rc::new(RefCell::new(Vec::new()));
    let kb = kb.with_reader(Box::new(Counting(reads.clone())));
    (d, kb, reads)
}

#[test]
fn unchanged_files_are_not_read_again() {
    let (_d, mut kb, reads) = counted_fixture();
    run(&mut kb, "auth");
    assert_eq!(reads.borrow().len(), 32);
    reads.borrow_mut().clear();
    run(&mut kb, "auth");
    assert!(reads.borrow().is_empty(), "{:?}", reads.borrow());
}

#[test]
fn edits_are_seen_and_only_that_file_is_read() {
    let (d, mut kb, reads) = counted_fixture();
    run(&mut kb, "auth");
    reads.borrow_mut().clear();
    let p = d.path().join("sumo/5xx-by-authorizer.md");
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(
        &p,
        text.replace("5xx by authorizer", "gateway timeouts by authorizer"),
    )
    .unwrap();
    let o = run(&mut kb, "gateway timeouts");
    assert_eq!(o.results[0].id, "sumo/5xx-by-authorizer");
    assert_eq!(reads.borrow().len(), 1);
}

#[test]
fn same_bytes_with_a_new_timestamp_only_touches_the_row() {
    let (d, mut kb, reads) = counted_fixture();
    run(&mut kb, "auth");
    reads.borrow_mut().clear();
    let p = d.path().join("cw/lambda-errors.md");
    std::fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(60))
        .unwrap();
    run(&mut kb, "lambda");
    assert_eq!(reads.borrow().len(), 1);
    reads.borrow_mut().clear();
    run(&mut kb, "lambda");
    assert!(reads.borrow().is_empty());
}

#[test]
fn a_racy_same_size_same_mtime_edit_is_still_seen() {
    let (d, mut kb) = kb_of(&[("a/one", entry("Alpha", "", "x", "aaaa"))]);
    let p = d.path().join("a/one.md");
    run(&mut kb, "alpha");
    let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
    std::fs::write(&p, entry("Alpha", "", "x", "bbbb")).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    assert_eq!(ids(&run(&mut kb, "bbbb")), vec!["a/one"]);
}

#[test]
fn deletes_and_creates_are_seen() {
    let (d, mut kb) = fixture();
    assert!(ids(&run(&mut kb, "login")).contains(&"sumo/login-failures"));
    std::fs::remove_file(d.path().join("sumo/login-failures.md")).unwrap();
    assert!(!ids(&run(&mut kb, "login")).contains(&"sumo/login-failures"));
    std::fs::write(
        d.path().join("sumo/new.md"),
        entry("Brand new login thing", "", "x", ""),
    )
    .unwrap();
    assert_eq!(ids(&run(&mut kb, "brand")), vec!["sumo/new"]);
}

#[test]
fn a_skipped_file_returns_when_fixed() {
    let (d, mut kb) = fixture();
    let p = d.path().join("prompt/cache.md");
    let good = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, good.replace("type: prompt", "type: nope")).unwrap();
    let o = run(&mut kb, "caching layers");
    assert!(o.results.is_empty());
    assert!(o.notices.contains(&Notice::Skipped { count: 1 }));
    std::fs::write(&p, &good).unwrap();
    let o = run(&mut kb, "caching layers");
    assert_eq!(ids(&o), vec!["prompt/cache"]);
    assert!(
        !o.notices
            .iter()
            .any(|n| matches!(n, Notice::Skipped { .. }))
    );
}

#[test]
fn case_colliding_ids_are_both_skipped() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    for n in ["one", "two"] {
        std::fs::write(root.join(format!("{n}.md")), entry(n, "", "x", "")).unwrap();
    }
    let walk = crate::walk::Walk {
        files: vec![
            crate::walk::WalkEntry {
                id: "A/x".into(),
                rel_path: "A/x.md".into(),
                abs_path: root.join("one.md"),
            },
            crate::walk::WalkEntry {
                id: "a/x".into(),
                rel_path: "a/x.md".into(),
                abs_path: root.join("two.md"),
            },
        ],
        findings: vec![],
    };
    let scan = crate::index::sync::scan(&HashMap::new(), 0, &walk, &FsReader);
    assert_eq!(scan.changes.len(), 2);
    for c in &scan.changes {
        match c {
            crate::index::sync::Change::Upsert { built, reason, .. } => {
                assert_eq!(built.verdict, crate::index::doc::Verdict::Skipped);
                assert_eq!(*reason, Some("collision"));
            }
            _ => panic!("expected upserts"),
        }
    }
}

#[test]
fn renames_keep_usage() {
    let (d, mut kb) = fixture();
    run(&mut kb, "auth");
    kb.record_use("sql/orders-by-status", now()).unwrap();
    kb.record_use("sql/orders-by-status", now()).unwrap();
    std::fs::rename(
        d.path().join("sql/orders-by-status.md"),
        d.path().join("sql/orders.md"),
    )
    .unwrap();
    let o = run(&mut kb, "status");
    assert_eq!(o.results[0].id, "sql/orders");
    assert_eq!(kb.usage("sql/orders").unwrap().count, 2);
    assert!(kb.usage("sql/orders-by-status").is_none());
}

#[test]
fn renames_keep_usage_even_when_the_index_was_deleted() {
    let (d, mut kb) = fixture();
    kb.record_use("sql/orders-by-status", now()).unwrap();
    std::fs::rename(
        d.path().join("sql/orders-by-status.md"),
        d.path().join("sql/orders.md"),
    )
    .unwrap();
    std::fs::remove_dir_all(d.path().join(".kb/index.db")).ok();
    let _ = std::fs::remove_file(d.path().join(".kb/index.db"));
    run(&mut kb, "status");
    assert_eq!(kb.usage("sql/orders").unwrap().count, 1);
}

#[test]
fn rename_plus_edit_is_a_new_entry_but_edit_then_rename_carries() {
    let (d, mut kb) = fixture();
    kb.record_use("sql/orders-by-status", now()).unwrap();
    let p = d.path().join("sql/orders-by-status.md");
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(
        d.path().join("sql/renamed-edited.md"),
        text.replace("Order counts", "Counts"),
    )
    .unwrap();
    std::fs::remove_file(&p).unwrap();
    run(&mut kb, "status");
    assert!(kb.usage("sql/renamed-edited").is_none());

    // edit, search (refreshes the stored hash), then rename
    let (d, mut kb) = fixture();
    kb.record_use("sql/orders-by-status", now()).unwrap();
    let p = d.path().join("sql/orders-by-status.md");
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, text.replace("Order counts", "Counts")).unwrap();
    run(&mut kb, "status");
    std::fs::rename(&p, d.path().join("sql/moved.md")).unwrap();
    run(&mut kb, "status");
    assert_eq!(kb.usage("sql/moved").unwrap().count, 1);
}

#[test]
fn reindex_prunes_usage_of_vanished_entries_and_keeps_the_rest() {
    let (d, mut kb) = fixture();
    kb.record_use("sql/orders-by-status", now()).unwrap();
    kb.record_use("link/oncall-handbook", now()).unwrap();
    std::fs::remove_file(d.path().join("link/oncall-handbook.md")).unwrap();
    run(&mut kb, "x");
    assert!(
        kb.usage("link/oncall-handbook").is_some(),
        "kept between rebuilds"
    );
    let r = kb.reindex().unwrap();
    assert_eq!((r.indexed, r.skipped), (31, 0));
    assert!(kb.usage("link/oncall-handbook").is_none());
    assert_eq!(kb.usage("sql/orders-by-status").unwrap().count, 1);
}

#[test]
fn deleting_the_kb_dir_changes_only_frecency_order() {
    let (d, mut kb) = fixture();
    let before = ids(&run(&mut kb, "latency"))
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    std::fs::remove_dir_all(d.path().join(".kb")).unwrap();
    let o = run(&mut kb, "latency");
    assert_eq!(
        ids(&o),
        before.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert!(
        o.notices
            .iter()
            .any(|n| matches!(n, Notice::Rebuilt { .. }))
    );
}

#[test]
fn a_garbage_index_or_state_file_is_recovered() {
    let (d, mut kb) = fixture();
    run(&mut kb, "x");
    // overwriting the file under an open connection would be fixed up when it closes
    kb.held = None;
    std::fs::write(d.path().join(".kb/index.db"), "garbage").unwrap();
    std::fs::write(d.path().join(".kb/state.json"), "{").unwrap();
    let o = run(&mut kb, "latency");
    assert!(
        o.notices
            .iter()
            .any(|n| matches!(n, Notice::Rebuilt { entries: 32 }))
    );
    assert!(o.notices.contains(&Notice::StateReset));
    assert!(d.path().join(".kb/state.json.bak").exists());
    assert!(!run(&mut kb, "latency").results.is_empty());
}

#[cfg(unix)]
#[test]
fn a_read_only_kb_dir_falls_back_to_memory() {
    use std::os::unix::fs::PermissionsExt;
    let (d, mut kb) = fixture();
    std::fs::create_dir(d.path().join(".kb")).unwrap();
    let kbdir = d.path().join(".kb");
    std::fs::set_permissions(&kbdir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let writable = std::fs::write(kbdir.join("probe"), "x").is_ok(); // running as root?
    let o = run(&mut kb, "latency");
    std::fs::set_permissions(&kbdir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!o.results.is_empty());
    if !writable {
        assert!(o.notices.contains(&Notice::ReadOnly));
        assert!(!kbdir.join("index.db").exists());
    }
}

#[test]
fn words_added_after_the_first_build_are_typo_matchable() {
    let (d, mut kb) = fixture();
    run(&mut kb, "auth");
    std::fs::write(
        d.path().join("sumo/new.md"),
        entry("Zeppelin schedule", "", "x", "nothing"),
    )
    .unwrap();
    let o = run(&mut kb, "zeppelni");
    assert_eq!(ids(&o), vec!["sumo/new"]);
    assert_eq!(o.results[0].tier, Tier::Typo);
}

#[test]
fn damage_found_while_querying_triggers_one_rebuild_and_a_retry() {
    let (d, mut kb) = fixture();
    backdate(d.path());
    run(&mut kb, "auth");
    {
        // Leave the file a valid database that opens fine, but break the full-text data.
        let c = Connection::open(d.path().join(".kb/index.db")).unwrap();
        c.execute_batch("UPDATE fts_data SET block = zeroblob(length(block)) WHERE id > 10")
            .unwrap();
    }
    let o = run(&mut kb, "5xx auth");
    assert_eq!(o.results[0].id, "sumo/5xx-by-authorizer");
    assert!(
        o.notices
            .iter()
            .any(|n| matches!(n, Notice::Rebuilt { entries: 32 })),
        "{:?}",
        o.notices
    );
    // and the index is healthy afterwards
    assert!(run(&mut kb, "5xx auth").notices.is_empty());
}

// ---- M3 additions: refresh/query, counts, result fields, lint_entry ----

#[test]
fn refresh_then_query_equals_search() {
    let (_d, mut kb) = fixture();
    let (_d2, mut other) = fixture();
    let r = kb.refresh().unwrap();
    assert_eq!((r.indexed, r.skipped), (32, 0));
    let table = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/search/queries.tsv"),
    )
    .unwrap();
    for line in table
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .take(10)
    {
        let q = Query::parse(line.split('\t').next().unwrap());
        let a = kb.query(&q, now()).unwrap();
        let b = other.search(&q, now()).unwrap();
        assert_eq!(ids(&a), ids(&b), "{line}");
        assert_eq!(a.type_counts, b.type_counts);
    }
}

#[test]
fn query_uses_the_held_index_and_search_refreshes_it() {
    let (d, mut kb) = fixture();
    kb.refresh().unwrap();
    std::fs::write(
        d.path().join("sumo/zeppelin.md"),
        entry("Zeppelin", "", "x", ""),
    )
    .unwrap();
    assert!(
        kb.query(&Query::parse("zeppelin"), now())
            .unwrap()
            .results
            .is_empty()
    );
    assert_eq!(
        ids(&kb.search(&Query::parse("zeppelin"), now()).unwrap()),
        vec!["sumo/zeppelin"]
    );
    // a query on a fresh handle builds the index itself and reports it
    let (_d2, mut fresh) = fixture();
    let o = fresh.query(&Query::parse("auth"), now()).unwrap();
    assert!(
        o.notices
            .iter()
            .any(|n| matches!(n, Notice::Rebuilt { entries: 32 }))
    );
}

#[test]
fn query_still_drops_files_that_are_gone() {
    let (d, mut kb) = fixture();
    kb.refresh().unwrap();
    std::fs::remove_file(d.path().join("sumo/5xx-by-authorizer.md")).unwrap();
    let o = kb.query(&Query::parse("5xx auth"), now()).unwrap();
    assert!(!ids(&o).contains(&"sumo/5xx-by-authorizer"));
}

#[test]
fn type_counts_ignore_the_type_filter() {
    let (_d, mut kb) = fixture();
    let plain = run(&mut kb, "latency");
    let cw = run(&mut kb, "latency type:cw");
    assert_eq!(plain.type_counts, cw.type_counts);
    assert!(cw.results.iter().all(|r| r.entry_type == EntryType::Cw));
    assert_eq!(cw.total, cw.results.len());
    assert!(plain.total > cw.total);
    let cw_index = EntryType::ALL
        .iter()
        .position(|t| *t == EntryType::Cw)
        .unwrap();
    assert_eq!(cw.type_counts[cw_index], cw.total);
    assert_eq!(plain.type_counts.iter().sum::<usize>(), plain.total);
    let all = run(&mut kb, "");
    assert_eq!(all.type_counts.iter().sum::<usize>(), 32);
    assert_eq!((all.indexed, all.skipped), (32, 0));
}

#[test]
fn typo_matches_are_considered_when_the_type_filtered_page_is_not_full() {
    let (_d, mut kb) = fixture();
    // one exact `link` match would fill nothing at limit 20; the typo path must still run
    let o = run(&mut kb, "type:sumo authroizer");
    assert!(ids(&o).contains(&"sumo/5xx-by-authorizer"), "{:?}", ids(&o));
    let mut q = Query::parse("type:link authorizer");
    q.limit = 1;
    assert!(kb.search(&q, now()).unwrap().results.is_empty());
}

#[test]
fn results_carry_use_count_updated_and_modified() {
    let (d, mut kb) = fixture();
    let r = run(&mut kb, "5xx auth").results.remove(0);
    assert_eq!(r.id, "sumo/5xx-by-authorizer");
    assert_eq!(r.updated.as_deref(), Some("2026-09-30"));
    assert_eq!(r.use_count, 0);
    let mtime = std::fs::metadata(d.path().join("sumo/5xx-by-authorizer.md"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert_eq!(r.modified, Some(mtime));
    kb.record_use("sumo/5xx-by-authorizer", now()).unwrap();
    kb.record_use("sumo/5xx-by-authorizer", now()).unwrap();
    let r = run(&mut kb, "5xx auth").results.remove(0);
    assert_eq!(r.use_count, 2);
    let other = run(&mut kb, "latency prod").results.remove(0);
    assert_eq!(other.updated, None);
}

#[test]
fn lint_entry_reports_errors_only() {
    let (d, kb) = fixture();
    assert!(kb.lint_entry("sql/orders-by-status").unwrap().is_empty());
    let p = d.path().join("sql/orders-by-status.md");
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, text.replace("type: sql", "type: nope")).unwrap();
    let findings = kb.lint_entry("sql/orders-by-status").unwrap();
    assert!(!findings.is_empty());
    assert!(
        findings
            .iter()
            .all(|f| f.severity == crate::lint::Severity::Error)
    );
    assert!(
        findings
            .iter()
            .any(|f| f.code.to_string().starts_with("KB001"))
    );
    assert!(matches!(kb.lint_entry("nope/x"), Err(KbError::NotFound(_))));
}

#[test]
fn a_version_one_index_is_rebuilt() {
    let (d, mut kb) = fixture();
    run(&mut kb, "x");
    {
        let c = Connection::open(d.path().join(".kb/index.db")).unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
    }
    let o = run(&mut kb, "auth");
    assert!(
        o.notices
            .iter()
            .any(|n| matches!(n, Notice::Rebuilt { entries: 32 }))
    );
    assert_eq!(crate::index::INDEX_VERSION, 4);
}

#[test]
fn snippet_text_loses_escape_sequences_and_ranges_follow_the_text() {
    let (text, ranges) = clean_text("ab\x1b[2Jcd needle ef\x07".to_string(), vec![9..15]);
    assert_eq!(text, "ab␛cd needle ef");
    assert_eq!(
        &text
            .chars()
            .skip(ranges[0].start)
            .take(ranges[0].len())
            .collect::<String>(),
        "needle"
    );
    // clean text is untouched
    let (text, ranges) = clean_text("plain text".to_string(), vec![0..5]);
    assert_eq!((text.as_str(), ranges), ("plain text", vec![0..5]));
    // a match that was only an escape sequence is dropped
    let (_, ranges) = clean_text("a\x1b[31mb".to_string(), vec![1..6]);
    assert!(ranges.iter().all(|r| r.start < r.end));
}
