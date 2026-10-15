//! The worker thread: it owns the index and answers the UI. The UI never blocks on it, and only
//! the newest request of each kind is answered (a keystroke that arrives while a search runs
//! replaces the queued searches).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::SystemTime;

use kb_core::query::Query;
use kb_core::search::Kb;

use super::app::{Page, RefreshInfo};
use super::preview::{PreviewModel, build_preview_with};
use super::render_cache::RenderCache;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Refresh,
    Search {
        generation: u64,
        query: String,
        limit: usize,
    },
    Preview {
        generation: u64,
        id: String,
        modified: Option<i64>,
        use_count: u64,
    },
    /// The tags in use, with how many entries have each (for the add form).
    Tags,
    Shutdown,
}

pub enum Reply {
    Refreshed(RefreshInfo),
    Results(u64, Page),
    Preview(u64, Box<PreviewModel>),
    Tags(Vec<(String, u32)>),
    Failed(String),
}

pub struct Worker {
    pub tx: Sender<Request>,
    pub rx: Receiver<Reply>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn spawn(root: PathBuf, highlight: bool) -> Worker {
        let (tx, requests) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || run(&root, highlight, &requests, &replies));
        Worker {
            tx,
            rx,
            handle: Some(handle),
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Request::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// What to do after reading everything that is waiting.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Batch {
    pub shutdown: bool,
    pub refresh: bool,
    pub search: Option<(u64, String, usize)>,
    pub preview: Option<(u64, String, Option<i64>, u64)>,
    pub tags: bool,
}

/// Keeps the newest search and the newest preview request, and one refresh.
pub fn coalesce(requests: Vec<Request>) -> Batch {
    let mut b = Batch::default();
    for r in requests {
        match r {
            Request::Shutdown => b.shutdown = true,
            Request::Refresh => b.refresh = true,
            Request::Tags => b.tags = true,
            Request::Search {
                generation,
                query,
                limit,
            } => b.search = Some((generation, query, limit)),
            Request::Preview {
                generation,
                id,
                modified,
                use_count,
            } => {
                b.preview = Some((generation, id, modified, use_count));
            }
        }
    }
    b
}

pub fn run(
    root: &std::path::Path,
    highlight: bool,
    requests: &Receiver<Request>,
    replies: &Sender<Reply>,
) {
    let mut kb = match Kb::open(root) {
        Ok(kb) => kb,
        Err(e) => {
            let _ = replies.send(Reply::Failed(e.to_string()));
            return;
        }
    };
    let mut cache = RenderCache::default();
    let mut warmed = false;
    loop {
        let Ok(first) = requests.recv() else {
            return;
        };
        let mut pending = vec![first];
        while let Ok(r) = requests.try_recv() {
            pending.push(r);
        }
        let batch = coalesce(pending);
        let batch_had_results = batch.search.is_some();
        if batch.shutdown {
            return;
        }
        if batch.refresh {
            let reply = match kb.refresh() {
                Ok(r) => Reply::Refreshed(RefreshInfo {
                    indexed: r.indexed,
                    skipped: r.skipped,
                    duration: r.duration,
                }),
                Err(e) => Reply::Failed(e.to_string()),
            };
            if replies.send(reply).is_err() {
                return;
            }
        }
        if let Some((generation, text, limit)) = batch.search {
            let mut query = Query::parse(&text);
            query.limit = limit;
            let reply = match kb.query(&query, SystemTime::now()) {
                Ok(o) => Reply::Results(
                    generation,
                    Page {
                        results: o.results,
                        total: o.total,
                        type_counts: o.type_counts,
                        indexed: o.indexed,
                        skipped: o.skipped,
                        notices: o.notices,
                    },
                ),
                Err(e) => Reply::Failed(e.to_string()),
            };
            if replies.send(reply).is_err() {
                return;
            }
        }
        if let Some((generation, id, modified, use_count)) = batch.preview {
            let mtime = modified.unwrap_or(0);
            let reply = if let Some(model) = cache.get(&id, mtime) {
                let mut model = model.clone();
                let unreviewed =
                    kb_core::mcp::review::ReviewStateTracker::is_unreviewed(kb.root(), &id);
                if let Some(meta) = &mut model.meta {
                    meta.use_count = use_count;
                    meta.unreviewed = unreviewed;
                }
                Reply::Preview(generation, Box::new(model))
            } else {
                match kb.entry(&id) {
                    Ok(entry) => {
                        let lookup = |target: &str| crate::show::link_lookup(&kb, target);
                        let mut model = build_preview_with(
                            &entry,
                            modified,
                            use_count,
                            highlight,
                            Some(&lookup),
                        );
                        let unreviewed =
                            kb_core::mcp::review::ReviewStateTracker::is_unreviewed(kb.root(), &id);
                        if let Some(meta) = &mut model.meta {
                            meta.unreviewed = unreviewed;
                        }
                        cache.put(model.clone());
                        Reply::Preview(generation, Box::new(model))
                    }
                    Err(e) => Reply::Failed(e.to_string()),
                }
            };
            if replies.send(reply).is_err() {
                return;
            }
        }
        if batch.tags && replies.send(Reply::Tags(kb.tag_counts())).is_err() {
            return;
        }
        if highlight && !warmed && batch_had_results {
            warmed = true;
            super::highlight::warm_up();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let src =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/search");
        copy(&src, d.path());
        let root = d.path().to_path_buf();
        (d, root)
    }

    fn copy(from: &std::path::Path, to: &std::path::Path) {
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

    fn recv(w: &Worker) -> Reply {
        w.rx.recv_timeout(Duration::from_secs(20)).expect("a reply")
    }

    #[test]
    fn coalescing_keeps_the_newest_of_each_kind() {
        let b = coalesce(vec![
            Request::Search {
                generation: 2,
                query: "a".into(),
                limit: 60,
            },
            Request::Refresh,
            Request::Search {
                generation: 3,
                query: "ab".into(),
                limit: 60,
            },
            Request::Preview {
                generation: 1,
                id: "x".into(),
                modified: None,
                use_count: 0,
            },
            Request::Refresh,
            Request::Search {
                generation: 4,
                query: "abc".into(),
                limit: 0,
            },
            Request::Preview {
                generation: 2,
                id: "y".into(),
                modified: Some(5),
                use_count: 2,
            },
        ]);
        assert!(b.refresh && !b.shutdown);
        assert_eq!(b.search, Some((4, "abc".into(), 0)));
        assert_eq!(b.preview, Some((2, "y".into(), Some(5), 2)));
        assert!(coalesce(vec![Request::Refresh, Request::Shutdown]).shutdown);
    }

    #[test]
    fn refresh_then_search_answers_with_the_headline_entry() {
        let (_d, root) = fixture();
        let w = Worker::spawn(root, true);
        w.tx.send(Request::Refresh).unwrap();
        let Reply::Refreshed(info) = recv(&w) else {
            panic!("expected Refreshed")
        };
        assert_eq!((info.indexed, info.skipped), (32, 0));
        w.tx.send(Request::Search {
            generation: 1,
            query: "5xx auth".into(),
            limit: 60,
        })
        .unwrap();
        let Reply::Results(g, page) = recv(&w) else {
            panic!("expected Results")
        };
        assert_eq!(g, 1);
        assert_eq!(page.results[0].id, "sumo/5xx-by-authorizer");
        assert_eq!(page.indexed, 32);
        let sumo = kb_core::entry::EntryType::ALL
            .iter()
            .position(|t| t.name() == "sumo")
            .unwrap();
        assert!(page.type_counts[sumo] >= 1);
        w.tx.send(Request::Preview {
            generation: 7,
            id: "sumo/5xx-by-authorizer".into(),
            modified: Some(1),
            use_count: 4,
        })
        .unwrap();
        let Reply::Preview(g, model) = recv(&w) else {
            panic!("expected Preview")
        };
        assert_eq!((g, model.title.as_str()), (7, "5xx by authorizer"));
    }

    #[test]
    fn only_the_newest_queued_search_is_answered() {
        let (_d, root) = fixture();
        let (tx, requests) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        tx.send(Request::Refresh).unwrap();
        for g in 2..=4 {
            tx.send(Request::Search {
                generation: g,
                query: format!("auth{g}"),
                limit: 60,
            })
            .unwrap();
        }
        let handle = std::thread::spawn(move || run(&root, true, &requests, &replies));
        let mut generations = Vec::new();
        while let Ok(r) = rx.recv_timeout(Duration::from_millis(1500)) {
            if let Reply::Results(g, _) = r {
                generations.push(g);
            }
        }
        assert_eq!(generations, vec![4]);
        tx.send(Request::Shutdown).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn a_search_with_no_matches_is_an_empty_page_not_a_failure() {
        let (_d, root) = fixture();
        let w = Worker::spawn(root, true);
        w.tx.send(Request::Search {
            generation: 1,
            query: "zzzznotaword".into(),
            limit: 60,
        })
        .unwrap();
        match recv(&w) {
            Reply::Results(1, page) => assert!(page.results.is_empty() && page.total == 0),
            _ => panic!("expected empty Results"),
        }
    }

    #[test]
    fn a_missing_root_fails_once_and_shutdown_joins() {
        let w = Worker::spawn(PathBuf::from("/no/such/kb"), true);
        assert!(matches!(recv(&w), Reply::Failed(_)));
        let (_d, root) = fixture();
        let mut w = Worker::spawn(root, true);
        w.shutdown();
    }
}
