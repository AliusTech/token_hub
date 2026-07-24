//! 积分管理接口：查询余额 / 调整余额 / 设置余额 / 流水。
//!
//! GET  /v1/admin/credits                  — 查询账号余额
//! POST /v1/admin/credits                  — 调整余额（delta，可正可负）
//! PUT  /v1/admin/credits/{account_id}     — 设置余额（按 target-current 计算 delta）
//! GET  /v1/admin/credits/transactions     — 查询账号流水（分页）

use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireAdmin;
use crate::state::AppState;
use domain::Scope;

/// GET /v1/admin/credits
#[derive(Debug, Deserialize)]
pub struct GetCreditsParams {
    pub account_id: Option<String>,
}

pub async fn get_credits(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Query(params): Query<GetCreditsParams>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::CreditsRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let account_id = params
        .account_id
        .ok_or_else(|| ApiError::BadRequest("account_id required".into()))?;
    let credits = state
        .inner
        .credits_repo
        .get(&account_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(serde_json::json!({
        "account_id": credits.account_id,
        "balance": credits.balance,
        "held": credits.held,
        "version": credits.version,
    })))
}

/// POST /v1/admin/credits
#[derive(Debug, Deserialize)]
pub struct AdjustReq {
    pub account_id: String,
    pub delta: i64,
    pub reason: Option<String>,
}

pub async fn adjust_credits(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Json(req): Json<AdjustReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::CreditsWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp();
    let new_balance = state
        .inner
        .credits_repo
        .adjust(
            &req.account_id,
            req.delta,
            req.reason.as_deref(),
            Some(&principal.id),
            now,
        )
        .await?
        .ok_or_else(|| ApiError::BadRequest("insufficient balance for negative delta".into()))?;

    // 审计
    let detail = serde_json::json!({
        "account_id": req.account_id,
        "delta": req.delta,
        "reason": req.reason,
        "balance_after": new_balance,
    })
    .to_string();
    let _ = state
        .inner
        .audit_repo
        .insert(
            "admin",
            Some(&principal.id),
            "credits.adjust",
            Some("account"),
            Some(&req.account_id),
            Some(&detail),
            None,
            now,
        )
        .await;

    Ok(Json(serde_json::json!({ "balance": new_balance })))
}

/// PUT /v1/admin/credits/{account_id}
#[derive(Debug, Deserialize)]
pub struct SetReq {
    pub balance: i64,
    pub reason: Option<String>,
}

pub async fn set_credits(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(account_id): Path<String>,
    Json(req): Json<SetReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::CreditsWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp();

    // 读取当前余额，计算 delta
    let current = state
        .inner
        .credits_repo
        .get(&account_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let delta = req.balance - current.balance;
    let new_balance = state
        .inner
        .credits_repo
        .adjust(
            &account_id,
            delta,
            req.reason.as_deref(),
            Some(&principal.id),
            now,
        )
        .await?
        .ok_or_else(|| ApiError::BadRequest("insufficient balance for negative delta".into()))?;

    // 审计
    let detail = serde_json::json!({
        "account_id": account_id,
        "from": current.balance,
        "to": req.balance,
        "delta": delta,
        "reason": req.reason,
    })
    .to_string();
    let _ = state
        .inner
        .audit_repo
        .insert(
            "admin",
            Some(&principal.id),
            "credits.set",
            Some("account"),
            Some(&account_id),
            Some(&detail),
            None,
            now,
        )
        .await;

    Ok(Json(serde_json::json!({ "balance": new_balance })))
}

/// GET /v1/admin/credits/transactions
#[derive(Debug, Deserialize)]
pub struct TxParams {
    pub account_id: String,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

pub async fn list_transactions(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Query(params): Query<TxParams>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::CreditsRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let txs = state
        .inner
        .credits_repo
        .transactions(&params.account_id, params.limit, params.offset)
        .await?;
    Ok(Json(serde_json::json!({ "data": txs })))
}
