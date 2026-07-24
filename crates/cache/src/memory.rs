//! 内存缓存实现（默认后端）。
//!
//! 基于 DashMap，值带可选过期时间。读时惰性清理过期项。
//! 永远不失败（纯内存操作），适合单机 / Agent 桌面模式。

use crate::backend::CacheBackend;
use crate::CacheError;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 内存缓存条目。
struct Entry {
    value: String,
    expires_at: Option<Instant>,
}

/// 内存缓存（DashMap 包装，Arc 共享）。
#[derive(Clone)]
pub struct MemoryCache {
    map: Arc<DashMap<String, Entry>>,
}

impl MemoryCache {
    pub fn new() -> Self {
        Self {
            map: Arc::new(DashMap::new()),
        }
    }

    /// 检查并清理过期项（惰性：读时检查单条；可选主动扫描）。
    fn is_expired(entry: &Entry) -> bool {
        entry
            .expires_at
            .map(|t| Instant::now() >= t)
            .unwrap_or(false)
    }
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl CacheBackend for MemoryCache {
    async fn get_string(&self, key: &str) -> Result<Option<String>, CacheError> {
        // 先清理过期项
        self.map.remove_if(key, |_, v| Self::is_expired(v));
        // 读值
        Ok(self.map.get(key).map(|e| e.value.clone()))
    }

    async fn set_string(
        &self,
        key: &str,
        value: &str,
        ttl: Option<Duration>,
    ) -> Result<(), CacheError> {
        let expires_at = ttl.map(|d| Instant::now() + d);
        self.map.insert(
            key.to_string(),
            Entry {
                value: value.to_string(),
                expires_at,
            },
        );
        Ok(())
    }

    async fn del(&self, key: &str) -> Result<(), CacheError> {
        self.map.remove(key);
        Ok(())
    }

    async fn incr_with_expire(
        &self,
        key: &str,
        delta: i64,
        window_secs: u64,
    ) -> Result<i64, CacheError> {
        let now = Instant::now();
        let window = Duration::from_secs(window_secs);

        // 先清理过期项（若已过期则移除，使后续 entry 视为新窗口）
        let expired = self
            .map
            .remove_if(key, |_, v| v.expires_at.map(|t| now >= t).unwrap_or(false));
        let is_new_window = expired.is_some() || !self.map.contains_key(key);

        let mut entry = self.map.entry(key.to_string()).or_insert(Entry {
            value: "0".to_string(),
            expires_at: Some(now + window),
        });
        if is_new_window {
            // 新窗口：从 delta 开始计数
            entry.value = delta.to_string();
            entry.expires_at = Some(now + window);
            Ok(delta)
        } else {
            let current: i64 = entry.value.parse().unwrap_or(0);
            let new_val = current + delta;
            entry.value = new_val.to_string();
            Ok(new_val)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn set_get_del() {
        let c = MemoryCache::new();
        assert_eq!(c.get_string("k").await.unwrap(), None);
        c.set_string("k", "v", None).await.unwrap();
        assert_eq!(c.get_string("k").await.unwrap(), Some("v".to_string()));
        c.del("k").await.unwrap();
        assert_eq!(c.get_string("k").await.unwrap(), None);
    }

    #[tokio::test]
    async fn ttl_expires() {
        let c = MemoryCache::new();
        c.set_string("k", "v", Some(Duration::from_millis(50)))
            .await
            .unwrap();
        assert_eq!(c.get_string("k").await.unwrap(), Some("v".to_string()));
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(c.get_string("k").await.unwrap(), None, "should expire");
    }

    #[tokio::test]
    async fn no_ttl_persists() {
        let c = MemoryCache::new();
        c.set_string("k", "v", None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(c.get_string("k").await.unwrap(), Some("v".to_string()));
    }

    #[tokio::test]
    async fn incr_with_expire_counts() {
        let c = MemoryCache::new();
        let v1 = c.incr_with_expire("rate", 1, 60).await.unwrap();
        let v2 = c.incr_with_expire("rate", 1, 60).await.unwrap();
        let v3 = c.incr_with_expire("rate", 1, 60).await.unwrap();
        assert_eq!(v1, 1);
        assert_eq!(v2, 2);
        assert_eq!(v3, 3);
    }

    #[tokio::test]
    async fn incr_window_expires() {
        let c = MemoryCache::new();
        // window_secs=1（1秒窗口）
        c.incr_with_expire("rate", 1, 1).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1200)).await;
        // 过期后重新计数（新窗口从 1 开始）
        let v = c.incr_with_expire("rate", 1, 1).await.unwrap();
        assert_eq!(v, 1, "should reset after window expiry, got {v}");
    }

    #[tokio::test]
    async fn shared_clone_sees_same_data() {
        let c1 = MemoryCache::new();
        let c2 = c1.clone();
        c1.set_string("k", "v", None).await.unwrap();
        assert_eq!(c2.get_string("k").await.unwrap(), Some("v".to_string()));
    }
}
