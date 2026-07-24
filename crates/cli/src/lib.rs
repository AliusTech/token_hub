//! CLI 子命令。
//!
//! `tokenhub admin create` — 初始化首个管理员
//! `tokenhub tun toggle`   — 切换远程管理通道
//! `tokenhub tun status`   — 查询通道状态

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tokenhub", version, about = "TokenHub management CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// 管理员账号管理
    Admin {
        #[command(subcommand)]
        action: AdminAction,
    },
    /// 远程管理通道（tun）
    Tun {
        #[command(subcommand)]
        action: TunAction,
    },
}

#[derive(Subcommand)]
pub enum AdminAction {
    /// 创建管理员（交互式输入手机号、密码，生成 TOTP）
    Create {
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
        #[arg(long, env = "SERVER_SECRET")]
        server_secret: String,
        #[arg(long)]
        phone: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long, default_value = "super_admin")]
        role: String,
    },
}

#[derive(Subcommand)]
pub enum TunAction {
    /// 切换通道（关→开 / 开→关）
    Toggle {
        /// Admin API 地址（如 http://localhost:8081）
        #[arg(long, env = "ADMIN_URL", default_value = "http://localhost:8081")]
        admin_url: String,
        /// 管理员 access token
        #[arg(long, env = "ADMIN_TOKEN")]
        token: String,
    },
    /// 查询通道状态
    Status {
        #[arg(long, env = "ADMIN_URL", default_value = "http://localhost:8081")]
        admin_url: String,
        #[arg(long, env = "ADMIN_TOKEN")]
        token: String,
    },
}

/// 运行 CLI。
pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Admin { action } => match action {
            AdminAction::Create {
                database_url,
                server_secret,
                phone,
                password,
                role,
            } => create_admin(&database_url, &server_secret, phone, password, &role).await,
        },
        Command::Tun { action } => match action {
            TunAction::Toggle { admin_url, token } => tun_toggle(&admin_url, &token).await,
            TunAction::Status { admin_url, token } => tun_status(&admin_url, &token).await,
        },
    }
}

async fn create_admin(
    database_url: &str,
    server_secret: &str,
    phone: Option<String>,
    password: Option<String>,
    role: &str,
) -> Result<()> {
    use std::io::{self, BufRead, Write};

    let store = storage::connect(database_url)
        .await
        .context("failed to connect to database")?;

    let phone = match phone {
        Some(p) => p,
        None => {
            print!("Enter admin phone: ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().lock().read_line(&mut input)?;
            input.trim().to_string()
        }
    };

    let password = match password {
        Some(p) => p,
        None => {
            print!("Enter password: ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().lock().read_line(&mut input)?;
            input.trim().to_string()
        }
    };

    if phone.is_empty() || password.is_empty() {
        anyhow::bail!("phone and password must not be empty");
    }

    let totp_secret = auth::totp::generate_totp_secret();
    let password_hash = auth::hash_password(&password).context("password hashing failed")?;

    let id = format!("adm_{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now().timestamp_millis();
    let roles = vec![role];

    let admin_repo = storage::AdminUserRepo::new(store.clone());
    admin_repo
        .create(&id, &phone, &password_hash, &totp_secret, &roles, now)
        .await
        .context("failed to create admin")?;

    let otpauth_url = auth::totp::totp_qrcode_datauri(&totp_secret, "TokenHub", &phone)
        .context("failed to generate otpauth url")?;

    println!("\n✓ Admin created successfully!");
    println!("  ID:    {}", id);
    println!("  Phone: {}", phone);
    println!("  Role:  {}", role);
    println!("\n  TOTP Secret (Base32): {}", totp_secret);
    println!("\n  Scan this QR code with your authenticator app:");
    println!("  {}", otpauth_url);

    Ok(())
}

async fn tun_toggle(admin_url: &str, token: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{admin_url}/v1/admin/tun/toggle"))
        .bearer_auth(token)
        .send()
        .await
        .context("failed to call tun/toggle")?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        anyhow::bail!("tun/toggle failed ({status}): {body}");
    }

    // 解析返回的 JSON 显示结果
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
        let active = v.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
        let service_id = v.get("service_id").and_then(|v| v.as_str()).unwrap_or("N/A");
        let url = v.get("url").and_then(|v| v.as_str()).unwrap_or("N/A");

        if active {
            println!("\n✓ Tun 通道已开通！");
            println!("  Service ID: {service_id}");
            println!("  访问地址:   https://{url}");
            println!("\n  手机端访问此地址即可远程管理 TokenHub。");
        } else {
            println!("\n✓ Tun 通道已关闭。");
        }
    } else {
        println!("{body}");
    }
    Ok(())
}

async fn tun_status(admin_url: &str, token: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{admin_url}/v1/admin/tun/status"))
        .bearer_auth(token)
        .send()
        .await
        .context("failed to call tun/status")?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        anyhow::bail!("tun/status failed ({status}): {body}");
    }

    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
        let active = v.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
        let service_id = v.get("service_id").and_then(|v| v.as_str()).unwrap_or("N/A");
        let url = v.get("url").and_then(|v| v.as_str()).unwrap_or("N/A");

        println!("\nTun 通道状态：");
        println!("  活跃:      {}", if active { "✅ 是" } else { "❌ 否" });
        println!("  Service ID: {service_id}");
        println!("  访问地址:   {url}");
    } else {
        println!("{body}");
    }
    Ok(())
}
