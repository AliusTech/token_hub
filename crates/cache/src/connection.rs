//! 缓存句柄：包装 `Arc<dyn CacheBackend>`，统一访问内存/Redis。

use crate::backend::CacheBackend;
use crate::memory::MemoryCache;
use crate::redis_backend::RedisCache;
use std::sync::Arc;
use std::time::Duration;

/// 统一缓存句柄。永远有值（至少是 MemoryCache）。
#[derive(Clone)]
pub struct CacheStore {
    backend: Arc<dyn CacheBackend>,
}

impl CacheStore {
    /// 读取字符串。
    pub async fn get_string(&self, key: &str) -> Result<Option<String>, crate::CacheError> {
        self.backend.get_string(key).await
    }

    /// 写入字符串（带可选 TTL）。
    pub async fn set_string(
        &self,
        key: &str,
        value: &str,
        ttl: Option<Duration>,
    ) -> Result<(), crate::CacheError> {
        self.backend.set_string(key, value, ttl).await
    }

    /// 删除键。
    pub async fn del(&self, key: &str) -> Result<(), crate::CacheError> {
        self.backend.del(key).await
    }

    /// 原子自增 + 设置过期窗口。
    pub async fn incr_with_expire(
        &self,
        key: &str,
        delta: i64,
        window_secs: u64,
    ) -> Result<i64, crate::CacheError> {
        self.backend.incr_with_expire(key, delta, window_secs).await
    }
}

/// 连接缓存：优先 Redis，失败/空 URL 回退 Memory。
/// **永远返回有效 CacheStore**（至少 Memory），绝不返回 Err。
///
/// 特殊值：
/// - 空 / "memory" / "none"：直接用内存缓存，不尝试 Redis
pub async fn connect(redis_url: &str) -> CacheStore {
    let normalized = redis_url.trim();
    if normalized.is_empty()
        || normalized == "memory"
        || normalized == "none"
        || normalized == "redis://"
    {
        tracing::info!("using memory cache (no Redis configured)");
        return CacheStore {
            backend: Arc::new(MemoryCache::new()),
        };
    }
    // 带超时尝试 Redis，避免不可达时无限阻塞
    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        RedisCache::new(normalized),
    )
    .await
    {
        Ok(Ok(redis)) => {
            tracing::info!("Redis cache connected");
            CacheStore {
                backend: Arc::new(redis),
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "Redis unavailable, falling back to memory cache");
            CacheStore {
                backend: Arc::new(MemoryCache::new()),
            }
        }
        Err(_) => {
            tracing::warn!("Redis connect timeout (3s), falling back to memory cache");
            CacheStore {
                backend: Arc::new(MemoryCache::new()),
            }
        }
    }
}

/// 构造纯内存缓存（测试/强制内存模式）。
pub fn memory() -> CacheStore {
    CacheStore {
        backend: Arc::new(MemoryCache::new()),
    }
}

/// 从任意后端构造（高级用法）。
pub fn from_backend(backend: Arc<dyn CacheBackend>) -> CacheStore {
    CacheStore { backend }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_store_never_fails() {
        let store = memory();
        store.set_string("k", "v", None).await.unwrap();
        assert_eq!(store.get_string("k").await.unwrap(), Some("v".to_string()));
    }

    #[tokio::test]
    async fn connect_empty_url_returns_memory() {
        let store = connect("").await;
        store.set_string("k", "v", None).await.unwrap();
        assert_eq!(store.get_string("k").await.unwrap(), Some("v".to_string()));
    }

    #[tokio::test]
    async fn connect_unreachable_redis_falls_back_to_memory() {
        // 不可达地址 + 超时会回退到 memory
        let store = connect("redis://192.0.2.1:6390").await;
        store.set_string("k", "v", None).await.unwrap();
        assert_eq!(store.get_string("k").await.unwrap(), Some("v".to_string()));
    }
}
