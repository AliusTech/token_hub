//! FRP 客户端配置生成 + Service ID 生成。
//!
//! 对外文档称"远程管理接入"，不暴露 frp 实现细节。

use std::path::Path;
use std::process::Stdio;
use tokio::process::{Child, Command};

/// 可用的 Service ID 字符集（小写字母 + 数字，排除易混淆字符 0/o/1/i/l）
const SERVICE_ID_CHARS: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
const SERVICE_ID_LEN: usize = 6;

/// FRP 隧道模式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelMode {
    /// HTTP 子域名模式：subdomain = "a3k9x2" → a3k9x2.tun.alius.tech
    /// 需要 frps 配置 vhostHTTPPort + subdomainHost
    HttpSubdomain,
    /// TCP 端口映射模式：remotePort = 6001 → frp.alius.tech:6001
    Tcp { remote_port: u16 },
}

/// FRP 客户端配置（从 Agent 配置注入）。
#[derive(Debug, Clone)]
pub struct FrpcConfig {
    pub server: String,
    pub port: u16,
    pub token: Option<String>,
    /// 子域名主机（如 tun.alius.tech）
    pub subdomain_host: String,
    /// 本地目标地址（通常 127.0.0.1）
    pub local_addr: String,
    /// 本地 admin 端口
    pub local_port: u16,
    /// frpc 可执行文件路径（随 Agent 打包或 PATH 可用）
    pub frpc_binary: String,
    /// 隧道模式
    pub mode: TunnelMode,
}

/// 生成 6 位 Service ID（小写字母 + 数字，排除易混淆字符）。
/// 示例：a3k9x2, b7m2p8
pub fn generate_service_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..SERVICE_ID_LEN)
        .map(|_| {
            let idx = rng.gen_range(0..SERVICE_ID_CHARS.len());
            SERVICE_ID_CHARS[idx] as char
        })
        .collect()
}

/// 生成 frpc.toml 配置内容（使用指定的 service_id）。
pub fn generate_frpc_toml(cfg: &FrpcConfig, service_id: &str) -> String {
    let mut s = format!(
        r#"# TokenHub 远程管理接入配置（自动生成，请勿手动修改）
serverAddr = "{server}"
serverPort = {port}
"#,
        server = cfg.server,
        port = cfg.port,
    );
    if let Some(token) = &cfg.token {
        if !token.is_empty() {
            s.push_str(&format!("auth.token = \"{token}\"\n"));
        }
    }
    match &cfg.mode {
        TunnelMode::HttpSubdomain => {
            s.push_str(&format!(
                r#"
[[proxies]]
name = "tokenhub-{service_id}"
type = "http"
localIP = "{local_addr}"
localPort = {local_port}
subdomain = "{service_id}"
"#,
                service_id = service_id,
                local_addr = cfg.local_addr,
                local_port = cfg.local_port,
            ));
        }
        TunnelMode::Tcp { remote_port } => {
            s.push_str(&format!(
                r#"
[[proxies]]
name = "tokenhub-{service_id}"
type = "tcp"
localIP = "{local_addr}"
localPort = {local_port}
remotePort = {remote_port}
"#,
                service_id = service_id,
                local_addr = cfg.local_addr,
                local_port = cfg.local_port,
                remote_port = remote_port,
            ));
        }
    }
    s
}

/// 将配置写入临时文件并启动 frpc 子进程。
pub async fn start_frpc(
    cfg: &FrpcConfig,
    service_id: &str,
    config_dir: &Path,
) -> anyhow::Result<(Child, std::path::PathBuf)> {
    std::fs::create_dir_all(config_dir)?;
    let config_path = config_dir.join("frpc.toml");
    let toml_content = generate_frpc_toml(cfg, service_id);
    std::fs::write(&config_path, &toml_content)?;

    let child = Command::new(&cfg.frpc_binary)
        .arg("-c")
        .arg(&config_path)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn frpc at {}: {e}", cfg.frpc_binary))?;

    tracing::info!(
        binary = %cfg.frpc_binary,
        config = %config_path.display(),
        service_id = %service_id,
        "远程管理接入已启动 (frpc)"
    );

    Ok((child, config_path))
}

/// 监控 frpc 子进程：若退出则重启。阻塞直到收到 shutdown 信号。
pub async fn supervise_frpc(
    cfg: FrpcConfig,
    service_id: String,
    config_dir: std::path::PathBuf,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        match start_frpc(&cfg, &service_id, &config_dir).await {
            Ok((mut child, _)) => {
                tokio::select! {
                    status = child.wait() => {
                        match status {
                            Ok(s) => tracing::warn!(exit = ?s.code(), "frpc exited, will restart in 5s"),
                            Err(e) => tracing::warn!(error = %e, "frpc wait failed, will restart in 5s"),
                        }
                    }
                    _ = shutdown.changed() => {
                        tracing::info!("shutdown signal received, stopping frpc");
                        let _ = child.kill().await;
                        return;
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to start frpc");
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
            _ = shutdown.changed() => {
                tracing::info!("shutdown during frpc backoff");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(mode: TunnelMode) -> FrpcConfig {
        FrpcConfig {
            server: "frp.alius.tech".to_string(),
            port: 7000,
            token: Some("secret123".to_string()),
            subdomain_host: "tun.alius.tech".to_string(),
            local_addr: "127.0.0.1".to_string(),
            local_port: 8081,
            frpc_binary: "frpc".to_string(),
            mode,
        }
    }

    #[test]
    fn service_id_format() {
        for _ in 0..100 {
            let id = generate_service_id();
            assert_eq!(id.len(), 6, "service ID must be 6 chars");
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "must be lowercase+digits"
            );
            // 不含易混淆字符
            assert!(!id.contains('0'), "must not contain 0");
            assert!(!id.contains('o'), "must not contain o");
            assert!(!id.contains('1'), "must not contain 1");
            assert!(!id.contains('i'), "must not contain i");
            assert!(!id.contains('l'), "must not contain l");
        }
    }

    #[test]
    fn service_id_randomness() {
        let ids: std::collections::HashSet<_> = (0..50).map(|_| generate_service_id()).collect();
        assert!(
            ids.len() > 40,
            "50 generated IDs should be mostly unique, got {}",
            ids.len()
        );
    }

    #[test]
    fn http_subdomain_toml() {
        let cfg = test_config(TunnelMode::HttpSubdomain);
        let toml = generate_frpc_toml(&cfg, "a3k9x2");
        assert!(toml.contains("type = \"http\""));
        assert!(toml.contains("subdomain = \"a3k9x2\""));
        assert!(toml.contains("localPort = 8081"));
        assert!(toml.contains("auth.token = \"secret123\""));
        assert!(toml.contains("name = \"tokenhub-a3k9x2\""));
    }

    #[test]
    fn tcp_mode_toml() {
        let cfg = test_config(TunnelMode::Tcp { remote_port: 6001 });
        let toml = generate_frpc_toml(&cfg, "b7m2p8");
        assert!(toml.contains("type = \"tcp\""));
        assert!(toml.contains("remotePort = 6001"));
        assert!(!toml.contains("subdomain"));
    }

    #[test]
    fn no_token_omitted() {
        let mut cfg = test_config(TunnelMode::HttpSubdomain);
        cfg.token = None;
        let toml = generate_frpc_toml(&cfg, "test01");
        assert!(!toml.contains("auth.token"));
    }
}
