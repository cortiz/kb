//! Authoring: detection, suggestions, drafts, canonical rendering and file operations (spec 007).
//!
//! Everything a front end needs to create an entry lives here, so the TUI form, `kb add` and the
//! editor template cannot disagree: they only collect input.

use std::time::SystemTime;

pub mod detect;
pub mod draft;
pub mod fsops;
pub mod render;
pub mod suggest;

pub use detect::{Confidence, Detection, detect, normalize_clipboard};
pub use draft::{Draft, DraftError, DraftFields, DraftOptions, DraftSource, PathChoice, VarDecl};
pub use suggest::{SuggestPattern, VariableSuggestion};

/// Where "now" comes from. Rendering and saving take it as an argument so tests can fix it.
pub trait Clock {
    fn now(&self) -> SystemTime;
}

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// A clock that always says the same time (tests, and `KB_TEST_NOW` in debug builds).
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub SystemTime);

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}
