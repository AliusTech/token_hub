//! Admin 端认证：支持三条凭证路径，统一映射到 Principal。
//!
//! 1. Admin 有状态 access token（hash 查 admin_access_tokens）
//! 2. Service JWT（verify_service_jwt）
//! 3. Device 凭证（Agent 模式，Console 接入）— Phase 4.5 实现
//!
//! 目前实现 1 和 2。按 Authorization 头的内容自动判断：
//! - 若是 server secret 生成的 JWT → Service 路径
//! - 否则视为 Admin session token

use crate::error::{ApiError, ApiResult};
use crate::middleware::auth_chat::extract_bearer;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use domain::{Principal, PrincipalKind, Scope};

/// 提取器：要求 Admin 或 Service 身份。
pub struct RequireAdmin(pub Principal);

#[axum::async_trait]
impl FromRequestParts<AppState> for RequireAdmin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_bearer(parts)?;
        let principal = authenticate_admin(&token, state).await?;
        Ok(RequireAdmin(principal))
    }
}

/// 要求特定 scope 的提取器（编译期无法参数化，用辅助函数检查）。
pub struct RequireScoped {
    pub principal: Principal,
    pub scope: Scope,
}

impl RequireScoped {
    /// 检查 principal 是否拥有指定 scope。
    pub fn check(principal: Principal, scope: Scope) -> ApiResult<Self> {
        if principal.has_scope(scope) || principal.is_super_admin() {
            Ok(RequireScoped { principal, scope })
        } else {
            Err(ApiError::Forbidden)
        }
    }
}

/// 认证 admin 端请求：尝试 JWT（Service）→ 失败则尝试 session token（Admin）。
pub async fn authenticate_admin(token: &str, state: &AppState) -> ApiResult<Principal> {
    // 1. 尝试作为 Service JWT
    if let Ok(claims) = auth::verify_service_jwt(token, state.secret()) {
        let scopes: Vec<Scope> = claims
            .scope
            .iter()
            .filter_map(|s| Scope::from_str_lossy(s))
            .collect();
        return Ok(Principal {
            kind: PrincipalKind::Service,
            id: claims.iss,
            account_id: None,
            scopes,
        });
    }

    // 2. 尝试作为 Admin 有状态 session token
    let token_hash = auth::hash_session_token(token, state.secret());
    let admin_repo = storage::AdminUserRepo::new(state.inner.store.clone());
    let access = admin_repo
        .find_access_token(&token_hash)
        .await?
        .ok_or_else(|| ApiError::Unauthorized("invalid admin token".into()))?;

    if access.revoked {
        return Err(ApiError::Unauthorized("token revoked".into()));
    }
    let now = chrono::Utc::now().timestamp();
    if access.expires_at < now {
        return Err(ApiError::Unauthorized("token expired".into()));
    }

    // 加载 admin 的 scopes（从 roles 推导；MVP：admin.write 用户为超管）
    let admin = admin_repo
        .find_by_id(&access.admin_id)
        .await?
        .ok_or_else(|| ApiError::Unauthorized("admin not found".into()))?;
    if admin.status != "active" {
        return Err(ApiError::Unauthorized("admin disabled".into()));
    }

    // MVP scope 策略：拥有 admin.write role → 超管（全部 scope）；否则只读
    let scopes = if admin
        .roles
        .iter()
        .any(|r| r == "admin.write" || r == "super_admin")
    {
        // 超管：全部 scope
        vec![
            Scope::AdminRead,
            Scope::AdminWrite,
            Scope::AccountsRead,
            Scope::AccountsWrite,
            Scope::TokensRead,
            Scope::TokensWrite,
            Scope::CreditsRead,
            Scope::CreditsWrite,
            Scope::CreditsAdmin,
            Scope::ModelsRead,
            Scope::ModelsWrite,
            Scope::ProvidersRead,
            Scope::ProvidersWrite,
            Scope::ServicesRead,
            Scope::ServicesWrite,
            Scope::PoliciesRead,
            Scope::PoliciesWrite,
            Scope::ReportsRead,
            Scope::AuditRead,
        ]
    } else {
        vec![
            Scope::AdminRead,
            Scope::AccountsRead,
            Scope::TokensRead,
            Scope::CreditsRead,
            Scope::ModelsRead,
            Scope::ProvidersRead,
            Scope::ReportsRead,
            Scope::AuditRead,
        ]
    };

    Ok(Principal {
        kind: PrincipalKind::Admin,
        id: admin.id,
        account_id: None,
        scopes,
    })
}
