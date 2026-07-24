//! 设备凭证管理接口（Agent 模式，Console 接入凭证）。
//!
//! GET    /v1/admin/devices          — 设备列表
//! POST   /v1/admin/devices          — 注册新设备（生成 device_id + device_key 明文仅返回一次）
//! DELETE /v1/admin/devices/{id}     — 停用设备
//!
//! 注：设备凭证表 device_credentials 在 Agent 本地 SQLite。
//! 此接口在 Agent 模式下用于管理 Console 接入凭证。

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireAdmin;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

/// GET /v1/admin/devices
pub async fn list_devices(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
) -> ApiResult<impl IntoResponse> {
    if !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let rows = sqlx::query(
        "SELECT device_id, name, platform, status, created_at, last_seen_at \
         FROM device_credentials ORDER BY created_at DESC",
    )
    .fetch_all(&state.inner.store.pool)
    .await?;
    let data: Vec<_> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "device_id": r.get::<String, _>("device_id"),
                "name": r.get::<String, _>("name"),
                "platform": r.get::<Option<String>, _>("platform"),
                "status": r.get::<String, _>("status"),
                "created_at": r.get::<i64, _>("created_at"),
                "last_seen_at": r.get::<Option<i64>, _>("last_seen_at"),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "data": data })))
}

#[derive(Debug, Deserialize)]
pub struct RegisterDeviceReq {
    pub name: String,
    pub platform: Option<String>,
}

/// POST /v1/admin/devices — 注册设备，device_key 明文仅返回一次
pub async fn register_device(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Json(req): Json<RegisterDeviceReq>,
) -> ApiResult<impl IntoResponse> {
    if !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let device_id = format!("dev_{}", Uuid::new_v4());
    // 生成 device_key 明文
    let device_key_plain = format!("dk_{}", auth::generate_session_token());
    let device_key_hash = auth::hash_api_token(&device_key_plain, state.secret());

    sqlx::query(
        "INSERT INTO device_credentials (device_id, device_key_hash, name, platform, status, created_at) \
         VALUES (?, ?, ?, ?, 'active', ?)",
    )
    .bind(&device_id)
    .bind(&device_key_hash)
    .bind(&req.name)
    .bind(&req.platform)
    .bind(now)
    .execute(&state.inner.store.pool)
    .await?;

    // 审计
    let _ = state
        .inner
        .audit_repo
        .insert("admin", Some(&principal.id), "device.register", Some("device"), Some(&device_id), None, None, now)
        .await;

    Ok(Json(serde_json::json!({
        "device_id": device_id,
        "device_key": device_key_plain,  // 仅此一次
        "name": req.name,
    })))
}

/// DELETE /v1/admin/devices/{id}
pub async fn delete_device(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Path(device_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let res = sqlx::query("DELETE FROM device_credentials WHERE device_id = ?")
        .bind(&device_id)
        .execute(&state.inner.store.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(serde_json::json!({"ok": true})))
}
