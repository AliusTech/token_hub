//! HTTP handler 模块。

pub mod admin;
pub mod chat;
pub mod system;

pub use chat::{chat_completions, get_usage, list_models};
pub use system::health;
