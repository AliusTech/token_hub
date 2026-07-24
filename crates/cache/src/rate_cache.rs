//! 限流（滑动窗口，原子自增 + 过期）。
//!
//! key: `ratelimit:<scope>:<id>`  value: 计数
//! 每窗口（默认 60s）内最多 max_count 次。超限返回 false。

use crate::CacheStore;

#[derive(Clone)]
pub struct RateLimiter {
    store: CacheStore,
}

impl RateLimiter {
    pub fn new(store: CacheStore) -> Self {
        Self { store }
    }

    /// 检查并计数。返回是否允许。
    /// key 如 `ratelimit:token:<token_hash>` 或 `ratelimit:ip:<ip>`。
    pub async fn check(
        &self,
        key: &str,
        max_count: u32,
        window_secs: u64,
    ) -> Result<bool, crate::CacheError> {
        let full_key = format!("ratelimit:{key}");
        let count = self.store.incr_with_expire(&full_key, 1, window_secs).await?;
        Ok(count <= max_count as i64)
    }
}
