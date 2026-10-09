//! Shared helpers for the CLI integration tests.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use assert_cmd::Command;

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

pub fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A temporary KB holding a copy of `tests/fixtures/search` (32 lint-clean entries).
pub fn search_kb() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kb");
    copy_dir(&fixtures().join("search"), &root);
    (dir, root)
}

/// A `kb` command that ignores the caller's environment root.
pub fn kb() -> Command {
    let mut cmd = Command::cargo_bin("kb").unwrap();
    cmd.env_remove("KB_ROOT").env_remove("NO_COLOR");
    cmd
}

pub fn kb_in(root: &Path) -> Command {
    let mut cmd = kb();
    cmd.arg("--root").arg(root);
    cmd
}

/// Sets every `.md` file's modification time to an hour ago, so the "racy" rule (files edited
/// within 2 s of the last sync are re-read) does not apply.
pub fn backdate(root: &Path) {
    let old = SystemTime::now() - Duration::from_secs(3600);
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if !path.file_name().unwrap().to_string_lossy().starts_with('.') {
                backdate(&path);
            }
        } else if path.extension().is_some_and(|e| e == "md") {
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
    }
}

pub fn entry_text(title: &str, tags: &str, body: &str, notes: &str) -> String {
    format!(
        "---\nkb_format: 1\ntype: prompt\ntitle: {title}\ntags: [{tags}]\n---\n\n```\n{body}\n```\n\n{notes}\n"
    )
}

// ---- corpus generator ----

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

const CONSONANTS: &[u8] = b"bdfgklmnprstvz";
const VOWELS: &[u8] = b"aeiou";

/// The `k`-th word of a made-up vocabulary: three consonant-vowel syllables.
fn word(mut k: usize) -> String {
    let mut s = String::new();
    for _ in 0..3 {
        s.push(CONSONANTS[k % CONSONANTS.len()] as char);
        k /= CONSONANTS.len();
        s.push(VOWELS[k % VOWELS.len()] as char);
        k /= VOWELS.len();
    }
    s
}

/// A word from a Zipf-like distribution over 40,000 words (a few are common, most are rare).
fn zipf_word(rng: &mut Rng) -> String {
    let u = rng.unit();
    word((40_000.0 * u * u * u) as usize)
}

fn entry(i: usize, rng: &mut Rng) -> String {
    let words: Vec<String> = (0..60).map(|_| zipf_word(rng)).collect();
    let notes = words.join(" ");
    let title_words = format!("{} {}", words[0], words[1]);
    match i % 7 {
        0 => format!(
            "---\nkb_format: 1\ntype: sql\ntitle: Query {i} {title_words}\ntags: [db, perf]\ndialect: postgres\nvars:\n  limit: {{ kind: number, default: 50 }}\n---\n\n{notes}\n\n```sql\nselect id, status, created_at\nfrom orders\nwhere status = 'failed'\norder by created_at desc\nlimit {{{{limit}}}};\n```\n"
        ),
        1 => format!(
            "---\nkb_format: 1\ntype: sumo\ntitle: Search {i} {title_words}\ntags: [logs]\nrange: \"-{{{{minutes}}}}m\"\nvars:\n  env: {{ kind: enum, options: [prod, stage], default: prod }}\n  minutes: {{ kind: number, default: 15 }}\n---\n\n```\n_sourceCategory={{{{env}}}}/app | where status >= 500 | count by host\n```\n\n{notes}\n"
        ),
        2 => format!(
            "---\nkb_format: 1\ntype: cw\ntitle: Insights {i} {title_words}\nlog_groups: [/aws/lambda/app-{i}]\nrange: -1h\n---\n\n```\nfields @timestamp, @message | filter @message like /ERROR/ | limit 20\n```\n\n{notes}\n"
        ),
        3 => format!(
            "---\nkb_format: 1\ntype: cmd\ntitle: Command {i} {title_words}\nshell: bash\nvars:\n  ns: {{ kind: text, default: default }}\n---\n\n```bash\nkubectl -n {{{{ns}}}} get pods -o wide | grep -v Running\n```\n\n{notes}\n"
        ),
        4 => format!(
            "---\nkb_format: 1\ntype: prompt\ntitle: Prompt {i} {title_words}\n---\n\n````\nSummarize:\n```\nlog\n```\n````\n\n{notes}\n"
        ),
        5 => format!(
            "---\nkb_format: 1\ntype: link\ntitle: Link {i} {title_words}\nurl: https://example.com/docs/{i}\n---\n\n{notes}\n"
        ),
        _ => format!(
            "---\nkb_format: 1\ntype: code\ntitle: Code {i} {title_words}\ncode:\n  repo: example-org/service\n  path: src/module_{i}.rs\n  line: {}\n  ref: main\n---\n\n{notes}\n",
            i % 900 + 1
        ),
    }
}

/// Writes `n` deterministic, lint-clean entries under `root` in 50 folders.
pub fn generate_kb(root: &Path, n: usize) {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for i in 0..n {
        let dir = root.join(format!("group-{:02}", i % 50));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("entry-{i:05}.md")), entry(i, &mut rng)).unwrap();
    }
}

/// Path of generated entry `i` (for tests that edit one).
pub fn generated_path(root: &Path, i: usize) -> PathBuf {
    root.join(format!("group-{:02}", i % 50))
        .join(format!("entry-{i:05}.md"))
}

#[test]
fn generated_entries_lint_clean() {
    let dir = tempfile::tempdir().unwrap();
    generate_kb(dir.path(), 200);
    let out = kb_in(dir.path())
        .args(["lint", "--strict", "--quiet"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "200 entries, 0 errors, 0 warnings\n"
    );
}
