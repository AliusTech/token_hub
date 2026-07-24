//! 可插拔缓存后端 trait。
//!
//! 两个实现：
//! - `MemoryCache`：默认，内存 DashMap + TTL，永远可用（零依赖）
//! - `RedisCache`：可选，Redis 可达时启用（分布式/高并发）
//!
//! 业务层通过 `CacheStore`（包装 `Arc<dyn CacheBackend>`）访问，
//! 不感知底层是内存还是 Redis。

use std::time::Duration;

/// 缓存后端接口。所有方法 best-effort，失败返回 Err（调用方可忽略降级回源）。
#[async_trait::async_trait]
pub trait CacheBackend: Send + Sync {
    /// 读取字符串值。未命中返回 Ok(None)。
    async fn get_string(&self, key: &str) -> Result<Option<String>, crate::CacheError>;

    /// 写入字符串值，带可选 TTL。
    async fn set_string(
        &self,
        key: &str,
        value: &str,
        ttl: Option<Duration>,
    ) -> Result<(), crate::CacheError>;

    /// 删除键。
    async fn del(&self, key: &str) -> Result<(), crate::CacheError>;

    /// 原子自增并设置过期窗口（限流用）。
    /// 首次自增时设置 window_secs 过期；返回自增后的值。
    async fn incr_with_expire(
        &self,
        key: &str,
        delta: i64,
        window_secs: u64,
    ) -> Result<i64, crate::CacheError>;
}
