//! Variable suggestions for literal values (FR-014 to FR-016, research R4).

use std::ops::Range;

use crate::vars::VarKind;

/// Which of the five patterns a suggestion came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestPattern {
    Env,
    Region,
    Time,
    Account,
    Id,
}

/// A literal value that could become a variable. Never applied on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableSuggestion {
    pub value: String,
    /// Byte ranges in the body; they cover the replaced part only (the digits for `Time`).
    pub positions: Vec<Range<usize>>,
    pub name: String,
    pub kind: VarKind,
    pub options: Vec<String>,
    pub default: String,
    pub pattern: SuggestPattern,
}

/// Literal values in `body` that could be variables. Names are unique against `declared` and each other.
pub fn suggest_vars(
    body: &str,
    declared: &[crate::author::draft::VarDecl],
) -> Vec<VariableSuggestion> {
    let _ = (body, declared);
    Vec::new()
}
