//! 账号管理接口：列表 / 创建 / 查询 / 更新 / 停用。
//!
//! GET    /v1/admin/accounts        — 分页列出账号（可选 status 过滤）
//! POST   /v1/admin/accounts        — 创建账号（生成 id + 初始化 credits）
//! GET    /v1/admin/accounts/{id}   — 查询单个账号
//! PUT    /v1/admin/accounts/{id}   — 更新账号（状态 / 备注）
//! DELETE /v1/admin/accounts/{id}   — 停用账号（status=disabled）

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireAdmin;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use domain::Scope;
use serde::Deserialize;
use uuid::Uuid;

/// GET /v1/admin/accounts 的查询参数。
#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub status: Option<String>,
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

/// POST /v1/admin/accounts 的请求体。
#[derive(Debug, Deserialize)]
pub struct CreateAccountReq {
    pub external_id: Option<String>,
    /// MVP：暂未接入 policy 查找，先存为 note 的一部分。
    pub template: Option<String>,
    pub note: Option<String>,
}

/// PUT /v1/admin/accounts/{id} 的请求体。
#[derive(Debug, Deserialize)]
pub struct UpdateAccountReq {
    pub status: Option<String>,
    pub note: Option<String>,
}

/// GET /v1/admin/accounts
pub async fn list_accounts(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Query(params): Query<ListParams>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::AccountsRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let rows = state
        .inner
        .account_repo
        .list(params.status.as_deref(), params.limit, params.offset)
        .await?;
    let total = rows.len() as i64;
    Ok(Json(serde_json::json!({
        "data": rows,
        "total": total,
    })))
}

/// POST /v1/admin/accounts
pub async fn create_account(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Json(req): Json<CreateAccountReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::AccountsWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp();
    let id = format!("acct_{}", Uuid::new_v4());

    // template → policy_id 查找留待后续；MVP 暂并入 note。
    let note = match (req.note.as_deref(), req.template.as_deref()) {
        (Some(n), Some(t)) => Some(format!("{n} [template={t}]")),
        (Some(n), None) => Some(n.to_string()),
        (None, Some(t)) => Some(format!("[template={t}]")),
        (None, None) => None,
    };

    let account = state
        .inner
        .account_repo
        .create(&id, req.external_id.as_deref(), None, note.as_deref(), now)
        .await?;

    // 初始化 credits 账本
    state.inner.credits_repo.init(&account.id, now).await?;

    Ok(Json(serde_json::to_value(&account).unwrap_or_default()))
}

/// GET /v1/admin/accounts/{id}
pub async fn get_account(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::AccountsRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let account = state
        .inner
        .account_repo
        .get(&id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(serde_json::to_value(&account).unwrap_or_default()))
}

/// PUT /v1/admin/accounts/{id}
pub async fn update_account(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
    Json(req): Json<UpdateAccountReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::AccountsWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp();

    if let Some(status) = req.status.as_deref() {
        state
            .inner
            .account_repo
            .set_status(&id, status, now)
            .await?;
    }
    // TODO: AccountRepo 暂无 update_note；note 更新留待后续补齐。
    if req.note.is_some() {
        // MVP：忽略 note 更新（仓储层无对应方法）。
    }

    let account = state
        .inner
        .account_repo
        .get(&id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(serde_json::to_value(&account).unwrap_or_default()))
}

/// DELETE /v1/admin/accounts/{id} — 软删除：status=disabled。
pub async fn delete_account(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::AccountsWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp();
    let updated = state
        .inner
        .account_repo
        .set_status(&id, "disabled", now)
        .await?;
    if !updated {
        return Err(ApiError::NotFound);
    }
    Ok(Json(serde_json::json!({"ok": true})))
}
