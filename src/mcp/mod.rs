//! Local stdio MCP server for file-based HWP/HWPX editing.

pub mod fs_guard;
pub mod protocol;
pub mod session;
pub mod tools;

pub use protocol::{handle_json_value, run_stdio};
