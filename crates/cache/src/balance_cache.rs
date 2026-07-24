//! 余额缓存（前置预筛 + 热读展示）。
//!
//! key: `balance:<account_id>`  value: 整数余额
//! TTL: 10s（短 TTL，结算后覆写；即使丢更新，下次乐观锁仍保证不超卖）。

use crate::CacheStore;
use std::time::Duration;

const TTL: Duration = Duration::from_secs(10);
const KEY_PREFIX: &str = "balance:";

#[derive(Clone)]
pub struct BalanceCache {
    store: CacheStore,
}

impl BalanceCache {
    pub fn new(store: CacheStore) -> Self {
        Self { store }
    }

    /// 读取缓存的余额。未命中返回 Ok(None)（回源 SQLite）。
    pub async fn get(&self, account_id: &str) -> Result<Option<i64>, crate::CacheError> {
        let val = self
            .store
            .get_string(&format!("{KEY_PREFIX}{account_id}"))
            .await?;
        Ok(val.and_then(|s| s.parse().ok()))
    }

    /// 写入/覆写余额（结算后调用）。
    pub async fn set(&self, account_id: &str, balance: i64) -> Result<(), crate::CacheError> {
        self.store
            .set_string(
                &format!("{KEY_PREFIX}{account_id}"),
                &balance.to_string(),
                Some(TTL),
            )
            .await
    }

    /// 作废余额缓存（admin 调整后）。
    pub async fn invalidate(&self, account_id: &str) -> Result<(), crate::CacheError> {
        self.store.del(&format!("{KEY_PREFIX}{account_id}")).await
    }
}
