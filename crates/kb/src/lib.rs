//! The `kb` command line and terminal UI, as a library so integration tests can reach the
//! pieces (snapshot tests draw screens without a terminal).

pub mod author;
pub mod color;
pub mod commands;
pub mod doctor;
pub mod editfile;
pub mod output;
pub mod platform;
pub mod show;
pub mod tui;
pub mod varcli;
