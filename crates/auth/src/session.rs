//! Admin 有状态 access token：随机串生成 + HMAC 哈希（DB 存 hash，可即时吊销）。

use crate::secret::{HmacSha256, ServerSecret};
use hmac::Mac;
use rand::RngCore;

/// 生成 access/refresh token 明文（48 字节随机 → base64url，无 padding）。
pub fn generate_session_token() -> String {
    let mut bytes = [0u8; 48];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64url(&bytes)
}

/// 计算 session token 的 HMAC-SHA256 hex 哈希（存 DB 主键，可即时吊销）。
pub fn hash_session_token(token: &str, secret: &ServerSecret) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(token.as_bytes());
    let bytes = mac.finalize().into_bytes();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 简单 base64url 编码（URL 安全，无 padding）。
fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &b in bytes {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            out.push(TABLE[((buf >> bits) & 0x3F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(TABLE[((buf << (6 - bits)) & 0x3F) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_token_unique_and_hashable() {
        let secret = ServerSecret::new("session-secret");
        let t1 = generate_session_token();
        let t2 = generate_session_token();
        assert_ne!(t1, t2);
        assert!(t1.len() >= 60);

        let h1 = hash_session_token(&t1, &secret);
        let h2 = hash_session_token(&t1, &secret);
        assert_eq!(h1, h2, "hash must be deterministic");
        assert_ne!(h1, hash_session_token(&t2, &secret));
    }
}
