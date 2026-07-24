//! 调用日志与计量来源标记。

use serde::{Deserialize, Serialize};

/// token 用量来源（计费权威性标记）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageSource {
    /// 上游响应的 usage 字段（权威，主路径）
    Upstream,
    /// 上游无 usage，tiktoken 兜底估算（低频容错）
    TiktokenFallback,
}

impl UsageSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::TiktokenFallback => "tiktoken_fallback",
        }
    }
    pub fn from_db(s: &str) -> Self {
        match s {
            "tiktoken_fallback" => Self::TiktokenFallback,
            _ => Self::Upstream,
        }
    }
}

/// 调用日志（事实表，每请求一条；account_id + provider_id 双标签用于多维度聚合）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLog {
    pub id: String,
    pub account_id: String,
    pub logical_model: String,
    pub provider_id: Option<String>,
    pub upstream_model: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub credits_cost: i64,
    pub usage_source: UsageSource,
    /// success / failed / fallback
    pub status: String,
    pub source_ip: Option<String>,
    pub created_at: i64,
}
