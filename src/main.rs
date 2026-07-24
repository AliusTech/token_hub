//! TokenHub 入口：支持两种运行方式（单一二进制）。
//!
//! - Server 模式（默认）：云端/容器，双 listener
//! - Agent 模式：桌面，本地 listener
//! 两种模式都支持 tun 通道（环境变量配置 FRP_SERVER 时可用）。

use clap::Parser;
use std::net::SocketAddr;

mod config;
use config::RunMode;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let cli = cli::Cli::parse();
        return cli::run(cli).await;
    }

    let cfg = config::Config::load()?;
    match cfg.run_mode {
        RunMode::Server => run_server(cfg).await,
        RunMode::Agent => run_agent(cfg).await,
    }
}

/// 构造 TunController（Server 和 Agent 模式共用）。
/// 仅在配置了 FRP_SERVER 时创建，否则返回 None。
fn build_tun_controller(cfg: &config::Config) -> Option<std::sync::Arc<dyn api::TunControl>> {
    let server = cfg.frp_server.as_ref()?;
    let port = cfg.frp_port.unwrap_or(7000);
    let local_port = cfg
        .admin_listen
        .parse::<SocketAddr>()
        .ok()
        .map(|a| a.port())
        .unwrap_or(8081);

    let mode = match cfg.frp_mode.as_str() {
        "tcp" => agent::TunnelMode::Tcp {
            remote_port: cfg.frp_remote_port.unwrap_or(6001),
        },
        _ => agent::TunnelMode::HttpSubdomain,
    };

    let frpc_cfg = agent::FrpcConfig {
        server: server.clone(),
        port,
        token: cfg.frp_token.clone(),
        subdomain_host: std::env::var("FRP_SUBDOMAIN_HOST")
            .unwrap_or_else(|_| "tun.alius.tech".to_string()),
        local_addr: "127.0.0.1".to_string(),
        local_port,
        frpc_binary: std::env::var("FRPC_BINARY").unwrap_or_else(|_| "frpc".to_string()),
        mode,
    };

    let config_dir = dirs_config_dir();
    let ctrl = agent::TunController::new(frpc_cfg, config_dir);
    Some(std::sync::Arc::new(ctrl) as std::sync::Arc<dyn api::TunControl>)
}

async fn run_server(cfg: config::Config) -> anyhow::Result<()> {
    use api::{admin_router, chat_router, AppState};

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "TokenHub Server starting");

    let store = storage::connect(&cfg.database_url).await?;
    let secret = auth::ServerSecret::new(&cfg.server_secret);
    let cache = cache::connect(&cfg.redis_url).await;
    let notifier: std::sync::Arc<dyn audit::Notifier> = std::sync::Arc::new(audit::ConsoleNotifier);

    // tun 通道（配置了 FRP_SERVER 才可用）
    let tun = build_tun_controller(&cfg);
    if tun.is_some() {
        tracing::info!("tun 通道功能可用（POST /v1/admin/tun/toggle 开通）");
    } else {
        tracing::info!("tun 通道未配置（FRP_SERVER 未设置），如需远程管理请配置 FRP_* 环境变量");
    }

    let state = AppState::new(store, secret, cache, notifier, tun);
    let chat_app = chat_router().with_state(state.clone());
    let admin_app = admin_router().with_state(state);

    let chat_addr: SocketAddr = cfg.chat_listen.parse()?;
    let admin_addr: SocketAddr = cfg.admin_listen.parse()?;

    let chat = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(chat_addr).await.unwrap();
        tracing::info!(%chat_addr, "chat API listening");
        axum::serve(listener, chat_app).await.unwrap();
    });
    let admin = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(admin_addr).await.unwrap();
        tracing::info!(%admin_addr, "admin API listening");
        axum::serve(listener, admin_app).await.unwrap();
    });

    tokio::select! {
        _ = chat => tracing::error!("chat listener exited"),
        _ = admin => tracing::error!("admin listener exited"),
    }
    Ok(())
}

async fn run_agent(cfg: config::Config) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Agent 默认绑 127.0.0.1
    let chat_listen = if cfg.chat_listen.starts_with("0.0.0.0") {
        "127.0.0.1:8080".to_string()
    } else {
        cfg.chat_listen.clone()
    };
    let admin_listen = if cfg.admin_listen.starts_with("0.0.0.0") {
        "127.0.0.1:8081".to_string()
    } else {
        cfg.admin_listen.clone()
    };

    // 构造 FRP 配置
    let frpc = cfg.frp_server.as_ref().map(|server| agent::FrpcConfig {
        server: server.clone(),
        port: cfg.frp_port.unwrap_or(7000),
        token: cfg.frp_token.clone(),
        subdomain_host: std::env::var("FRP_SUBDOMAIN_HOST")
            .unwrap_or_else(|_| "tun.alius.tech".to_string()),
        local_addr: "127.0.0.1".to_string(),
        local_port: admin_listen.parse::<SocketAddr>().ok().map(|a| a.port()).unwrap_or(8081),
        frpc_binary: std::env::var("FRPC_BINARY").unwrap_or_else(|_| "frpc".to_string()),
        mode: agent::TunnelMode::HttpSubdomain,
    });

    let config_dir = dirs_config_dir();

    agent::run_agent(agent::runtime::AgentConfig {
        database_url: cfg.database_url,
        redis_url: cfg.redis_url,
        server_secret: cfg.server_secret,
        chat_listen,
        admin_listen,
        frpc,
        config_dir,
    })
    .await
}

/// 获取配置目录（跨平台）。
fn dirs_config_dir() -> std::path::PathBuf {
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        std::path::Path::new(&home).join(".tokenhub")
    } else {
        std::path::PathBuf::from(".tokenhub")
    }
}
