//! 逻辑模型、模型-供应商映射、路由策略。

use serde::{Deserialize, Serialize};

/// 逻辑模型（basic/standard/expert），聚合多个供应商实例。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalModel {
    pub id: String,
    pub logical_name: String,
    pub description: Option<String>,
    /// 每 1000 input token 对应积分
    pub input_rate_per_1k: i64,
    /// 每 1000 output token 对应积分
    pub output_rate_per_1k: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum RoutingStrategy {
    /// 按 level 升序顺次尝试
    #[default]
    Sequential,
    /// 按 weight 加权随机
    Random,
}

/// 逻辑模型 ↔ 供应商映射（每个映射带 level/weight/strategy）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProvider {
    pub id: String,
    pub logical_model_id: String,
    pub provider_id: String,
    /// 上游真实模型名（gpt-4o / claude-3-5-sonnet）
    pub upstream_model: String,
    /// 1/2/3，供应商分级
    pub level: i32,
    pub weight: i32,
    pub strategy: RoutingStrategy,
    pub enabled: bool,
    pub created_at: i64,
}
