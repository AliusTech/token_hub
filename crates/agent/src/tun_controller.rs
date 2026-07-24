//! Tun 通道控制器：运行时开关 frpc 隧道。
//!
//! 提供 open / close / status 三个操作，供 Admin API 和 CLI 调用。
//! open 时自动生成 Service ID（小写字母+数字），写入 frpc.toml 并启动 frpc 子进程。
//! 实现 api::TunControl trait，打破循环依赖。

use crate::frpc::{self, FrpcConfig};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;

/// 通道状态（运行时可变，受 Mutex 保护）。
struct TunState {
    /// 当前通道的 Service ID
    service_id: Option<String>,
    /// 关闭信号发送端（None = 未运行）
    shutdown_tx: Option<watch::Sender<bool>>,
    /// supervise 任务句柄
    task_handle: Option<JoinHandle<()>>,
}

/// Tun 控制器（Arc 共享，多 handler 可并发访问）。
#[derive(Clone)]
pub struct TunController {
    state: Arc<Mutex<TunState>>,
    config: FrpcConfig,
    config_dir: PathBuf,
}

impl TunController {
    /// 创建控制器。初始状态为关闭。
    pub fn new(config: FrpcConfig, config_dir: PathBuf) -> Self {
        Self {
            state: Arc::new(Mutex::new(TunState {
                service_id: None,
                shutdown_tx: None,
                task_handle: None,
            })),
            config,
            config_dir,
        }
    }

    /// 开通通道：生成 Service ID → 写 frpc.toml → 启动 frpc。
    /// 若已开通则返回当前状态（幂等）。
    pub async fn open(&self) -> anyhow::Result<api::TunStatus> {
        let mut state = self.state.lock().await;

        // 若已在运行，直接返回当前状态
        if state.shutdown_tx.is_some() {
            return Ok(self.status_locked(&state));
        }

        // 生成新 Service ID
        let service_id = frpc::generate_service_id();

        // 创建关闭信号通道
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // 启动 supervise 任务
        let cfg = self.config.clone();
        let sid = service_id.clone();
        let dir = self.config_dir.clone();
        let handle = tokio::spawn(async move {
            frpc::supervise_frpc(cfg, sid, dir, shutdown_rx).await;
        });

        state.service_id = Some(service_id.clone());
        state.shutdown_tx = Some(shutdown_tx);
        state.task_handle = Some(handle);

        let status = self.status_locked(&state);
        tracing::info!(service_id = %service_id, "tun 通道已开通");
        Ok(status)
    }

    /// 关闭通道：发送 shutdown 信号 → 等待 frpc 子进程退出。
    /// 若未开通则无操作（幂等）。
    pub async fn close(&self) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;

        if let Some(tx) = state.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        if let Some(handle) = state.task_handle.take() {
            let _ = handle.await;
        }
        let sid = state.service_id.take();
        tracing::info!(service_id = ?sid, "tun 通道已关闭");
        Ok(())
    }

    /// 查询当前通道状态。
    pub async fn status(&self) -> api::TunStatus {
        let state = self.state.lock().await;
        self.status_locked(&state)
    }

    fn status_locked(&self, state: &TunState) -> api::TunStatus {
        let active = state.shutdown_tx.is_some();
        let service_id = state.service_id.clone();
        let url = service_id.as_ref().map(|sid| match &self.config.mode {
            frpc::TunnelMode::HttpSubdomain => {
                format!("{sid}.{}", self.config.subdomain_host)
            }
            frpc::TunnelMode::Tcp { remote_port } => {
                format!("{}:{remote_port}", self.config.server)
            }
        });
        let mode = match &self.config.mode {
            frpc::TunnelMode::HttpSubdomain => "http_subdomain".to_string(),
            frpc::TunnelMode::Tcp { .. } => "tcp".to_string(),
        };
        api::TunStatus {
            active,
            service_id,
            url,
            mode,
        }
    }
}

/// 实现 api::TunControl trait，供 AppState 通过 trait object 使用。
#[async_trait::async_trait]
impl api::TunControl for TunController {
    async fn open(&self) -> anyhow::Result<api::TunStatus> {
        TunController::open(self).await
    }
    async fn close(&self) -> anyhow::Result<()> {
        TunController::close(self).await
    }
    async fn status(&self) -> api::TunStatus {
        TunController::status(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frpc::TunnelMode;

    fn test_controller(mode: TunnelMode) -> TunController {
        let config = FrpcConfig {
            server: "frp.alius.tech".to_string(),
            port: 7000,
            token: Some("test".to_string()),
            subdomain_host: "tun.alius.tech".to_string(),
            local_addr: "127.0.0.1".to_string(),
            local_port: 8081,
            frpc_binary: "echo".to_string(), // 用 echo 模拟 frpc（不会真正运行）
            mode,
        };
        let dir = std::env::temp_dir().join("tokenhub-test-tun");
        TunController::new(config, dir)
    }

    #[tokio::test]
    async fn status_initially_inactive() {
        let ctrl = test_controller(TunnelMode::HttpSubdomain);
        let status = ctrl.status().await;
        assert!(!status.active);
        assert!(status.service_id.is_none());
    }

    #[tokio::test]
    async fn open_generates_service_id() {
        let ctrl = test_controller(TunnelMode::HttpSubdomain);
        let status = ctrl.open().await.unwrap();
        assert!(status.active);
        assert!(status.service_id.is_some());
        let sid = status.service_id.unwrap();
        assert_eq!(sid.len(), 6);
        assert!(sid
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert!(status.url.as_ref().unwrap().ends_with(".tun.alius.tech"));
        assert_eq!(status.mode, "http_subdomain");
        // 清理
        ctrl.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_mode_url_format() {
        let ctrl = test_controller(TunnelMode::Tcp { remote_port: 6001 });
        let status = ctrl.open().await.unwrap();
        assert!(status.url.as_ref().unwrap().contains(":6001"));
        assert_eq!(status.mode, "tcp");
        ctrl.close().await.unwrap();
    }

    #[tokio::test]
    async fn close_sets_inactive() {
        let ctrl = test_controller(TunnelMode::HttpSubdomain);
        ctrl.open().await.unwrap();
        assert!(ctrl.status().await.active);
        ctrl.close().await.unwrap();
        assert!(!ctrl.status().await.active);
    }

    #[tokio::test]
    async fn open_idempotent() {
        let ctrl = test_controller(TunnelMode::HttpSubdomain);
        let s1 = ctrl.open().await.unwrap();
        let s2 = ctrl.open().await.unwrap();
        // 二次 open 应返回相同 service_id
        assert_eq!(s1.service_id, s2.service_id);
        ctrl.close().await.unwrap();
    }
}
