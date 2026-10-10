//! Code link management: clone discovery, git diff drift detection, permalinks, and re-pinning.

pub mod clone;
pub mod drift;
pub mod permalink;
pub mod repin;
pub mod snippet;

pub use clone::{LocalClone, RemoteHost};
