//! 模型与费率管理接口：列表 / 创建 / 更新 / 删除 / 设置费率。
//!
//! GET    /v1/admin/models                  — 列出全部逻辑模型
//! POST   /v1/admin/models                  — 创建逻辑模型
//! PUT    /v1/admin/models/{id}             — 更新逻辑模型
//! DELETE /v1/admin/models/{id}             — 删除逻辑模型
//! PUT    /v1/admin/models/{id}/rates       — 设置 input/output 费率

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireAdmin;
use crate::state::AppState;
use domain::Scope;

/// GET /v1/admin/models
pub async fn list_models_admin(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::ModelsRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let models = state.inner.model_repo.list().await?;
    Ok(Json(serde_json::json!({ "data": models })))
}

/// POST /v1/admin/models
#[derive(Debug, Deserialize)]
pub struct CreateModelReq {
    pub logical_name: String,
    pub description: Option<String>,
    pub input_rate_per_1k: i64,
    pub output_rate_per_1k: i64,
}

pub async fn create_model(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Json(req): Json<CreateModelReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::ModelsWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp();
    let id = format!("mdl_{}", Uuid::new_v4());
    let model = state
        .inner
        .model_repo
        .create(
            &id,
            &req.logical_name,
            req.description.as_deref(),
            req.input_rate_per_1k,
            req.output_rate_per_1k,
            now,
        )
        .await?;

    // 审计
    let detail = serde_json::json!({
        "id": model.id,
        "logical_name": model.logical_name,
        "input_rate_per_1k": model.input_rate_per_1k,
        "output_rate_per_1k": model.output_rate_per_1k,
    })
    .to_string();
    let _ = state
        .inner
        .audit_repo
        .insert(
            "admin",
            Some(&principal.id),
            "model.create",
            Some("model"),
            Some(&model.id),
            Some(&detail),
            None,
            now,
        )
        .await;

    Ok(Json(serde_json::json!({ "data": model })))
}

/// PUT /v1/admin/models/{id}
#[derive(Debug, Deserialize)]
pub struct UpdateModelReq {
    pub logical_name: String,
    pub description: Option<String>,
    pub input_rate_per_1k: i64,
    pub output_rate_per_1k: i64,
    pub status: String,
}

pub async fn update_model(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
    Json(req): Json<UpdateModelReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::ModelsWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp();
    let updated = state
        .inner
        .model_repo
        .update(
            &id,
            &req.logical_name,
            req.description.as_deref(),
            req.input_rate_per_1k,
            req.output_rate_per_1k,
            &req.status,
            now,
        )
        .await?;
    if !updated {
        return Err(ApiError::NotFound);
    }

    // 审计
    let detail = serde_json::json!({
        "id": id,
        "logical_name": req.logical_name,
        "description": req.description,
        "input_rate_per_1k": req.input_rate_per_1k,
        "output_rate_per_1k": req.output_rate_per_1k,
        "status": req.status,
    })
    .to_string();
    let _ = state
        .inner
        .audit_repo
        .insert(
            "admin",
            Some(&principal.id),
            "model.update",
            Some("model"),
            Some(&id),
            Some(&detail),
            None,
            now,
        )
        .await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// DELETE /v1/admin/models/{id}
pub async fn delete_model(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::ModelsWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp();
    let deleted = state.inner.model_repo.delete(&id).await?;
    if !deleted {
        return Err(ApiError::NotFound);
    }

    // 审计
    let _ = state
        .inner
        .audit_repo
        .insert(
            "admin",
            Some(&principal.id),
            "model.delete",
            Some("model"),
            Some(&id),
            None,
            None,
            now,
        )
        .await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// PUT /v1/admin/models/{id}/rates
#[derive(Debug, Deserialize)]
pub struct SetRatesReq {
    pub input_rate_per_1k: i64,
    pub output_rate_per_1k: i64,
}

pub async fn set_rates(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(id): Path<String>,
    Json(req): Json<SetRatesReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(Scope::ModelsWrite) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp();

    // 读取旧费率，便于审计记录 before/after
    let before = state.inner.model_repo.get(&id).await?.ok_or(ApiError::NotFound)?;

    let updated = state
        .inner
        .model_repo
        .set_rates(&id, req.input_rate_per_1k, req.output_rate_per_1k, now)
        .await?;
    if !updated {
        return Err(ApiError::NotFound);
    }

    // TODO: 失效 Redis 模型费率缓存。缓存键模式 `model_rates:<logical_name>`，
    // 当前尚无 ModelRateCache 封装，待引入后在此调用 cache 失效（state.inner.cache）。
    // 例如：if let Some(cache) = &state.inner.cache { cache.del(format!("model_rates:{}", before.logical_name)).await; }

    // 审计（含 before/after）
    let detail = serde_json::json!({
        "id": id,
        "logical_name": before.logical_name,
        "before": {
            "input_rate_per_1k": before.input_rate_per_1k,
            "output_rate_per_1k": before.output_rate_per_1k,
        },
        "after": {
            "input_rate_per_1k": req.input_rate_per_1k,
            "output_rate_per_1k": req.output_rate_per_1k,
        },
    })
    .to_string();
    let _ = state
        .inner
        .audit_repo
        .insert(
            "admin",
            Some(&principal.id),
            "rate.update",
            Some("model"),
            Some(&id),
            Some(&detail),
            None,
            now,
        )
        .await;

    Ok(Json(serde_json::json!({
        "id": id,
        "input_rate_per_1k": req.input_rate_per_1k,
        "output_rate_per_1k": req.output_rate_per_1k,
    })))
}
