//! 认证 + 限流中间件。
//!
//! 认证两条路径：
//! - chat 端：API Token（HMAC hash 查表/缓存）
//! - admin 端：Admin 有状态 token / Service JWT / Device 凭证

pub mod auth_chat;
pub mod auth_admin;
pub mod ratelimit;

pub use auth_chat::RequireApiUser;
pub use auth_admin::RequireAdmin;
