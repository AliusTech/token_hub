//! HTTP handler 模块。

pub mod chat;
pub mod system;
pub mod admin;

pub use chat::{chat_completions, list_models, get_usage};
pub use system::health;
