//! 策略模板（可用模型集、月度上限）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub name: String,
    /// 可用逻辑模型名列表
    pub allowed_models: Vec<String>,
    /// 月度积分上限（可选）
    pub monthly_credit_cap: Option<i64>,
    pub description: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
