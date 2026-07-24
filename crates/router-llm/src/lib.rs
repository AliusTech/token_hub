//! 模型路由：分级选择 / 上游代理 / SSE 流式 / 自动降级 / 熔断。
//!
//! 调用策略：provider 标 level（1/2/3），sequential 按 level 升序、random 按 weight 加权。
//! 额度耗尽（429/402/配额错误）→ 标记 provider disabled → 自动 fallback 到下一个。

pub mod selector;
pub mod client;
pub mod fallback;

pub use selector::{RouteCandidate, RouteSelector, RouteDecision};
pub use client::{UpstreamClient, ChatRequest, ChatResponse, ChatMessage, ChatChoice, ResponseUsage, UpstreamError};
pub use fallback::{QuotaErrorDetector, FallbackOutcome};
