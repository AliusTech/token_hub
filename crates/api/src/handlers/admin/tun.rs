//! Tun 通道管理接口。
//!
//! POST /v1/admin/tun/toggle  — 切换通道（关→开 / 开→关）
//! GET  /v1/admin/tun/status  — 查询通道状态

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireAdmin;
use crate::state::AppState;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;

/// GET /v1/admin/tun/status
pub async fn tun_status(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
) -> ApiResult<impl IntoResponse> {
    if !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    match &state.inner.tun {
        Some(tun) => {
            let status = tun.status().await;
            Ok(Json(serde_json::to_value(&status).unwrap_or_default()))
        }
        None => Ok(Json(serde_json::json!({
            "active": false,
            "service_id": null,
            "url": null,
            "mode": null,
            "available": false,
            "message": "tun 通道未配置（FRP_SERVER 未设置）"
        }))),
    }
}

/// POST /v1/admin/tun/toggle — 切换通道状态
pub async fn tun_toggle(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
) -> ApiResult<impl IntoResponse> {
    if !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let tun = state
        .inner
        .tun
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("tun 通道未配置（FRP_SERVER 未设置）".into()))?;

    let current = tun.status().await;
    let now = chrono::Utc::now().timestamp_millis();

    if current.active {
        // 开 → 关
        tun.close()
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        // 审计
        let _ = state
            .inner
            .audit_repo
            .insert(
                "admin",
                Some(&principal.id),
                "tun.close",
                None,
                None,
                None,
                None,
                now,
            )
            .await;
        Ok(Json(serde_json::json!({
            "action": "closed",
            "active": false,
            "service_id": null,
            "url": null,
        })))
    } else {
        // 关 → 开
        let status = tun
            .open()
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        // 审计
        let _ = state
            .inner
            .audit_repo
            .insert(
                "admin",
                Some(&principal.id),
                "tun.open",
                None,
                None,
                Some(&format!(
                    "{{\"service_id\":\"{}\"}}",
                    status.service_id.as_deref().unwrap_or("")
                )),
                None,
                now,
            )
            .await;
        Ok(Json(serde_json::to_value(&status).unwrap_or_default()))
    }
}
