//! API Token（应用用户的静态 Bearer 凭证）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum TokenStatus {
    #[default]
    Active,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: String,
    /// HMAC-SHA256(token, server_secret)，用于查表
    pub token_hash: String,
    /// 明文前 8 位，前端展示识别用
    pub prefix: String,
    pub account_id: String,
    pub name: Option<String>,
    pub status: TokenStatus,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
}

/// Token 明文前缀（生成时拼接，如 `th_live_` + 随机串）
pub const TOKEN_PREFIX_LITERAL: &str = "th_live_";

/// 计算明文 token 用于展示的 prefix（前 8 位，含前缀字面量）。
pub fn display_prefix(plaintext: &str) -> String {
    plaintext.chars().take(8).collect()
}
