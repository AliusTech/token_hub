//! TokenHub Agent：桌面常驻节点（带 FRP 远程接入的迷你 Server）。
//!
//! Agent 模式 = Server 模式 + 本地化绑定(127.0.0.1) + FRP 客户端内嵌。
//! 核心逻辑完全复用 api/storage/auth/billing/router-llm crate。

pub mod frpc;
pub mod runtime;
pub mod tun_controller;

pub use frpc::{generate_frpc_toml, generate_service_id, FrpcConfig, TunnelMode};
pub use runtime::run_agent;
pub use tun_controller::TunController;
