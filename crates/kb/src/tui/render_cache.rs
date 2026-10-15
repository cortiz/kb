//! A bounded cache of built previews, so going back to a note does not parse it again.
//!
//! A note's parsed blocks ([`RenderedNote`]) do not depend on the terminal width: only its lines
//! do, and those are laid out again from the blocks when the width changes. The cache keeps at
//! most [`MAX_NOTES`] notes and [`MAX_BYTES`] of styled text, whichever limit is reached first,
//! and drops the least recently viewed first.

use std::collections::{HashMap, VecDeque};

use kb_core::entry::EntryType;

use super::preview::PreviewModel;

/// A note's parsed Markdown and the lines laid out from it for the last width drawn.
pub use super::markdown::Notes as RenderedNote;

/// Most notes kept.
pub const MAX_NOTES: usize = 20;
/// Most styled text kept, in bytes (16 MiB).
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
/// Most previews of any kind kept.
pub const MAX_ENTRIES: usize = 64;

type Key = (String, i64);

#[derive(Debug)]
pub struct RenderCache {
    map: HashMap<Key, (PreviewModel, usize)>,
    /// Least recently viewed first.
    order: VecDeque<Key>,
    max_notes: usize,
    max_bytes: usize,
    max_entries: usize,
    bytes: usize,
    notes: usize,
}

impl Default for RenderCache {
    fn default() -> RenderCache {
        RenderCache::with_limits(MAX_ENTRIES, MAX_NOTES, MAX_BYTES)
    }
}

fn is_note(model: &PreviewModel) -> bool {
    model.meta.as_ref().map(|m| m.entry_type) == Some(EntryType::Note)
}

impl RenderCache {
    pub fn with_limits(max_entries: usize, max_notes: usize, max_bytes: usize) -> RenderCache {
        RenderCache {
            map: HashMap::new(),
            order: VecDeque::new(),
            max_notes,
            max_bytes,
            max_entries,
            bytes: 0,
            notes: 0,
        }
    }

    fn touch(&mut self, key: &Key) {
        if let Some(i) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(i).expect("a position that exists");
            self.order.push_back(k);
        }
    }

    /// The preview of `id` as it was when the file had modification time `mtime`. Viewing it
    /// makes it the most recently viewed.
    pub fn get(&mut self, id: &str, mtime: i64) -> Option<&PreviewModel> {
        let key = (id.to_string(), mtime);
        if !self.map.contains_key(&key) {
            return None;
        }
        self.touch(&key);
        self.map.get(&key).map(|(m, _)| m)
    }

    pub fn put(&mut self, model: PreviewModel) {
        let key = (model.id.clone(), model.mtime);
        self.remove(&key);
        let size = model.approx_bytes();
        self.bytes += size;
        self.notes += usize::from(is_note(&model));
        self.map.insert(key.clone(), (model, size));
        self.order.push_back(key);
        // the newest preview is never the one dropped
        while self.order.len() > 1
            && (self.order.len() > self.max_entries
                || self.notes > self.max_notes
                || self.bytes > self.max_bytes)
        {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            self.forget(&old);
        }
    }

    fn remove(&mut self, key: &Key) {
        if self.map.contains_key(key) {
            self.order.retain(|k| k != key);
            self.forget(key);
        }
    }

    fn forget(&mut self, key: &Key) {
        if let Some((model, size)) = self.map.remove(key) {
            self.bytes -= size;
            self.notes -= usize::from(is_note(&model));
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// How many notes are held.
    pub fn notes(&self) -> usize {
        self.notes
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::preview::Meta;

    fn model(id: &str, mtime: i64, note: bool, text_bytes: usize) -> PreviewModel {
        let blocks = if text_bytes == 0 {
            Vec::new()
        } else {
            vec![crate::tui::markdown::Block::Paragraph(vec![
                crate::tui::highlight::Span::new(
                    "x".repeat(text_bytes),
                    crate::tui::theme::Class::Plain,
                ),
            ])]
        };
        PreviewModel {
            id: id.to_string(),
            mtime,
            meta: Some(Meta {
                entry_type: if note {
                    EntryType::Note
                } else {
                    EntryType::Cmd
                },
                tags: Vec::new(),
                use_count: 0,
                edited: None,
                unreviewed: false,
            }),
            notes: crate::tui::markdown::Notes::new(crate::tui::markdown::Rendered {
                blocks,
                links: Vec::new(),
            }),
            ..PreviewModel::default()
        }
    }

    #[test]
    fn the_key_is_the_entry_and_its_modification_time() {
        let mut c = RenderCache::default();
        c.put(model("a", 1, false, 0));
        assert!(c.get("a", 1).is_some() && c.get("a", 2).is_none() && c.get("b", 1).is_none());
        assert!(!c.is_empty());
    }

    #[test]
    fn at_most_twenty_notes_and_the_least_recently_viewed_go_first() {
        let mut c = RenderCache::default();
        for i in 0..20 {
            c.put(model(&format!("n{i}"), 1, true, 10));
        }
        assert_eq!(c.notes(), 20);
        // viewing n0 makes n1 the oldest
        assert!(c.get("n0", 1).is_some());
        c.put(model("n20", 1, true, 10));
        assert_eq!(c.notes(), 20);
        assert!(c.get("n0", 1).is_some(), "recently viewed stays");
        assert!(c.get("n1", 1).is_none(), "the least recently viewed went");
        assert!(c.get("n20", 1).is_some());
    }

    #[test]
    fn sixteen_megabytes_of_text_is_the_other_limit() {
        let mut c = RenderCache::default();
        let big = 6 * 1024 * 1024;
        for i in 0..4 {
            c.put(model(&format!("big{i}"), 1, true, big));
            assert!(c.bytes() <= MAX_BYTES, "{} bytes", c.bytes());
        }
        assert!(c.notes() < 4 && c.notes() >= 1, "{}", c.notes());
        assert!(c.get("big3", 1).is_some(), "the newest is kept");
        // a single note over the limit is still kept: it is the one being viewed
        let mut c = RenderCache::default();
        c.put(model("huge", 1, true, MAX_BYTES + 1));
        assert!(c.get("huge", 1).is_some());
    }

    #[test]
    fn other_entries_do_not_count_as_notes_but_are_bounded_too() {
        let mut c = RenderCache::default();
        for i in 0..100 {
            c.put(model(&format!("e{i}"), 1, false, 0));
        }
        assert_eq!(c.len(), MAX_ENTRIES);
        assert_eq!(c.notes(), 0);
        assert!(c.get("e0", 1).is_none() && c.get("e99", 1).is_some());
        c.put(model("e99", 1, false, 0));
        assert_eq!(c.len(), MAX_ENTRIES, "replacing does not grow it");
    }
}
