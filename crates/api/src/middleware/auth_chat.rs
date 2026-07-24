//! Chat 端认证：解析 API Token → API User Principal。
//!
//! 流程：
//! 1. 从 Authorization: Bearer <token> 提取明文 token
//! 2. HMAC hash → Redis 查缓存（命中则直接拿 account_id）
//! 3. 未命中 → SQLite 查 api_tokens 表 → 回填缓存
//! 4. 构造 Principal(kind=ApiUser, account_id)

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use domain::{Principal, PrincipalKind};

/// 提取器：要求有效的 API Token，产出 Principal。
pub struct RequireApiUser(pub Principal);

#[axum::async_trait]
impl FromRequestParts<AppState> for RequireApiUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_bearer(parts)?;
        let principal = authenticate_api_token(&token, state).await?;
        Ok(RequireApiUser(principal))
    }
}

/// 从 Authorization 头提取 Bearer token。
pub fn extract_bearer(parts: &Parts) -> ApiResult<String> {
    let auth_header = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::Unauthorized("missing authorization header".into()))?;
    let token = auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))
        .ok_or_else(|| ApiError::Unauthorized("invalid authorization scheme".into()))?;
    Ok(token.trim().to_string())
}

/// 认证 API Token：缓存优先，回源 SQLite。
pub async fn authenticate_api_token(token: &str, state: &AppState) -> ApiResult<Principal> {
    let token_hash = auth::hash_api_token(token, state.secret());

    // 1. 尝试缓存（内存或 Redis）
    let token_cache = cache::TokenCache::new(state.inner.cache.clone());
    if let Ok(Some(info)) = token_cache.get(&token_hash).await {
        if info.status != "active" {
            return Err(ApiError::Unauthorized("token revoked".into()));
        }
        return Ok(Principal {
            kind: PrincipalKind::ApiUser,
            id: info.account_id.clone(),
            account_id: Some(info.account_id),
            scopes: vec![],
        });
    }

    // 2. 回源 SQLite
    let api_token = state
        .inner
        .token_repo
        .find_by_hash(&token_hash)
        .await?
        .ok_or_else(|| ApiError::Unauthorized("invalid token".into()))?;

    if api_token.status != domain::TokenStatus::Active {
        return Err(ApiError::Unauthorized("token revoked".into()));
    }
    // 检查过期
    if let Some(exp) = api_token.expires_at {
        let now = chrono::Utc::now().timestamp_millis();
        if exp < now {
            return Err(ApiError::Unauthorized("token expired".into()));
        }
    }

    let account_id = api_token.account_id.clone();

    // 3. 回填缓存
    let token_cache = cache::TokenCache::new(state.inner.cache.clone());
    let _ = token_cache
        .set(
            &token_hash,
            &cache::TokenInfo {
                account_id: account_id.clone(),
                allowed_models: vec![],
                status: "active".to_string(),
            },
        )
        .await;

    Ok(Principal {
        kind: PrincipalKind::ApiUser,
        id: account_id.clone(),
        account_id: Some(account_id),
        scopes: vec![],
    })
}
