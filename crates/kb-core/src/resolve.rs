//! Turning what the user typed into one entry: an exact ID, an ID with `.md` or a path, or a
//! search with a clear winner. `kb copy` and `kb show` share this (FR-020).

use std::path::Path;
use std::time::SystemTime;

use crate::query::Query;
use crate::search::{Kb, KbError};

/// The first search result wins when its score is at least this many times the second one's.
/// A ratio, not a gap, because scores scale with the query (relevance times boosts).
///
/// Calibrated on `tests/fixtures/search` (see `the_ratio_separates_the_fixture_cases`): the
/// headline query `5xx auth` has a single match (score 8.4), while `latency` has four near-equal
/// ones (4.200, 4.192, 4.144, 3.633: ratio 1.002). Any ratio between about 1.01 and 8 separates
/// them; 1.5 leaves room on both sides.
pub const CLEAR_WINNER_RATIO: f64 = 1.5;

/// How many close matches are offered when there is no clear winner.
pub const TOP_CANDIDATES: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// Exactly one entry: an exact ID, a path, or a clear search winner.
    Resolved(String),
    /// Several close matches, best first (at most [`TOP_CANDIDATES`]).
    Ambiguous(Vec<Candidate>),
    NotFound,
}

/// Whether scores sorted best-first have a clear winner: there is one result, or the best is at
/// least [`CLEAR_WINNER_RATIO`] times the runner-up.
pub fn clear_winner(scores: &[f64]) -> bool {
    match scores {
        [] => false,
        [_] => true,
        [best, second, ..] => *second <= 0.0 || *best >= CLEAR_WINNER_RATIO * *second,
    }
}

/// `text` as an entry ID: no `.md`, no leading `./`, no trailing slash.
fn normalize_id(text: &str) -> String {
    let t = text.trim().trim_start_matches("./").trim_end_matches('/');
    t.strip_suffix(".md").unwrap_or(t).to_string()
}

