//! Model Context Protocol (MCP) server support for kb.

pub mod audit;
pub mod engine;
pub mod review;
pub mod types;
pub mod write;

pub use audit::log_mcp_operation;
pub use engine::McpEngine;
pub use review::ReviewStateTracker;
pub use types::*;
