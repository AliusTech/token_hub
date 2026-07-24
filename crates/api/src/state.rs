//! 应用共享状态：注入到所有 handler / 中间件。

use std::sync::Arc;

/// 共享状态（Arc 包装，廉价克隆）。
#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<AppStateInner>,
}

pub struct AppStateInner {
    pub store: storage::SqliteStore,
    pub credits_repo: storage::CreditsRepo,
    pub account_repo: storage::AccountRepo,
    pub token_repo: storage::TokenRepo,
    pub model_repo: storage::ModelRepo,
    pub provider_repo: storage::ProviderRepo,
    pub mapping_repo: storage::ModelProviderRepo,
    pub usage_repo: storage::UsageLogRepo,
    pub quota_repo: storage::ProviderQuotaRepo,
    pub audit_repo: storage::AuditLogRepo,
    pub secret: auth::ServerSecret,
    pub upstream: router_llm::UpstreamClient,
    // 统一缓存（永远有值：默认内存，可选 Redis）
    pub cache: cache::CacheStore,
    // 告警通知器（80% 额度告警等）
    pub notifier: std::sync::Arc<dyn audit::Notifier>,
    // Tun 通道控制器（Agent 模式有值，Server 模式 None）
    pub tun: Option<std::sync::Arc<dyn crate::tun_trait::TunControl>>,
}

impl AppState {
    pub fn new(
        store: storage::SqliteStore,
        secret: auth::ServerSecret,
        cache: cache::CacheStore,
        notifier: std::sync::Arc<dyn audit::Notifier>,
        tun: Option<std::sync::Arc<dyn crate::tun_trait::TunControl>>,
    ) -> Self {
        let credits_repo = storage::CreditsRepo::new(store.clone());
        let account_repo = storage::AccountRepo::new(store.clone());
        let token_repo = storage::TokenRepo::new(store.clone());
        let model_repo = storage::ModelRepo::new(store.clone());
        let provider_repo = storage::ProviderRepo::new(store.clone());
        let mapping_repo = storage::ModelProviderRepo::new(store.clone());
        let usage_repo = storage::UsageLogRepo::new(store.clone());
        let quota_repo = storage::ProviderQuotaRepo::new(store.clone());
        let audit_repo = storage::AuditLogRepo::new(store.clone());
        let upstream = router_llm::UpstreamClient::new();

        Self {
            inner: Arc::new(AppStateInner {
                store,
                credits_repo,
                account_repo,
                token_repo,
                model_repo,
                provider_repo,
                mapping_repo,
                usage_repo,
                quota_repo,
                audit_repo,
                secret,
                upstream,
                cache,
                notifier,
                tun,
            }),
        }
    }

    /// 便捷访问
    pub fn credits(&self) -> &storage::CreditsRepo {
        &self.inner.credits_repo
    }
    pub fn secret(&self) -> &auth::ServerSecret {
        &self.inner.secret
    }
}
