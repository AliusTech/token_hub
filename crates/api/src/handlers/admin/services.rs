//! Service 账号管理接口。
//!
//! GET    /v1/admin/services              — 列表
//! POST   /v1/admin/services              — 创建（client_id + client_secret 明文仅返回一次）
//! PUT    /v1/admin/services/{id}         — 修改（scope/ip 白名单/状态）
//! DELETE /v1/admin/services/{id}         — 停用
//! POST   /v1/admin/services/{id}/reset-secret — 重置 client_secret

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireAdmin;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateServiceReq {
    pub name: String,
    pub scopes: Vec<String>,
    #[serde(default)]
    pub ip_whitelist: Option<Vec<String>>,
}

/// GET /v1/admin/services
pub async fn list_services(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ServicesRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let svc_repo = storage::ServiceAccountRepo::new(state.inner.store.clone());
    let services = svc_repo.list(100, 0).await?;
    Ok(Json(serde_json::json!({ "data": services })))
}

/// POST /v1/admin/services — 创建，client_secret 明文仅返回一次
pub async fn create_service(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Json(req): Json<CreateServiceReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ServicesWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let id = format!("svc_{}", Uuid::new_v4());
    let client_id = id.clone();
    // 生成 client_secret 明文
    let client_secret_plain = format!("scs_{}", auth::generate_session_token());
    let client_secret_hash = auth::hash_api_token(&client_secret_plain, state.secret());

    let scopes_refs: Vec<&str> = req.scopes.iter().map(|s| s.as_str()).collect();
    let ip_refs: Option<Vec<&str>> = req
        .ip_whitelist
        .as_ref()
        .map(|ips| ips.iter().map(|s| s.as_str()).collect());

    let svc_repo = storage::ServiceAccountRepo::new(state.inner.store.clone());
    let record = svc_repo
        .create(
            &id,
            &client_id,
            &client_secret_hash,
            &req.name,
            &scopes_refs,
            ip_refs.as_deref(),
            now,
        )
        .await?;

    // 审计
    let _ = state
        .inner
        .audit_repo
        .insert(
            "admin",
            Some(&principal.id),
            "service.create",
            Some("service"),
            Some(&record.id),
            None,
            None,
            now,
        )
        .await;

    Ok(Json(serde_json::json!({
        "id": record.id,
        "client_id": record.client_id,
        "client_secret": client_secret_plain,  // 仅此一次
        "name": record.name,
        "scopes": record.scopes,
    })))
}

#[derive(Debug, Deserialize)]
pub struct UpdateServiceReq {
    pub scopes: Option<Vec<String>>,
    pub ip_whitelist: Option<Vec<String>>,
    pub status: Option<String>,
}

/// PUT /v1/admin/services/{id}
pub async fn update_service(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
    Json(req): Json<UpdateServiceReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ServicesWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let svc_repo = storage::ServiceAccountRepo::new(state.inner.store.clone());
    if let Some(status) = &req.status {
        let _ = svc_repo.set_status(&id, status, now).await?;
    }
    if req.scopes.is_some() || req.ip_whitelist.is_some() {
        let scopes_refs: Vec<&str> = req
            .scopes
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let ip_refs: Option<Vec<&str>> = req
            .ip_whitelist
            .as_ref()
            .map(|ips| ips.iter().map(|s| s.as_str()).collect());
        let _ = svc_repo
            .update_scopes_ips(&id, &scopes_refs, ip_refs.as_deref(), now)
            .await?;
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

/// DELETE /v1/admin/services/{id}
pub async fn delete_service(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ServicesWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let svc_repo = storage::ServiceAccountRepo::new(state.inner.store.clone());
    let _ = svc_repo.set_status(&id, "disabled", now).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// POST /v1/admin/services/{id}/reset-secret
pub async fn reset_service_secret(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ServicesWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let new_secret_plain = format!("scs_{}", auth::generate_session_token());
    let new_secret_hash = auth::hash_api_token(&new_secret_plain, state.secret());
    let svc_repo = storage::ServiceAccountRepo::new(state.inner.store.clone());
    let _ = svc_repo.reset_secret(&id, &new_secret_hash, now).await?;
    Ok(Json(serde_json::json!({
        "client_secret": new_secret_plain,  // 仅此一次
    })))
}
