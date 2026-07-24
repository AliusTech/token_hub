//! 认证接口：管理员登录（TOTP）/ 刷新 / 登出 / 当前用户 / Service token。
//!
//! POST /v1/admin/auth/login        — phone+password+totp_code → access+refresh token
//! POST /v1/admin/auth/refresh      — refresh_token → 新 access_token
//! POST /v1/admin/auth/logout       — 吊销当前 access token
//! GET  /v1/admin/auth/me           — 当前管理员信息
//! POST /v1/admin/auth/token        — Service client_credentials → JWT

use crate::error::{ApiError, ApiResult};
use crate::middleware::auth_admin::authenticate_admin;
use crate::middleware::auth_chat::extract_bearer;
use crate::state::AppState;
use axum::extract::{Request, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

const ACCESS_TOKEN_TTL_SECS: i64 = 3600; // 1 小时
const REFRESH_TOKEN_TTL_SECS: i64 = 7 * 24 * 3600; // 7 天
const SERVICE_JWT_TTL_SECS: i64 = 3600;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub phone: String,
    pub password: String,
    pub totp_code: String,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub token_type: String,
}

/// POST /v1/admin/auth/login
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> ApiResult<impl IntoResponse> {
    let admin_repo = storage::AdminUserRepo::new(state.inner.store.clone());
    let admin = admin_repo
        .find_by_phone(&req.phone)
        .await?
        .ok_or_else(|| ApiError::Unauthorized("invalid credentials".into()))?;
    if admin.status != "active" {
        return Err(ApiError::Unauthorized("admin disabled".into()));
    }
    // 校验密码
    auth::verify_password(&req.password, &admin.password_hash)
        .map_err(|_| ApiError::Unauthorized("invalid credentials".into()))?;
    // 校验 TOTP
    auth::verify_totp(&admin.totp_secret, &req.totp_code)
        .map_err(|_| ApiError::Unauthorized("invalid totp code".into()))?;

    let now = chrono::Utc::now().timestamp();
    let _ = admin_repo.update_last_login(&admin.id, now).await;

    // 生成有状态 access token + refresh token
    let access_plain = auth::generate_session_token();
    let access_hash = auth::hash_session_token(&access_plain, state.secret());
    admin_repo
        .create_access_token(&access_hash, &admin.id, now + ACCESS_TOKEN_TTL_SECS, now)
        .await?;

    let refresh_plain = auth::generate_session_token();
    let refresh_hash = auth::hash_session_token(&refresh_plain, state.secret());
    admin_repo
        .create_refresh_token(
            &refresh_hash,
            &admin.id,
            &access_hash,
            now + REFRESH_TOKEN_TTL_SECS,
            now,
        )
        .await?;

    Ok(Json(serde_json::json!({
        "access_token": access_plain,
        "refresh_token": refresh_plain,
        "expires_in": ACCESS_TOKEN_TTL_SECS,
        "token_type": "Bearer",
    })))
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// POST /v1/admin/auth/refresh
pub async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> ApiResult<impl IntoResponse> {
    let admin_repo = storage::AdminUserRepo::new(state.inner.store.clone());
    let refresh_hash = auth::hash_session_token(&req.refresh_token, state.secret());
    let (admin_id, old_access_hash, expires_at, revoked) = admin_repo
        .find_refresh_token(&refresh_hash)
        .await?
        .ok_or_else(|| ApiError::Unauthorized("invalid refresh token".into()))?;
    if revoked || expires_at < chrono::Utc::now().timestamp() {
        return Err(ApiError::Unauthorized(
            "refresh token expired or revoked".into(),
        ));
    }
    // 吊销旧 refresh + 旧 access
    let _ = admin_repo.revoke_refresh_token(&refresh_hash).await;
    let _ = admin_repo.revoke_access_token(&old_access_hash).await;

    let now = chrono::Utc::now().timestamp();
    let access_plain = auth::generate_session_token();
    let access_hash = auth::hash_session_token(&access_plain, state.secret());
    admin_repo
        .create_access_token(&access_hash, &admin_id, now + ACCESS_TOKEN_TTL_SECS, now)
        .await?;
    let new_refresh_plain = auth::generate_session_token();
    let new_refresh_hash = auth::hash_session_token(&new_refresh_plain, state.secret());
    admin_repo
        .create_refresh_token(
            &new_refresh_hash,
            &admin_id,
            &access_hash,
            now + REFRESH_TOKEN_TTL_SECS,
            now,
        )
        .await?;

    Ok(Json(serde_json::json!({
        "access_token": access_plain,
        "refresh_token": new_refresh_plain,
        "expires_in": ACCESS_TOKEN_TTL_SECS,
        "token_type": "Bearer",
    })))
}

/// POST /v1/admin/auth/logout — 吊销当前 access token
pub async fn logout(State(state): State<AppState>, req: Request) -> ApiResult<impl IntoResponse> {
    let (parts, _body) = req.into_parts();
    let token = extract_bearer(&parts)?;
    let admin_repo = storage::AdminUserRepo::new(state.inner.store.clone());
    let access_hash = auth::hash_session_token(&token, state.secret());
    let _ = admin_repo.revoke_access_token(&access_hash).await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// GET /v1/admin/auth/me
pub async fn me(State(state): State<AppState>, req: Request) -> ApiResult<impl IntoResponse> {
    let (parts, _body) = req.into_parts();
    let token = extract_bearer(&parts)?;
    let principal = authenticate_admin(&token, &state).await?;
    let admin_repo = storage::AdminUserRepo::new(state.inner.store.clone());
    let admin = admin_repo
        .find_by_id(&principal.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(serde_json::json!({
        "id": admin.id,
        "phone": admin.phone,
        "roles": admin.roles,
        "status": admin.status,
    })))
}

/// Service client_credentials 换 JWT。
/// POST /v1/admin/auth/token  (form-urlencoded: grant_type, client_id, client_secret)
#[derive(Debug, Deserialize)]
pub struct ClientCredentialsForm {
    pub grant_type: String,
    pub client_id: String,
    pub client_secret: String,
}

pub async fn service_token(
    State(state): State<AppState>,
    Json(form): Json<ClientCredentialsForm>,
) -> ApiResult<impl IntoResponse> {
    // 也兼容 form-urlencoded（MVP 用 JSON 即可，前端按需调整）
    if form.grant_type != "client_credentials" {
        return Err(ApiError::BadRequest("unsupported grant_type".into()));
    }
    let svc_repo = storage::ServiceAccountRepo::new(state.inner.store.clone());
    let (id, secret_hash, scopes, status) = svc_repo
        .find_auth(&form.client_id)
        .await?
        .ok_or_else(|| ApiError::Unauthorized("invalid client credentials".into()))?;
    if status != "active" {
        return Err(ApiError::Unauthorized("service disabled".into()));
    }
    // 校验 client_secret（HMAC 比对）
    let provided_hash = auth::hash_api_token(&form.client_secret, state.secret());
    if provided_hash != secret_hash {
        return Err(ApiError::Unauthorized("invalid client credentials".into()));
    }
    let jwt = auth::issue_service_jwt(&id, &scopes, SERVICE_JWT_TTL_SECS, state.secret())
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({
        "access_token": jwt,
        "token_type": "Bearer",
        "expires_in": SERVICE_JWT_TTL_SECS,
        "scope": scopes.join(" "),
    })))
}
