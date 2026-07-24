//! Redis 缓存实现（可选后端）。
//!
//! 当 Redis 可达时启用，支持分布式/多实例场景。
//! 包装 redis::ConnectionManager，实现 CacheBackend trait。

use crate::backend::CacheBackend;
use crate::CacheError;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use std::time::Duration;

/// Redis 缓存。
#[derive(Clone)]
pub struct RedisCache {
    conn: ConnectionManager,
}

impl RedisCache {
    /// 连接 Redis。失败返回 Err（调用方可回退到 MemoryCache）。
    pub async fn new(redis_url: &str) -> Result<Self, CacheError> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }
}

#[async_trait::async_trait]
impl CacheBackend for RedisCache {
    async fn get_string(&self, key: &str) -> Result<Option<String>, CacheError> {
        let mut conn = self.conn.clone();
        let val: Option<String> = conn.get(key).await?;
        Ok(val)
    }

    async fn set_string(
        &self,
        key: &str,
        value: &str,
        ttl: Option<Duration>,
    ) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        match ttl {
            Some(d) => {
                let _: () = conn.set_ex(key, value, d.as_secs().max(1)).await?;
            }
            None => {
                let _: () = conn.set(key, value).await?;
            }
        }
        Ok(())
    }

    async fn del(&self, key: &str) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        let _: () = conn.del(key).await?;
        Ok(())
    }

    async fn incr_with_expire(
        &self,
        key: &str,
        delta: i64,
        window_secs: u64,
    ) -> Result<i64, CacheError> {
        let mut conn = self.conn.clone();
        let count: i64 = conn.incr(key, delta).await?;
        if count == delta.abs() || count == 1 {
            // 首次进入窗口（从 0 自增），设置过期
            let _: () = conn.expire(key, window_secs as i64).await?;
        }
        Ok(count)
    }
}
