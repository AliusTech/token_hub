//! 服务器密钥封装：用于 HMAC（API Token / session token）与 JWT 签名。

use hmac::Hmac;
use sha2::Sha256;

/// HMAC-SHA256 类型别名
pub type HmacSha256 = Hmac<Sha256>;

/// 服务器密钥。从配置加载，进程内共享。
#[derive(Clone)]
pub struct ServerSecret(pub Vec<u8>);

impl ServerSecret {
    pub fn new(secret: &str) -> Self {
        Self(secret.as_bytes().to_vec())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
