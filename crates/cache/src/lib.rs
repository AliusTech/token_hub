//! 可插拔缓存层。
//!
//! 默认内存缓存（零依赖），可选 Redis（高并发/分布式）。
//! 权威数据在 SQLite，缓存仅加速热路径。
//!
//! - token 映射（鉴权热点）
//! - 余额（前置预筛）
//! - 限流计数（滑动窗口）

pub mod backend;
pub mod balance_cache;
pub mod connection;
pub mod memory;
pub mod rate_cache;
pub mod redis_backend;
pub mod token_cache;

pub use backend::CacheBackend;
pub use balance_cache::BalanceCache;
pub use connection::{connect, from_backend, memory as memory_store, CacheStore};
pub use memory::MemoryCache;
pub use rate_cache::RateLimiter;
pub use redis_backend::RedisCache;
pub use token_cache::{TokenCache, TokenInfo};

/// 缓存错误类型。
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
