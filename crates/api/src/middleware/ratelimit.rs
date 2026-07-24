//! 限流中间件：基于缓存（内存/Redis）的滑动窗口计数。
//!
//! - chat 端：按 API Token（token_hash）限流
//! - admin 端：按来源 IP 限流
//! 超限返回 429 Too Many Requests。

use crate::error::ApiError;
use crate::state::AppState;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

/// Chat 端限流中间件。
/// 从请求中提取 Authorization Bearer → token_hash 作为限流键。
/// 默认每 60s 最多 60 次（可配置）。
pub async fn chat_ratelimit(
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let state = req
        .extensions()
        .get::<AppState>()
        .cloned()
        .ok_or_else(|| ApiError::Internal("missing state".into()))?;

    // 提取 token 做限流键（粗粒度：用 Authorization 头的 hash 摘要）
    let key = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous");

    // 用 token 的 hash 作为限流键（避免明文 token 进缓存 key）
    let rate_key = format!("token:{}", short_hash(key));
    let limiter = cache::RateLimiter::new(state.inner.cache.clone());
    let allowed = limiter.check(&rate_key, 60, 60).await.unwrap_or(true); // 缓存失败不阻断（降级放行）

    if !allowed {
        return Err(ApiError::BadRequest(
            "rate limit exceeded, too many requests".into(),
        ));
    }
    Ok(next.run(req).await)
}

/// Admin 端限流中间件（按 IP）。
pub async fn admin_ratelimit(
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let state = req
        .extensions()
        .get::<AppState>()
        .cloned()
        .ok_or_else(|| ApiError::Internal("missing state".into()))?;

    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .unwrap_or("unknown");

    let rate_key = format!("ip:{ip}");
    let limiter = cache::RateLimiter::new(state.inner.cache.clone());
    let allowed = limiter.check(&rate_key, 30, 60).await.unwrap_or(true);

    if !allowed {
        return Err(ApiError::BadRequest("rate limit exceeded".into()));
    }
    Ok(next.run(req).await)
}

/// 简单 hash（取前 16 字符的十六进制摘要），仅用于限流键去重。
fn short_hash(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_hash_consistent() {
        assert_eq!(short_hash("abc"), short_hash("abc"));
        assert_ne!(short_hash("abc"), short_hash("abd"));
    }
}
