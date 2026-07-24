//! 供应商凭证与状态。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderStatus {
    /// 可用
    Active,
    /// 已禁用（额度耗尽/欠费自动标记 或 管理员手动禁用，不自动恢复）
    Disabled,
}

impl Default for ProviderStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// 供应商凭证。api_key 加密存储；额度监控字段用于 80% 告警。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCredential {
    pub id: String,
    pub name: String,
    /// openai/anthropic/gemini/custom
    pub provider_type: String,
    pub base_url: String,
    /// 加密后的 api_key（序列化给前端时应脱敏）
    pub api_key_enc: String,
    pub status: ProviderStatus,
    pub disabled_reason: Option<String>,
    pub disabled_at: Option<i64>,
    /// 额度上限
    pub quota_limit: Option<i64>,
    /// 平台累加的已用量
    pub quota_used: i64,
    /// 告警阈值（百分比），默认 80
    pub quota_threshold: i32,
    /// 是否已发过告警（防重复）
    pub quota_alert_sent: bool,
    pub quota_synced_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 返回给前端的脱敏视图（不暴露 api_key）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCredentialView {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key_masked: String,
    pub status: ProviderStatus,
    pub disabled_reason: Option<String>,
    pub disabled_at: Option<i64>,
    pub quota_limit: Option<i64>,
    pub quota_used: i64,
    pub quota_threshold: i32,
    pub quota_alert_sent: bool,
    pub created_at: i64,
    pub updated_at: i64,
}
