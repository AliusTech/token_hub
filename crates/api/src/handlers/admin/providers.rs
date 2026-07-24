//! 供应商凭证 + 模型-供应商映射 + 额度 + 上游模型同步 接口。
//!
//! 供应商凭证：
//! GET    /v1/admin/providers                — 列表（api_key 脱敏）
//! POST   /v1/admin/providers                — 创建
//! PUT    /v1/admin/providers/{id}           — 修改
//! DELETE /v1/admin/providers/{id}           — 删除
//! POST   /v1/admin/providers/{id}/disable   — 禁用
//! POST   /v1/admin/providers/{id}/enable    — 启用
//! GET    /v1/admin/providers/{id}/usage     — 额度用量
//! POST   /v1/admin/providers/{id}/sync-quota — 用官方数据校准用量
//! POST   /v1/admin/providers/{id}/sync-models — 上游模型同步（MVP 桩）
//!
//! 模型-供应商映射：
//! GET    /v1/admin/model-providers          — 列表（可按 logical_model_id 过滤）
//! POST   /v1/admin/model-providers          — 创建
//! PUT    /v1/admin/model-providers/{id}     — 修改
//! DELETE /v1/admin/model-providers/{id}     — 删除

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireAdmin;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

// ================================ 供应商凭证 ================================

/// GET /v1/admin/providers — 列表（api_key 脱敏，不暴露真实密钥）
pub async fn list_providers(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let providers = state.inner.provider_repo.list().await?;
    let data: Vec<_> = providers.iter().map(mask_provider).collect();
    Ok(Json(serde_json::json!({ "data": data })))
}

#[derive(Debug, Deserialize)]
pub struct CreateProviderReq {
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub quota_limit: Option<i64>,
    #[serde(default = "default_threshold")]
    pub quota_threshold: i32,
}

fn default_threshold() -> i32 {
    80
}

