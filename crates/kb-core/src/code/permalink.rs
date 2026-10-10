//! Web permalink generation for GitHub, GitLab, Bitbucket, and custom hosts.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermalinkError {
    InvalidRef(String),
    MissingTemplateField(String),
}

impl std::fmt::Display for PermalinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PermalinkError::InvalidRef(s) => write!(f, "invalid ref: {s}"),
            PermalinkError::MissingTemplateField(s) => write!(f, "missing template field: {s}"),
        }
    }
}

impl std::error::Error for PermalinkError {}
