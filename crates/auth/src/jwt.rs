//! Service JWT：OAuth2 client_credentials 流颁发的短期 JWT。
//!
//! Claims：iss=client_id, exp, iat, scope（字符串数组）。
//! 用 HS256（HMAC-SHA256，复用 server_secret）签名。

use crate::secret::ServerSecret;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceClaims {
    /// client_id
    pub iss: String,
    /// 签发时间（Unix 秒）
    pub iat: i64,
    /// 过期时间（Unix 秒）
    pub exp: i64,
    /// 权限范围（scope 字符串数组）
    pub scope: Vec<String>,
    /// 主体类型，固定为 "service"
    pub sub_kind: String,
}

#[derive(Debug, Error)]
pub enum JwtError {
    #[error("jwt signing failed: {0}")]
    Sign(String),
    #[error("jwt invalid or expired")]
    Invalid,
}

/// 签发 Service JWT。ttl_secs 为有效期秒数。
pub fn issue_service_jwt(
    client_id: &str,
    scopes: &[String],
    ttl_secs: i64,
    secret: &ServerSecret,
) -> Result<String, JwtError> {
    let now = chrono::Utc::now().timestamp();
    let claims = ServiceClaims {
        iss: client_id.to_string(),
        iat: now,
        exp: now + ttl_secs,
        scope: scopes.to_vec(),
        sub_kind: "service".to_string(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| JwtError::Sign(e.to_string()))
}

/// 校验 Service JWT，返回解析后的 claims。过期/篡改返回 Err。
pub fn verify_service_jwt(token: &str, secret: &ServerSecret) -> Result<ServiceClaims, JwtError> {
    let mut validation = Validation::default();
    // leeway=0：严格按 exp 判断，不容忍时钟漂移（JWT 已是短生命周期）
    validation.leeway = 0;
    let data = decode::<ServiceClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| JwtError::Invalid)?;
    if data.claims.sub_kind != "service" {
        return Err(JwtError::Invalid);
    }
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_and_verify_roundtrip() {
        let secret = ServerSecret::new("jwt-test-secret");
        let scopes = vec!["accounts.read".to_string(), "credits.write".to_string()];
        let token = issue_service_jwt("svc_1", &scopes, 3600, &secret).unwrap();
        let claims = verify_service_jwt(&token, &secret).unwrap();
        assert_eq!(claims.iss, "svc_1");
        assert_eq!(claims.scope, scopes);
        assert_eq!(claims.sub_kind, "service");
    }

    #[test]
    fn tampered_token_rejected() {
        let secret = ServerSecret::new("jwt-test-secret");
        let token = issue_service_jwt("svc_1", &[], 3600, &secret).unwrap();
        // 篡改最后一个字符
        let mut tampered = token.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'a' { 'b' } else { 'a' });
        assert!(verify_service_jwt(&tampered, &secret).is_err());
    }

    #[test]
    fn wrong_secret_rejected() {
        let secret1 = ServerSecret::new("secret-1");
        let secret2 = ServerSecret::new("secret-2");
        let token = issue_service_jwt("svc_1", &[], 3600, &secret1).unwrap();
        assert!(verify_service_jwt(&token, &secret2).is_err());
    }

    #[test]
    fn expired_token_rejected() {
        let secret = ServerSecret::new("jwt-test-secret");
        // ttl = -10 表示已过期 10 秒
        let token = issue_service_jwt("svc_1", &[], -10, &secret).unwrap();
        assert!(verify_service_jwt(&token, &secret).is_err());
    }
}