/// POST /v1/admin/providers
pub async fn create_provider(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Json(req): Json<CreateProviderReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let id = format!("prov_{}", Uuid::new_v4());

    // TODO(encryption): 当前 api_key 明文存入 api_key_enc，后续接入 KMS/字段级加密后改为存密文。
    let record = state
        .inner
        .provider_repo
        .create(
            &id,
            &req.name,
            &req.provider_type,
            &req.base_url,
            &req.api_key,
            req.quota_limit,
            req.quota_threshold,
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
            "provider.create",
            Some("provider"),
            Some(&record.id),
            None,
            None,
            now,
        )
        .await;

    Ok(Json(mask_provider(&record)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateProviderReq {
    pub name: String,
    pub base_url: String,
    /// None 表示不更新密钥
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub quota_limit: Option<i64>,
    pub quota_threshold: i32,
}

/// PUT /v1/admin/providers/{id}
pub async fn update_provider(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
    Json(req): Json<UpdateProviderReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    // TODO(encryption): api_key 目前明文写入；后续加密后再落库。
    let updated = state
        .inner
        .provider_repo
        .update(
            &id,
            &req.name,
            &req.base_url,
            req.api_key.as_deref(),
            req.quota_limit,
            req.quota_threshold,
            now,
        )
        .await?;
    if !updated {
        return Err(ApiError::NotFound);
    }
    let _ = state
        .inner
        .audit_repo
        .insert(
            "admin",
            Some(&principal.id),
            "provider.update",
            Some("provider"),
            Some(&id),
            None,
            None,
            now,
        )
        .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// DELETE /v1/admin/providers/{id}
pub async fn delete_provider(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let deleted = state.inner.provider_repo.delete(&id).await?;
    if !deleted {
        return Err(ApiError::NotFound);
    }
    let _ = state
        .inner
        .audit_repo
        .insert(
            "admin",
            Some(&principal.id),
            "provider.delete",
            Some("provider"),
            Some(&id),
            None,
            None,
            now,
        )
        .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Debug, Deserialize)]
pub struct DisableProviderReq {
    #[serde(default)]
    pub reason: Option<String>,
}

/// POST /v1/admin/providers/{id}/disable
pub async fn disable_provider(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
    Json(req): Json<DisableProviderReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let updated = state
        .inner
        .provider_repo
        .set_status(&id, "disabled", req.reason.as_deref(), now)
        .await?;
    if !updated {
        return Err(ApiError::NotFound);
    }
    let _ = state
        .inner
        .audit_repo
        .insert(
            "admin",
            Some(&principal.id),
            "provider.disable",
            Some("provider"),
            Some(&id),
            None,
            None,
            now,
        )
        .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// POST /v1/admin/providers/{id}/enable
pub async fn enable_provider(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let updated = state
        .inner
        .provider_repo
        .set_status(&id, "active", None, now)
        .await?;
    if !updated {
        return Err(ApiError::NotFound);
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

/// GET /v1/admin/providers/{id}/usage
pub async fn get_provider_usage(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let (used, limit, threshold, alert_sent) = state
        .inner
        .quota_repo
        .get_quota(&id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let percentage = match limit {
        Some(l) if l > 0 => ((used as f64 / l as f64) * 100.0).round() as i64,
        _ => 0,
    };
    Ok(Json(serde_json::json!({
        "used": used,
        "limit": limit,
        "threshold": threshold,
        "alert_sent": alert_sent,
        "percentage": percentage,
    })))
}

#[derive(Debug, Deserialize)]
pub struct SyncQuotaReq {
    pub official_used: i64,
}

/// POST /v1/admin/providers/{id}/sync-quota — 用官方数据校准已用量
pub async fn sync_provider_quota(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
    Json(req): Json<SyncQuotaReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    // 先确认供应商存在
    if state.inner.provider_repo.get(&id).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    state
        .inner
        .quota_repo
        .sync_quota(&id, req.official_used, now)
        .await?;
    Ok(Json(serde_json::json!({"synced": true})))
}

/// POST /v1/admin/providers/{id}/sync-models — 上游模型同步（MVP 桩）
pub async fn sync_upstream_models(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    // 确认供应商存在
    if state.inner.provider_repo.get(&id).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    // TODO(upstream-sync): 调用 provider.base_url + "/v1/models" 拉取上游模型清单，
    // 落库到 upstream_models 表并由管理员手工建立 model-provider 映射。
    Ok(Json(serde_json::json!({
        "synced": 0,
        "note": "not yet implemented — manual labeling via upstream-models API",
    })))
}

// ================================ 模型-供应商映射 ================================

#[derive(Debug, Deserialize)]
pub struct MapListParams {
    #[serde(default)]
    pub logical_model_id: Option<String>,
}

/// GET /v1/admin/model-providers
pub async fn list_mappings(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Query(params): Query<MapListParams>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let mappings = if let Some(lm_id) = &params.logical_model_id {
        state
            .inner
            .mapping_repo
            .list_by_logical_model(lm_id)
            .await?
    } else {
        state.inner.mapping_repo.list_all().await?
    };
    Ok(Json(serde_json::json!({ "data": mappings })))
}

#[derive(Debug, Deserialize)]
pub struct CreateMapReq {
    pub logical_model_id: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub level: i32,
    pub weight: i32,
    pub strategy: String,
    pub enabled: bool,
}

/// POST /v1/admin/model-providers
pub async fn create_mapping(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Json(req): Json<CreateMapReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let record = state
        .inner
        .mapping_repo
        .create(
            &req.logical_model_id,
            &req.provider_id,
            &req.upstream_model,
            req.level,
            req.weight,
            &req.strategy,
            req.enabled,
            now,
        )
        .await?;
    Ok(Json(record))
}

#[derive(Debug, Deserialize)]
pub struct UpdateMapReq {
    pub level: i32,
    pub weight: i32,
    pub strategy: String,
    pub enabled: bool,
}

/// PUT /v1/admin/model-providers/{id}
pub async fn update_mapping(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
    Json(req): Json<UpdateMapReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let updated = state
        .inner
        .mapping_repo
        .update(&id, req.level, req.weight, &req.strategy, req.enabled, now)
        .await?;
    if !updated {
        return Err(ApiError::NotFound);
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

/// DELETE /v1/admin/model-providers/{id}
pub async fn delete_mapping(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ProvidersWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let deleted = state.inner.mapping_repo.delete(&id).await?;
    if !deleted {
        return Err(ApiError::NotFound);
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

// ================================ 辅助函数 ================================

/// 构造脱敏视图：api_key_enc 不直接暴露，替换为 "****"。
fn mask_provider(p: &domain::ProviderCredential) -> serde_json::Value {
    serde_json::json!({
        "id": p.id,
        "name": p.name,
        "provider_type": p.provider_type,
        "base_url": p.base_url,
        "api_key": "****",
        "status": p.status,
        "disabled_reason": p.disabled_reason,
        "disabled_at": p.disabled_at,
        "quota_limit": p.quota_limit,
        "quota_used": p.quota_used,
        "quota_threshold": p.quota_threshold,
        "quota_alert_sent": p.quota_alert_sent,
        "quota_synced_at": p.quota_synced_at,
        "created_at": p.created_at,
        "updated_at": p.updated_at,
    })
}
