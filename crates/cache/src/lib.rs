//! 可插拔缓存层。
//!
//! 默认内存缓存（零依赖），可选 Redis（高并发/分布式）。
//! 权威数据在 SQLite，缓存仅加速热路径。
//!
//! - token 映射（鉴权热点）
//! - 余额（前置预筛）
//! - 限流计数（滑动窗口）

pub mod backend;
pub mod memory;
pub mod redis_backend;
pub mod connection;
pub mod token_cache;
pub mod balance_cache;
pub mod rate_cache;

pub use backend::CacheBackend;
pub use memory::MemoryCache;
pub use redis_backend::RedisCache;
pub use connection::{connect, from_backend, memory as memory_store, CacheStore};
pub use token_cache::{TokenInfo, TokenCache};
pub use balance_cache::BalanceCache;
pub use rate_cache::RateLimiter;

/// 缓存错误类型。
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
