//! API Token 管理接口：列表 / 创建 / 吊销。
//!
//! GET    /v1/admin/tokens          — 列出 token（可选 account_id 过滤；token_hash 脱敏）
//! POST   /v1/admin/tokens          — 创建 token（明文仅此一次返回）
//! DELETE /v1/admin/tokens/{id}     — 吊销 token

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireAdmin;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use domain::token::display_prefix;
use domain::Scope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// GET /v1/admin/tokens 的查询参数。
#[derive(Debug, Deserialize)]
pub struct TokensListParams {
    pub account_id: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default = "default_offset")]
    pub offset: i64,
}

fn default_limit() -> i64 {
    20
}
fn default_offset() -> i64 {
    0
}

/// POST /v1/admin/tokens 的请求体。
#[derive(Debug, Deserialize)]
pub struct CreateTokenReq {
    pub account_id: String,
    pub name: Option<String>,
}

/// 列表项：对 token_hash 做脱敏展示。
#[derive(Debug, Serialize)]
pub struct TokenListItem {
    pub id: String,
    pub token_hash: String,
    pub prefix: String,
    pub account_id: String,
    pub name: Option<String>,
    pub status: String,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
}

/// GET /v1/admin/tokens
pub async fn list_tokens(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Query(params): Query<TokensListParams>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::TokensRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let tokens = if let Some(account_id) = params.account_id.as_deref() {
        state.inner.token_repo.list_by_account(account_id).await?
    } else {
        state
            .inner
            .token_repo
            .list_all(params.limit, params.offset)
            .await?
    };
    let data: Vec<TokenListItem> = tokens
        .into_iter()
        .map(|t| TokenListItem {
            id: t.id,
            // 脱敏：只保留 hash 前若干位，避免泄露可被比对的完整哈希。
            token_hash: mask_hash(&t.token_hash),
            prefix: t.prefix,
            account_id: t.account_id,
            name: t.name,
            status: match t.status {
                domain::TokenStatus::Active => "active".to_string(),
                domain::TokenStatus::Revoked => "revoked".to_string(),
            },
            expires_at: t.expires_at,
            created_at: t.created_at,
            revoked_at: t.revoked_at,
        })
        .collect();
    let total = data.len() as i64;
    Ok(Json(serde_json::json!({
        "data": data,
        "total": total,
    })))
}

/// POST /v1/admin/tokens — 创建 token，明文仅此一次返回。
pub async fn create_token(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Json(req): Json<CreateTokenReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::TokensWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    // 1. 校验账号存在
    let account = state
        .inner
        .account_repo
        .get(&req.account_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest("account not found".into()))?;

    // 2. 生成明文 + HMAC 哈希 + 展示前缀
    let plaintext = auth::generate_api_token();
    let hash = auth::hash_api_token(&plaintext, state.secret());
    let prefix = display_prefix(&plaintext);

    let now = chrono::Utc::now().timestamp();
    let id = format!("tok_{}", Uuid::new_v4());

    // 3. 落库
    state
        .inner
        .token_repo
        .create(
            &id,
            &hash,
            &prefix,
            &account.id,
            req.name.as_deref(),
            None,
            now,
        )
        .await?;

    // 明文仅此一次返回
    Ok(Json(serde_json::json!({
        "id": id,
        "token": plaintext,
        "account_id": account.id,
        "prefix": prefix,
    })))
}

/// DELETE /v1/admin/tokens/{id} — 按 id 吊销。
pub async fn revoke_token(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::TokensWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp();
    let revoked = state.inner.token_repo.revoke(&id, now).await?;
    if !revoked {
        return Err(ApiError::NotFound);
    }
    // 注：仅有 id 无法重建明文 hash，故 Redis 缓存失效需在 chat 认证路径按 hash 处理。
    Ok(Json(serde_json::json!({"ok": true})))
}

/// 对 token_hash 做脱敏：保留前 8 位 + "…"。
fn mask_hash(hash: &str) -> String {
    let prefix: String = hash.chars().take(8).collect();
    format!("{prefix}…")
}
