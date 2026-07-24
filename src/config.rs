//! 配置加载：环境变量优先（Docker 友好），支持 .env / config.toml 兜底。

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    #[default]
    Server,
    Agent,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub chat_listen: String,
    pub admin_listen: String,
    /// HMAC secret：API Token 哈希、session token 等
    pub server_secret: String,
    /// 运行模式：server | agent（默认 server）
    #[serde(default)]
    pub run_mode: RunMode,
    // === Agent 模式专用 ===
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub agent_platform: Option<String>,
    #[serde(default)]
    pub frp_server: Option<String>,
    #[serde(default)]
    pub frp_port: Option<u16>,
    #[serde(default)]
    pub frp_token: Option<String>,
    #[serde(default)]
    pub frp_subdomain: Option<String>,
    /// 隧道模式：http_subdomain（默认）| tcp
    #[serde(default)]
    pub frp_mode: String,
    /// TCP 模式的远程端口（allowPorts 范围内，如 6001）
    #[serde(default)]
    pub frp_remote_port: Option<u16>,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub device_key: Option<String>,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let run_mode_str = env_opt("RUN_MODE")
            .or(env_opt("run_mode"))
            .unwrap_or_else(|| "server".to_string());
        let cfg = config::Config::builder()
            .set_default("chat_listen", "0.0.0.0:8080")?
            .set_default("admin_listen", "0.0.0.0:8081")?
            .set_default("run_mode", run_mode_str.clone())?
            .set_default("database_url", "sqlite:///data/tokenhub.db")?
            .set_default("redis_url", "memory")?
            .set_default("server_secret", "")?
            .set_override_option(
                "database_url",
                env_opt("DATABASE_URL").or(env_opt("database_url")),
            )?
            .set_override_option("redis_url", env_opt("REDIS_URL").or(env_opt("redis_url")))?
            .set_override_option(
                "chat_listen",
                env_opt("CHAT_LISTEN").or(env_opt("chat_listen")),
            )?
            .set_override_option(
                "admin_listen",
                env_opt("ADMIN_LISTEN").or(env_opt("admin_listen")),
            )?
            .set_override_option(
                "server_secret",
                env_opt("SERVER_SECRET").or(env_opt("server_secret")),
            )?
            .set_override_option("run_mode", env_opt("RUN_MODE").or(env_opt("run_mode")))?
            .set_override_option(
                "agent_name",
                env_opt("AGENT_NAME").or(env_opt("agent_name")),
            )?
            .set_override_option(
                "agent_platform",
                env_opt("AGENT_PLATFORM").or(env_opt("agent_platform")),
            )?
            .set_override_option(
                "frp_server",
                env_opt("FRP_SERVER").or(env_opt("frp_server")),
            )?
            .set_override_option(
                "frp_port",
                env_opt("FRP_PORT")
                    .or(env_opt("frp_port"))
                    .and_then(|s| s.parse::<u16>().ok())
                    .map(|v| v.to_string()),
            )?
            .set_override_option("frp_token", env_opt("FRP_TOKEN").or(env_opt("frp_token")))?
            .set_override_option(
                "frp_subdomain",
                env_opt("FRP_SUBDOMAIN").or(env_opt("frp_subdomain")),
            )?
            .set_override_option("frp_mode", env_opt("FRP_MODE").or(env_opt("frp_mode")))?
            .set_override_option(
                "frp_remote_port",
                env_opt("FRP_REMOTE_PORT")
                    .or(env_opt("frp_remote_port"))
                    .and_then(|s| s.parse::<u16>().ok())
                    .map(|v| v.to_string()),
            )?
            .set_override_option("device_id", env_opt("DEVICE_ID").or(env_opt("device_id")))?
            .set_override_option(
                "device_key",
                env_opt("DEVICE_KEY").or(env_opt("device_key")),
            )?
            .build()?;

        let cfg: Config = cfg.try_deserialize()?;
        Ok(cfg)
    }
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}
