//! 计费核心：汇率计算 / tiktoken 估算 / 计量来源决策。
//!
//! 计费权威：上游 `usage` 为准；tiktoken 仅用于预冻结估算 + 无 usage 兜底。
//! 积分整数运算：credits = tokens * rate_per_1k / 1000。

pub mod rates;
pub mod estimator;
pub mod usage_source;

pub use rates::{compute_credits, UsageTokens};
pub use estimator::estimate_prompt_tokens;
pub use usage_source::{resolve_usage, ResolvedUsage};
