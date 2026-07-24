//! API Token → account 映射缓存（鉴权热路径）。
//!
//! key: `token:<hmac_hash>`  value: JSON {account_id, scopes, model_levels_allowed, status}
//! TTL: 10 分钟。吊销时 DEL。

use crate::CacheStore;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub account_id: String,
    /// 允许的逻辑模型名（空表示全部允许，由策略进一步控制）
    pub allowed_models: Vec<String>,
    pub status: String,
}

#[derive(Clone)]
pub struct TokenCache {
    store: CacheStore,
}

const TTL: Duration = Duration::from_secs(600);
const KEY_PREFIX: &str = "token:";

impl TokenCache {
    pub fn new(store: CacheStore) -> Self {
        Self { store }
    }

    /// 读取缓存的 token 信息。未命中返回 Ok(None)。
    pub async fn get(&self, token_hash: &str) -> Result<Option<TokenInfo>, crate::CacheError> {
        let val = self
            .store
            .get_string(&format!("{KEY_PREFIX}{token_hash}"))
            .await?;
        match val {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None => Ok(None),
        }
    }

    /// 写入 token 映射。
    pub async fn set(&self, token_hash: &str, info: &TokenInfo) -> Result<(), crate::CacheError> {
        let s = serde_json::to_string(info)?;
        self.store
            .set_string(&format!("{KEY_PREFIX}{token_hash}"), &s, Some(TTL))
            .await
    }

    /// 吊销：删除缓存（即时失效）。
    pub async fn revoke(&self, token_hash: &str) -> Result<(), crate::CacheError> {
        self.store.del(&format!("{KEY_PREFIX}{token_hash}")).await
    }
}