impl Kb {
    /// Resolves `text` (FR-020): an exact ID, then an ID with `.md` or a path to a file inside
    /// the root, then a search.
    pub fn resolve(&mut self, text: &str) -> Result<Resolution, KbError> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Resolution::NotFound);
        }
        if !text.contains(char::is_whitespace) {
            // 1. an exact ID, 2. the same with `.md` or `./` (a path relative to the root)
            for candidate in [text.to_string(), normalize_id(text)] {
                if self.entry_path(&candidate).is_ok() {
                    return Ok(Resolution::Resolved(candidate));
                }
            }
            // 2. a path to a file (absolute, or relative to the root) that is inside the root
            if let Some(id) = self.id_of_path(Path::new(text)) {
                return Ok(Resolution::Resolved(id));
            }
        }
        // 3. a search
        let mut query = Query::parse(text);
        query.limit = TOP_CANDIDATES;
        let outcome = self.search(&query, SystemTime::now())?;
        let scores: Vec<f64> = outcome.results.iter().map(|r| r.score).collect();
        Ok(match outcome.results.len() {
            0 => Resolution::NotFound,
            _ if clear_winner(&scores) => Resolution::Resolved(outcome.results[0].id.clone()),
            _ => Resolution::Ambiguous(
                outcome
                    .results
                    .iter()
                    .map(|r| Candidate {
                        id: r.id.clone(),
                        title: r.title.clone(),
                        score: r.score,
                    })
                    .collect(),
            ),
        })
    }

    /// The ID of the entry file at `path`, if it is a `.md` file inside the root.
    fn id_of_path(&self, path: &Path) -> Option<String> {
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root().join(path)
        };
        let full = full.canonicalize().ok()?;
        let root = self.root().canonicalize().ok()?;
        let rel = full.strip_prefix(&root).ok()?;
        let id = normalize_id(&rel.to_string_lossy());
        (full.extension().is_some_and(|e| e == "md") && self.entry_path(&id).is_ok()).then_some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/search");
        let root = d.path().join("kb");
        copy(&src, &root);
        (d, root)
    }

    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &to.join(e.file_name()));
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }

    fn resolve(kb: &mut Kb, text: &str) -> Resolution {
        kb.resolve(text).unwrap()
    }

    #[test]
    fn the_rule_for_a_clear_winner() {
        assert!(!clear_winner(&[]));
        assert!(clear_winner(&[0.3]), "a single result always wins");
        assert!(clear_winner(&[3.0, 2.0]), "exactly 1.5x wins");
        assert!(!clear_winner(&[2.9, 2.0]));
        assert!(clear_winner(&[1.0, 0.0]), "a runner-up with no score loses");
        assert!(clear_winner(&[9.0, 2.0, 2.0, 2.0]));
        assert!(!clear_winner(&[2.0, 2.0, 1.0]), "a tie is ambiguous");
    }

    #[test]
    fn an_exact_id_resolves_without_searching() {
        let (_d, root) = fixture();
        let mut kb = Kb::open(&root).unwrap();
        assert_eq!(
            resolve(&mut kb, "sumo/5xx-by-authorizer"),
            Resolution::Resolved("sumo/5xx-by-authorizer".into())
        );
        // whitespace around it is ignored
        assert_eq!(
            resolve(&mut kb, "  sumo/5xx-by-authorizer \n"),
            Resolution::Resolved("sumo/5xx-by-authorizer".into())
        );
    }

    #[test]
    fn an_id_with_md_or_dot_slash_resolves_to_the_id() {
        let (_d, root) = fixture();
        let mut kb = Kb::open(&root).unwrap();
        for text in [
            "sumo/5xx-by-authorizer.md",
            "./sumo/5xx-by-authorizer",
            "./sumo/5xx-by-authorizer.md",
        ] {
            assert_eq!(
                resolve(&mut kb, text),
                Resolution::Resolved("sumo/5xx-by-authorizer".into()),
                "{text}"
            );
        }
    }

    #[test]
    fn a_path_inside_the_root_resolves_and_one_outside_does_not() {
        let (d, root) = fixture();
        let mut kb = Kb::open(&root).unwrap();
        let abs = root.join("sumo/5xx-by-authorizer.md");
        assert_eq!(
            resolve(&mut kb, abs.to_str().unwrap()),
            Resolution::Resolved("sumo/5xx-by-authorizer".into())
        );
        let outside = d.path().join("elsewhere.md");
        std::fs::write(
            &outside,
            "---\nkb_format: 1\ntype: prompt\ntitle: x\n---\n\n```\nx\n```\n",
        )
        .unwrap();
        // a real file, but not in the root: not an entry (and the search finds nothing for it)
        assert_eq!(
            resolve(&mut kb, outside.to_str().unwrap()),
            Resolution::NotFound
        );
        // dot-dot cannot climb out of the root
        assert_eq!(resolve(&mut kb, "../elsewhere.md"), Resolution::NotFound);
    }

    #[test]
    fn a_search_with_one_clear_winner_resolves() {
        let (_d, root) = fixture();
        let mut kb = Kb::open(&root).unwrap();
        assert_eq!(
            resolve(&mut kb, "5xx auth"),
            Resolution::Resolved("sumo/5xx-by-authorizer".into())
        );
    }

    #[test]
    fn close_matches_are_ambiguous_with_at_most_five_candidates_best_first() {
        let (_d, root) = fixture();
        let mut kb = Kb::open(&root).unwrap();
        let Resolution::Ambiguous(c) = resolve(&mut kb, "latency") else {
            panic!("latency should be ambiguous")
        };
        assert!(c.len() >= 2 && c.len() <= TOP_CANDIDATES, "{}", c.len());
        assert!(c.windows(2).all(|w| w[0].score >= w[1].score), "best first");
        assert!(c.iter().all(|c| !c.title.is_empty() && c.id.contains('/')));
    }

    #[test]
    fn no_match_is_not_found() {
        let (_d, root) = fixture();
        let mut kb = Kb::open(&root).unwrap();
        assert_eq!(resolve(&mut kb, "zzzznotaword"), Resolution::NotFound);
        assert_eq!(resolve(&mut kb, ""), Resolution::NotFound);
        assert_eq!(resolve(&mut kb, "   "), Resolution::NotFound);
    }

    #[test]
    fn an_exact_id_is_never_treated_as_a_search() {
        // `cw/latency-prod` also matches other entries in a search; as an ID it must win alone
        let (_d, root) = fixture();
        let mut kb = Kb::open(&root).unwrap();
        assert_eq!(
            resolve(&mut kb, "cw/latency-prod"),
            Resolution::Resolved("cw/latency-prod".into())
        );
    }

    /// The scores behind `CLEAR_WINNER_RATIO` (run with `--nocapture` to see them). `5xx auth` is
    /// the spec's headline query and must resolve; `latency` matches several entries about
    /// equally and must stay ambiguous. If these fail after a scoring change, move the ratio.
    #[test]
    fn the_ratio_separates_the_fixture_cases() {
        let (_d, root) = fixture();
        let mut kb = Kb::open(&root).unwrap();
        let mut show = |text: &str| {
            let mut q = Query::parse(text);
            q.limit = TOP_CANDIDATES;
            let o = kb.search(&q, SystemTime::now()).unwrap();
            let scores: Vec<f64> = o.results.iter().map(|r| r.score).collect();
            eprintln!(
                "{text:>12}: {scores:.3?} -> clear winner: {}",
                clear_winner(&scores)
            );
            scores
        };
        let headline = show("5xx auth");
        let close = show("latency");
        assert!(clear_winner(&headline), "{headline:?}");
        assert!(!clear_winner(&close), "{close:?}");
    }
}
