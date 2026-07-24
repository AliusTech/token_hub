//! 认证层：三类身份的凭证生成与校验。
//!
//! - `password`：argon2 密码哈希
//! - `totp`：TOTP 生成/校验（RFC 6238）
//! - `token_hmac`：API Token 生成 + HMAC-SHA256 哈希
//! - `jwt`：Service 客户端凭证 JWT 签发/校验
//! - `session`：Admin 有状态 access token 生成（随机串）
//! - `secret`：HMAC/JWT 密钥封装

pub mod jwt;
pub mod password;
pub mod secret;
pub mod session;
pub mod token_hmac;
pub mod totp;

pub use jwt::{issue_service_jwt, verify_service_jwt, ServiceClaims};
pub use password::{hash_password, verify_password};
pub use secret::ServerSecret;
pub use session::{generate_session_token, hash_session_token};
pub use token_hmac::{generate_api_token, hash_api_token};
pub use totp::{generate_totp_secret, totp_qrcode_datauri, verify_totp};
