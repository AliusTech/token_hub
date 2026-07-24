//! TOTP（RFC 6238）：生成 secret、校验 6 位码、生成绑定 URL。

use thiserror::Error;
use totp_rs::{Algorithm, Secret, TOTP};

#[derive(Debug, Error)]
pub enum TotpError {
    #[error("invalid totp secret: {0}")]
    InvalidSecret(String),
    #[error("totp verification failed")]
    VerifyFailed,
}

/// 生成一个新的 Base32 TOTP secret（用于首次绑定 / 重置）。
pub fn generate_totp_secret() -> String {
    Secret::generate_secret().to_encoded().to_string()
}

/// 解码 secret bytes。
fn decode_secret(secret: &str) -> Result<Vec<u8>, TotpError> {
    Secret::Encoded(secret.to_string())
        .to_bytes()
        .map_err(|e| TotpError::InvalidSecret(e.to_string()))
}

/// 用 secret 构造 TOTP 实例。skew=1 容忍前后各 30s 时钟漂移。
fn build_totp(secret: &str) -> Result<TOTP, TotpError> {
    let bytes = decode_secret(secret)?;
    Ok(TOTP::new_unchecked(Algorithm::SHA1, 6, 1, 30, bytes))
}

/// 校验用户输入的 6 位 TOTP 码。
pub fn verify_totp(secret: &str, code: &str) -> Result<(), TotpError> {
    let totp = build_totp(secret)?;
    let now = chrono::Utc::now().timestamp().max(0) as u64;
    if totp.check(code, now) {
        Ok(())
    } else {
        Err(TotpError::VerifyFailed)
    }
}

/// 在指定时刻生成正确的 TOTP 码（测试辅助）。
pub fn generate_at(secret: &str, time: u64) -> Result<String, TotpError> {
    let totp = build_totp(secret)?;
    Ok(totp.generate(time))
}

/// 生成 otpauth 绑定 URL（前端用 qrcode 库渲染为二维码）。
/// 手动拼接，不依赖 otpauth feature。
pub fn totp_qrcode_datauri(secret: &str, issuer: &str, account: &str) -> Result<String, TotpError> {
    let issuer_enc = urlenc(issuer);
    let account_enc = urlenc(account);
    Ok(format!(
        "otpauth://totp/{issuer_enc}:{account_enc}?secret={secret}&issuer={issuer_enc}&algorithm=SHA1&digits=6&period=30"
    ))
}

fn urlenc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            ':' => "%3A".to_string(),
            '?' => "%3F".to_string(),
            '&' => "%26".to_string(),
            '=' => "%3D".to_string(),
            c if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' => {
                c.to_string()
            }
            c => format!("%{:02X}", c as u32),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_generate_and_verify_current() {
        let secret = generate_totp_secret();
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        let code = generate_at(&secret, now).unwrap();
        assert!(verify_totp(&secret, &code).is_ok());
    }

    #[test]
    fn invalid_secret_rejected() {
        assert!(build_totp("not-a-valid-base32!!!").is_err());
    }

    #[test]
    fn skew_allows_clock_drift() {
        let secret = generate_totp_secret();
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        // 30 秒前的码也应被接受（skew=1）
        let code_past = generate_at(&secret, now.saturating_sub(30)).unwrap();
        assert!(verify_totp(&secret, &code_past).is_ok());
    }

    #[test]
    fn otpauth_url_well_formed() {
        let secret = generate_totp_secret();
        let url = totp_qrcode_datauri(&secret, "TokenHub", "admin@x.com").unwrap();
        assert!(url.starts_with("otpauth://totp/"));
        assert!(url.contains("secret="));
        assert!(url.contains("issuer=TokenHub"));
    }
}
