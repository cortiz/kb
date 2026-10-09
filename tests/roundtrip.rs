//! Every fixture round-trips byte for byte (FR-016, SC-001), and no input makes the parser panic.

use std::fs;
use std::path::{Path, PathBuf};

use kb_core::entry::parse_entry;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn md_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(md_files(&path));
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[test]
fn every_parseable_fixture_round_trips() {
    let mut checked = 0;
    for file in md_files(&fixtures()) {
        let bytes = fs::read(&file).unwrap();
        let name = file.to_string_lossy();
        if let Some(entry) = parse_entry("x", &name, bytes.clone()).entry {
            assert_eq!(entry.to_bytes(), bytes, "{name}");
            checked += 1;
        }
    }
    assert!(checked >= 40, "only {checked} fixtures parsed");
}

#[test]
fn all_valid_fixtures_parse() {
    for file in md_files(&fixtures().join("valid")) {
        let out = parse_entry("x", "x.md", fs::read(&file).unwrap());
        assert!(out.entry.is_some(), "{}", file.display());
    }
}

/// A small deterministic generator, so the test needs no extra dependency.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap()
    }
}

#[test]
fn parser_never_panics() {
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    const ALPHABET: &[u8] = b"---\n\r\t :{}[],\"'#&*!|>%@`~\\-ab1\xC3\xA9\xFF";
    for _ in 0..10_000 {
        let len = rng.below(200);
        let mut bytes: Vec<u8> = b"---\n".to_vec();
        bytes.extend((0..len).map(|_| ALPHABET[rng.below(ALPHABET.len())]));
        if rng.below(2) == 0 {
            bytes.extend_from_slice(b"\n---\n```\n{{a}} \\{{b}}\n```\n");
        }
        let _ = parse_entry("x", "x.md", bytes);
    }
    for file in md_files(&fixtures()) {
        let bytes = fs::read(&file).unwrap();
        for end in 0..=bytes.len() {
            let _ = parse_entry("x", "x.md", bytes[..end].to_vec());
        }
    }
}
