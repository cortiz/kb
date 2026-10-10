//! Git snippet extraction.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeSnippet {
    pub lines: Vec<(usize, String)>,
    pub target_line: usize,
    pub target_end_line: Option<usize>,
}
