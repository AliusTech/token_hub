//! Chat 接口 handler：核心业务编排。
//!
//! 流程（非流式）：
//! 1. 鉴权（RequireApiUser 提取器完成）→ 拿到 account_id
//! 2. 余额预筛（Redis 缓存或 DB）
//! 3. 查逻辑模型 + 候选 provider 映射
//! 4. tiktoken 估算 prompt → 预冻结积分
//! 5. 按 level/strategy 路由，调用上游（失败自动降级 + 标记 disabled）
//! 6. 拿到 usage → 计算实际积分 → 结算（多退少补）
//! 7. 异步记 usage_logs + 预聚合
//! 8. 返回 OpenAI 格式响应

use crate::error::{ApiError, ApiResult};
use crate::middleware::RequireApiUser;
use crate::state::AppState;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use router_llm::{ChatRequest, FallbackOutcome, QuotaErrorDetector, RouteCandidate, RouteSelector};
use serde::Deserialize;
use uuid::Uuid;

/// OpenAI 兼容的 chat completions 入参（透传上游）。
#[derive(Debug, Deserialize)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<router_llm::ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
}

impl From<ChatCompletionsRequest> for ChatRequest {
    fn from(r: ChatCompletionsRequest) -> Self {
        ChatRequest {
            model: r.model,
            messages: r.messages,
            stream: if r.stream { Some(true) } else { None },
            temperature: r.temperature,
            max_tokens: r.max_tokens,
        }
    }
}

