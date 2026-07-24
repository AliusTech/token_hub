//! 报表与审计接口。
//!
//! GET /v1/admin/reports/usage   — 整体消耗报表（按账号/模型/供应商分组）
//! GET /v1/admin/reports/cost    — 成本报表（各供应商消耗、80% 告警汇总）
//! GET /v1/admin/audit/logs      — 操作审计日志
//! GET /v1/admin/usage-logs      — 调用明细日志

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireAdmin;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ReportParams {
    pub period: Option<String>, // yyyymm，默认当月
}

#[derive(Debug, Deserialize)]
pub struct AuditLogParams {
    pub actor_kind: Option<String>,
    pub action: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UsageLogParams {
    pub account_id: Option<String>,
    pub provider_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

fn current_period() -> String {
    chrono::Utc::now().format("%Y%m").to_string()
}

/// GET /v1/admin/reports/usage
pub async fn report_usage(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Query(params): Query<ReportParams>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ReportsRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let period = params.period.unwrap_or_else(current_period);
    // MVP：返回所有账号 + 所有 provider 的汇总（从预聚合表）
    // 完整多维报表需要额外的聚合查询，这里返回基础数据
    let usage_repo = state.inner.usage_repo.clone();
    let audit_repo = state.inner.audit_repo.clone();

    // 查询所有账号的汇总（需要账号列表）
    let account_repo = storage::AccountRepo::new(state.inner.store.clone());
    let accounts = account_repo.list(None, 1000, 0).await?;
    let mut by_account = Vec::new();
    for acct in &accounts {
        let (p, c, credits, calls) = usage_repo.account_summary(&acct.id, &period).await?;
        if calls > 0 {
            by_account.push(serde_json::json!({
                "account_id": acct.id,
                "external_id": acct.external_id,
                "prompt_tokens": p,
                "completion_tokens": c,
                "credits": credits,
                "calls": calls,
            }));
        }
    }

    let total_credits: i64 = by_account.iter().filter_map(|v| v["credits"].as_i64()).sum();
    let total_calls: i64 = by_account.iter().filter_map(|v| v["calls"].as_i64()).sum();

    Ok(Json(serde_json::json!({
        "period": period,
        "total_credits": total_credits,
        "total_calls": total_calls,
        "by_account": by_account,
    })))
}

/// GET /v1/admin/reports/cost
pub async fn report_cost(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Query(params): Query<ReportParams>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ReportsRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let period = params.period.unwrap_or_else(current_period);
    let provider_repo = state.inner.provider_repo.clone();
    let quota_repo = state.inner.quota_repo.clone();
    let providers = provider_repo.list().await?;
    let mut by_provider = Vec::new();
    for p in &providers {
        let tokens = state.inner.usage_repo.provider_summary(&p.id, &period).await?;
        let (used, limit, threshold, alert_sent) = quota_repo.get_quota(&p.id).await?.unwrap_or((0, None, 80, false));
        let pct = limit.map(|l| if l > 0 { used * 100 / l } else { 0 }).unwrap_or(0);
        by_provider.push(serde_json::json!({
            "provider_id": p.id,
            "name": p.name,
            "status": if p.status == domain::ProviderStatus::Active { "active" } else { "disabled" },
            "tokens_used": tokens,
            "quota_used": used,
            "quota_limit": limit,
            "quota_percentage": pct,
            "alert_sent": alert_sent,
        }));
    }
    Ok(Json(serde_json::json!({
        "period": period,
        "by_provider": by_provider,
    })))
}

/// GET /v1/admin/audit/logs
pub async fn audit_logs(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Query(params): Query<AuditLogParams>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::AuditRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);
    let logs = state
        .inner
        .audit_repo
        .list(params.actor_kind.as_deref(), params.action.as_deref(), limit, offset)
        .await?;
    Ok(Json(serde_json::json!({ "data": logs })))
}

/// GET /v1/admin/usage-logs
pub async fn usage_logs(
    State(state): State<AppState>,
    RequireAdmin(principal): RequireAdmin,
    Query(params): Query<UsageLogParams>,
) -> ApiResult<impl IntoResponse> {
    if !principal.has_scope(domain::Scope::ReportsRead) && !principal.is_super_admin() {
        return Err(ApiError::Forbidden);
    }
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);
    let logs = state
        .inner
        .usage_repo
        .list(params.account_id.as_deref(), params.provider_id.as_deref(), limit, offset)
        .await?;
    Ok(Json(serde_json::json!({ "data": logs })))
}
