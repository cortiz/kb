//! Core library for kb: the entry file format, its lossless parser, validation, and search.

pub mod body;
pub mod config;
pub mod edit;
pub mod entry;
pub mod entry_json;
pub mod fold;
pub mod frontmatter;
pub mod index;
pub mod init;
pub mod lint;
pub mod payload;
pub mod query;
pub mod root;
pub mod search;
pub mod source;
pub mod usage;
pub mod vars;
pub mod walk;
pub mod yaml;