/// POST /v1/chat/completions
pub async fn chat_completions(
    State(state): State<AppState>,
    RequireApiUser(principal): RequireApiUser,
    Json(req): Json<ChatCompletionsRequest>,
) -> ApiResult<impl IntoResponse> {
    if req.stream {
        // 流式：Phase 6 完善（MVP 先返回 501）
        return Err(ApiError::BadRequest("streaming not yet implemented".into()));
    }
    let account_id = principal.account_id.clone().unwrap_or_default();
    let now = chrono::Utc::now().timestamp_millis();

    // 1. 查逻辑模型
    let logical_model = state
        .inner
        .model_repo
        .get_by_name(&req.model)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("unknown model: {}", req.model)))?;

    // 2. 查候选 provider 映射
    let mappings = state.inner.mapping_repo.list_by_logical_model(&logical_model.id).await?;
    if mappings.is_empty() {
        return Err(ApiError::NoProvider);
    }
    // 过滤掉 provider 已 disabled 的
    let mut candidates = Vec::new();
    for m in &mappings {
        let provider = match state.inner.provider_repo.get(&m.provider_id).await? {
            Some(p) if p.status == domain::ProviderStatus::Active => p,
            _ => continue,
        };
        candidates.push((m.clone(), provider));
    }
    if candidates.is_empty() {
        return Err(ApiError::NoProvider);
    }

    // 3. 预冻结：tiktoken 估算 prompt
    let prompt_text: String = req
        .messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");
    let est_prompt_tokens = billing::estimate_prompt_tokens(&prompt_text);
    // 估算积分 = prompt 估算 * input_rate（保守，不估 completion）
    let est_credits = billing::compute_credits(
        &billing::UsageTokens {
            prompt_tokens: est_prompt_tokens,
            completion_tokens: 0,
        },
        logical_model.input_rate_per_1k,
        logical_model.output_rate_per_1k,
    );
    // 至少冻结 1（避免 0 冻结）
    let est_credits = est_credits.max(1);

    let hold = state
        .inner
        .credits_repo
        .place_hold(&account_id, est_credits, Some(&Uuid::new_v4().to_string()), now)
        .await?
        .ok_or(ApiError::InsufficientCredits)?;
    let hold_id = hold.0;

    // 4. 路由选择
    let route_candidates: Vec<RouteCandidate> = candidates
        .iter()
        .map(|(m, _)| RouteCandidate {
            mapping_id: m.id.clone(),
            provider_id: m.provider_id.clone(),
            upstream_model: m.upstream_model.clone(),
            level: m.level,
            weight: m.weight,
            strategy: m.strategy,
        })
        .collect();
    let decision = RouteSelector::select(&route_candidates);

    // 5. 逐个尝试上游（失败降级）
    let mut last_error: Option<ApiError> = None;
    let mut used_provider_id: Option<String> = None;
    let mut used_upstream_model: Option<String> = None;
    let mut chat_response: Option<router_llm::ChatResponse> = None;

    // 构造上游请求（替换 model 为 upstream_model）
    let mut upstream_req: ChatRequest = req.clone().into();
    for candidate in &decision.ordered {
        // 查 provider 的 api_key（此处简化：从 provider repo 取，实际需解密）
        let provider = candidates
            .iter()
            .find(|(_, p)| p.id == candidate.provider_id)
            .map(|(_, p)| p.clone());
        let Some(provider) = provider else { continue };

        upstream_req.model = candidate.upstream_model.clone();
        // MVP：api_key_enc 暂当明文用（Phase 4 加密层完善后解密）
        match state
            .inner
            .upstream
            .chat(&provider.base_url, &provider.api_key_enc, &upstream_req)
            .await
        {
            Ok(resp) => {
                used_provider_id = Some(candidate.provider_id.clone());
                used_upstream_model = Some(candidate.upstream_model.clone());
                chat_response = Some(resp);
                last_error = None;
                break;
            }
            Err(e) => {
                // 判断是否降级 + disable provider
                match QuotaErrorDetector::classify(&e) {
                    FallbackOutcome::ShouldDisableAndFallback { reason } => {
                        tracing::warn!(
                            provider_id = %candidate.provider_id,
                            %reason,
                            "provider failed, disabling and falling back"
                        );
                        let _ = state
                            .inner
                            .provider_repo
                            .set_status(&candidate.provider_id, "disabled", Some(&reason), now)
                            .await;
                        last_error = Some(ApiError::Upstream(format!("provider unavailable: {reason}")));
                    }
                    FallbackOutcome::NoFallback => {
                        // 非 quota 错误：释放冻结，直接返回
                        let _ = state.inner.credits_repo.release_hold(&hold_id, now).await;
                        return Err(map_upstream_error(e));
                    }
                }
            }
        }
    }

    let Some(response) = chat_response else {
        // 所有候选都失败，释放冻结
        let _ = state.inner.credits_repo.release_hold(&hold_id, now).await;
        return Err(last_error.unwrap_or(ApiError::NoProvider));
    };

    // 6. 结算：按 usage 计算实际积分
    let completion_text: String = response
        .choices
        .iter()
        .filter_map(|c| c.message.as_ref())
        .map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n");

    let upstream_usage = response.usage.as_ref().map(|u| {
        billing::usage_source::UpstreamUsage {
            prompt_tokens: Some(u.prompt_tokens),
            completion_tokens: Some(u.completion_tokens),
            total_tokens: Some(u.total_tokens),
        }
    });
    let resolved = billing::resolve_usage(upstream_usage.as_ref(), &prompt_text, &completion_text);
    let actual_credits = billing::compute_credits(
        &resolved.tokens,
        logical_model.input_rate_per_1k,
        logical_model.output_rate_per_1k,
    );
    let actual_credits = actual_credits.max(0);

    let new_balance = state
        .inner
        .credits_repo
        .settle_hold(&hold_id, actual_credits, now)
        .await?
        .ok_or_else(|| ApiError::Internal("settle failed".into()))?;

    // 7. 回填余额缓存
    let balance_cache = cache::BalanceCache::new(state.inner.cache.clone());
    let _ = balance_cache.set(&account_id, new_balance).await;

    // 8. 记日志 + 预聚合 + 供应商额度累加
    let period = chrono::Utc::now().format("%Y%m").to_string();
    let entry = storage::UsageLogEntry {
        id: Uuid::new_v4().to_string(),
        account_id: account_id.clone(),
        logical_model: req.model.clone(),
        provider_id: used_provider_id.clone(),
        upstream_model: used_upstream_model.clone(),
        prompt_tokens: Some(resolved.tokens.prompt_tokens),
        completion_tokens: Some(resolved.tokens.completion_tokens),
        total_tokens: Some(resolved.tokens.total()),
        credits_cost: actual_credits,
        usage_source: resolved.source,
        status: "success".to_string(),
        source_ip: None,
        created_at: now,
    };
    state
        .inner
        .usage_repo
        .insert_and_aggregate(entry, &period)
        .await?;

    // 供应商额度累加 + 80% 告警检测
    if let Some(pid) = &used_provider_id {
        if let Ok((used, limit, threshold, alert_sent)) =
            state.inner.quota_repo.add_usage(pid, resolved.tokens.total(), now).await
        {
            if !alert_sent {
                if let Some(lim) = limit {
                    if lim > 0 && used * 100 / lim >= threshold as i64 {
                        let _ = state.inner.quota_repo.mark_alert_sent(pid, now).await;
                        let pct = used * 100 / lim;
                        tracing::warn!(provider_id = %pid, used, limit, pct, "provider reached quota threshold");
                        // 触发告警通知（防重复：alert_sent 已置位）
                        let provider_name = state
                            .inner
                            .provider_repo
                            .get(pid)
                            .await
                            .ok()
                            .flatten()
                            .map(|p| p.name)
                            .unwrap_or_else(|| pid.clone());
                        state
                            .inner
                            .notifier
                            .notify(
                                "供应商额度告警",
                                &format!(
                                    "供应商 {provider_name}({pid}) 额度已达 {pct}%（{used}/{lim}），请及时处理。"
                                ),
                            )
                            .await;
                    }
                }
            }
        }
    }

    // 9. 返回 OpenAI 格式
    Ok(Json(serde_json::to_value(&response).unwrap_or_default()))
}

