//! Agent 运行时编排。

use crate::frpc::FrpcConfig;
use crate::tun_controller::TunController;
use api::{admin_router, chat_router, AppState};
use std::net::SocketAddr;

/// Agent 运行配置。
pub struct AgentConfig {
    pub database_url: String,
    pub redis_url: String,
    pub server_secret: String,
    pub chat_listen: String,
    pub admin_listen: String,
    /// FRP 配置（可选：有则创建 TunController，无则 tun 功能不可用）
    pub frpc: Option<FrpcConfig>,
    /// frpc 配置/工作目录
    pub config_dir: std::path::PathBuf,
}

/// 运行 Agent。
pub async fn run_agent(cfg: AgentConfig) -> anyhow::Result<()> {
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "TokenHub Agent starting"
    );

    let store = storage::connect(&cfg.database_url).await?;
    let secret = auth::ServerSecret::new(&cfg.server_secret);

    let cache = cache::connect(&cfg.redis_url).await;
    let notifier: std::sync::Arc<dyn audit::Notifier> = std::sync::Arc::new(audit::ConsoleNotifier);

    // 创建 TunController（Agent 模式独有）
    let tun: Option<std::sync::Arc<dyn api::TunControl>> = cfg.frpc.as_ref().map(|frpc_cfg| {
        let ctrl = TunController::new(frpc_cfg.clone(), cfg.config_dir.clone());
        std::sync::Arc::new(ctrl) as std::sync::Arc<dyn api::TunControl>
    });

    let state = AppState::new(store, secret, cache, notifier, tun);

    let chat_app = chat_router().with_state(state.clone());
    let admin_app = admin_router().with_state(state);

    let chat_addr: SocketAddr = cfg.chat_listen.parse()?;
    let admin_addr: SocketAddr = cfg.admin_listen.parse()?;

    let chat = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(chat_addr).await.unwrap();
        tracing::info!(%chat_addr, "Agent chat API listening (local)");
        axum::serve(listener, chat_app).await.unwrap();
    });

    let admin = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(admin_addr).await.unwrap();
        tracing::info!(%admin_addr, "Agent admin API listening (local)");
        axum::serve(listener, admin_app).await.unwrap();
    });

    tracing::info!("Agent 启动完成。使用 'tokenhub tun open' 开通远程管理通道。");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Ctrl-C received, shutting down");
        }
        _ = chat => tracing::error!("chat listener exited"),
        _ = admin => tracing::error!("admin listener exited"),
    }

    Ok(())
}
