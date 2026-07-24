//! Tun 通道控制 trait（打破 api ↔ agent 循环依赖）。
//!
//! api crate 定义此 trait，agent crate 的 TunController 实现它。
//! AppState 持有 Option<Arc<dyn TunControl>>，无需依赖 agent crate。

use async_trait::async_trait;
use serde::Serialize;

/// 通道状态（跨 crate 共享的数据结构）。
#[derive(Debug, Clone, Serialize)]
pub struct TunStatus {
    pub active: bool,
    pub service_id: Option<String>,
    pub url: Option<String>,
    pub mode: String,
}

/// Tun 通道控制接口。
#[async_trait]
pub trait TunControl: Send + Sync {
    /// 开通通道。
    async fn open(&self) -> anyhow::Result<TunStatus>;
    /// 关闭通道。
    async fn close(&self) -> anyhow::Result<()>;
    /// 查询状态。
    async fn status(&self) -> TunStatus;
}
