//! 策略模板管理接口。
//!
//! GET    /v1/admin/policies              — 列表
//! POST   /v1/admin/policies              — 创建
//! PUT    /v1/admin/policies/{id}         — 修改
//! DELETE /v1/admin/policies/{id}         — 删除
//! POST   /v1/admin/accounts/{id}/policy  — 给账号绑定策略

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireAdmin;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct CreatePolicyReq {
    pub name: String,
    pub allowed_models: Vec<String>,
    pub monthly_credit_cap: Option<i64>,
    pub description: Option<String>,
}

/// GET /v1/admin/policies
pub async fn list_policies(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::PoliciesRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let repo = storage::PolicyRepo::new(state.inner.store.clone());
    let policies = repo.list().await?;
    Ok(Json(serde_json::json!({ "data": policies })))
}

/// POST /v1/admin/policies
pub async fn create_policy(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Json(req): Json<CreatePolicyReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::PoliciesWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let id = format!("pol_{}", Uuid::new_v4());
    let repo = storage::PolicyRepo::new(state.inner.store.clone());
    let models_refs: Vec<&str> = req.allowed_models.iter().map(|s| s.as_str()).collect();
    let policy = repo
        .create(&id, &req.name, &models_refs, req.monthly_credit_cap, req.description.as_deref(), now)
        .await?;
    Ok(Json(policy))
}

#[derive(Debug, Deserialize)]
pub struct UpdatePolicyReq {
    pub name: String,
    pub allowed_models: Vec<String>,
    pub monthly_credit_cap: Option<i64>,
    pub description: Option<String>,
}

/// PUT /v1/admin/policies/{id}
pub async fn update_policy(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
    Json(req): Json<UpdatePolicyReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::PoliciesWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let repo = storage::PolicyRepo::new(state.inner.store.clone());
    let models_refs: Vec<&str> = req.allowed_models.iter().map(|s| s.as_str()).collect();
    let _ = repo
        .update(&id, &req.name, &models_refs, req.monthly_credit_cap, req.description.as_deref(), now)
        .await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// DELETE /v1/admin/policies/{id}
pub async fn delete_policy(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::PoliciesWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let repo = storage::PolicyRepo::new(state.inner.store.clone());
    let _ = repo.delete(&id).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Debug, Deserialize)]
pub struct BindPolicyReq {
    pub policy_id: String,
}

/// POST /v1/admin/accounts/{id}/policy
pub async fn bind_account_policy(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(account_id): Path<String>,
    Json(req): Json<BindPolicyReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::PoliciesWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let repo = storage::AccountRepo::new(state.inner.store.clone());
    let ok = repo.set_policy(&account_id, Some(&req.policy_id), now).await?;
    if !ok {
        return Err(ApiError::NotFound);
    }
    Ok(Json(serde_json::json!({"ok": true})))
}
