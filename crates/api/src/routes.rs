//! 路由组装：chat router + admin router + health router。

use crate::handlers;
use axum::routing::{get, post, put, delete};
use axum::Router;

/// Chat API router（:8080）。
pub fn chat_router() -> Router<crate::AppState> {
    Router::new()
        .route("/v1/chat/completions", post(handlers::chat_completions))
        .route("/v1/models", get(handlers::list_models))
        .route("/v1/usage", get(handlers::get_usage))
        .route("/healthz", get(handlers::health))
}

/// Admin API router（:8081）。
pub fn admin_router() -> Router<crate::AppState> {
    use handlers::admin as a;
    Router::new()
        // 认证
        .route("/v1/admin/auth/login", post(a::auth::login))
        .route("/v1/admin/auth/refresh", post(a::auth::refresh))
        .route("/v1/admin/auth/logout", post(a::auth::logout))
        .route("/v1/admin/auth/me", get(a::auth::me))
        .route("/v1/admin/auth/token", post(a::auth::service_token))
        // 管理员管理（MVP 通过 CLI 创建，列表只读）
        // 账号
        .route("/v1/admin/accounts", get(a::accounts::list_accounts).post(a::accounts::create_account))
        .route("/v1/admin/accounts/:id", get(a::accounts::get_account).put(a::accounts::update_account).delete(a::accounts::delete_account))
        .route("/v1/admin/accounts/:id/policy", post(a::policies::bind_account_policy))
        // Token
        .route("/v1/admin/tokens", get(a::tokens::list_tokens).post(a::tokens::create_token))
        .route("/v1/admin/tokens/:id", delete(a::tokens::revoke_token))
        // 积分
        .route("/v1/admin/credits", get(a::credits::get_credits).post(a::credits::adjust_credits))
        .route("/v1/admin/credits/transactions", get(a::credits::list_transactions))
        .route("/v1/admin/credits/:account_id", put(a::credits::set_credits))
        // 模型
        .route("/v1/admin/models", get(a::models::list_models_admin).post(a::models::create_model))
        .route("/v1/admin/models/:id", put(a::models::update_model).delete(a::models::delete_model))
        .route("/v1/admin/models/:id/rates", put(a::models::set_rates))
        // 供应商
        .route("/v1/admin/providers", get(a::providers::list_providers).post(a::providers::create_provider))
        .route("/v1/admin/providers/:id", put(a::providers::update_provider).delete(a::providers::delete_provider))
        .route("/v1/admin/providers/:id/disable", post(a::providers::disable_provider))
        .route("/v1/admin/providers/:id/enable", post(a::providers::enable_provider))
        .route("/v1/admin/providers/:id/usage", get(a::providers::get_provider_usage))
        .route("/v1/admin/providers/:id/sync-quota", post(a::providers::sync_provider_quota))
        .route("/v1/admin/providers/:id/sync-models", post(a::providers::sync_upstream_models))
        // 模型-供应商映射
        .route("/v1/admin/model-providers", get(a::providers::list_mappings).post(a::providers::create_mapping))
        .route("/v1/admin/model-providers/:id", put(a::providers::update_mapping).delete(a::providers::delete_mapping))
        // Service 账号
        .route("/v1/admin/services", get(a::services::list_services).post(a::services::create_service))
        .route("/v1/admin/services/:id", put(a::services::update_service).delete(a::services::delete_service))
        .route("/v1/admin/services/:id/reset-secret", post(a::services::reset_service_secret))
        // 策略
        .route("/v1/admin/policies", get(a::policies::list_policies).post(a::policies::create_policy))
        .route("/v1/admin/policies/:id", put(a::policies::update_policy).delete(a::policies::delete_policy))
        // 设备（Agent 模式）
        .route("/v1/admin/devices", get(a::devices::list_devices).post(a::devices::register_device))
        .route("/v1/admin/devices/:id", delete(a::devices::delete_device))
        // Tun 通道（远程管理接入）
        .route("/v1/admin/tun/status", get(a::tun::tun_status))
        .route("/v1/admin/tun/toggle", post(a::tun::tun_toggle))
        // 报表与审计
        .route("/v1/admin/reports/usage", get(a::reports::report_usage))
        .route("/v1/admin/reports/cost", get(a::reports::report_cost))
        .route("/v1/admin/audit/logs", get(a::reports::audit_logs))
        .route("/v1/admin/usage-logs", get(a::reports::usage_logs))
        // 健康
        .route("/healthz", get(handlers::health))
}

/// 健康检查 router（无状态，启动时占位用）。
pub fn health_router() -> Router {
    Router::new().route("/healthz", get(|| async { "ok" }))
}