fn map_upstream_error(e: router_llm::UpstreamError) -> ApiError {
    match e {
        router_llm::UpstreamError::Status { status, body } => {
            ApiError::Upstream(format!("upstream {status}: {}", &body[..body.len().min(200)]))
        }
        router_llm::UpstreamError::Timeout => ApiError::Upstream("upstream timeout".into()),
        router_llm::UpstreamError::Connect(msg) => ApiError::Upstream(format!("connect: {msg}")),
        router_llm::UpstreamError::Other(msg) => ApiError::Upstream(msg),
    }
}

/// GET /v1/models — 返回可用逻辑模型列表（OpenAI 格式）。
pub async fn list_models(
    State(state): State<AppState>,
    RequireApiUser(_): RequireApiUser,
) -> ApiResult<impl IntoResponse> {
    let models = state.inner.model_repo.list().await?;
    let data: Vec<serde_json::Value> = models
        .into_iter()
        .filter(|m| m.status == "active")
        .map(|m| {
            serde_json::json!({
                "id": m.logical_name,
                "object": "model",
                "owned_by": "tokenhub",
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "object": "list", "data": data })))
}

/// GET /v1/usage — 当前账号余额 + 用量统计。
pub async fn get_usage(
    State(state): State<AppState>,
    RequireApiUser(principal): RequireApiUser,
) -> ApiResult<impl IntoResponse> {
    let account_id = principal.account_id.unwrap_or_default();
    let credits = state.inner.credits_repo.get(&account_id).await?;
    let period = chrono::Utc::now().format("%Y%m").to_string();
    let (_, _, used_credits, calls) = state.inner.usage_repo.account_summary(&account_id, &period).await?;

    let total_credits = credits.as_ref().map(|c| c.balance + c.used_or_zero()).unwrap_or(0);
    let remaining = credits.as_ref().map(|c| c.balance).unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total_credits": total_credits,
        "used_credits": used_credits,
        "remaining_credits": remaining,
        "calls_this_month": calls,
    })))
}

// 辅助 trait：Credits 的 used 字段（不存在则返回 0，这里简化）
trait UsedOrZero {
    fn used_or_zero(&self) -> i64;
}
impl UsedOrZero for domain::Credits {
    fn used_or_zero(&self) -> i64 {
        0 // used_credits 来自预聚合表，balance 是当前余额
    }
}

// Clone for ChatCompletionsRequest（需要 clone 给 upstream_req）
impl Clone for ChatCompletionsRequest {
    fn clone(&self) -> Self {
        Self {
            model: self.model.clone(),
            messages: self.messages.clone(),
            stream: self.stream,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
        }
    }
}
