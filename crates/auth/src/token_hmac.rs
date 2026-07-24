//! API Token：生成明文 + HMAC-SHA256 哈希（用于 DB 查表，不存明文）。

use crate::secret::{HmacSha256, ServerSecret};
use domain::token::TOKEN_PREFIX_LITERAL;
use hmac::Mac;
use rand::RngCore;

/// 生成新的 API Token 明文（如 `th_live_` + 32 字节随机 hex）。
/// 返回明文。调用方：明文返回给用户一次，hash 存 DB。
pub fn generate_api_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{TOKEN_PREFIX_LITERAL}{hex}")
}

/// 计算 token 的 HMAC-SHA256 hex 哈希（用于 DB 唯一索引 + 查表）。
/// 使用 HMAC 而非裸 SHA256：让 hash 依赖服务器密钥，即使 DB 泄露也无法直接比对。
pub fn hash_api_token(token: &str, secret: &ServerSecret) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(token.as_bytes());
    let bytes = mac.finalize().into_bytes();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_generation_and_hash() {
        let secret = ServerSecret::new("test-server-secret");
        let t1 = generate_api_token();
        let t2 = generate_api_token();
        assert!(t1.starts_with(TOKEN_PREFIX_LITERAL));
        assert_ne!(t1, t2, "tokens must be unique");

        // 同一 token + 同一密钥 → 同一 hash（幂等）
        let h1 = hash_api_token(&t1, &secret);
        let h2 = hash_api_token(&t1, &secret);
        assert_eq!(h1, h2);

        // 不同 token → 不同 hash
        let h3 = hash_api_token(&t2, &secret);
        assert_ne!(h1, h3);

        // 不同密钥 → 不同 hash（DB 泄露防护）
        let secret2 = ServerSecret::new("different-secret");
        let h4 = hash_api_token(&t1, &secret2);
        assert_ne!(h1, h4);
    }
}
