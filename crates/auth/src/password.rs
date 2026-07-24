//! 密码哈希（argon2）。

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PasswordError {
    #[error("password hash failed: {0}")]
    Hash(String),
    #[error("password verify failed")]
    Verify,
}

/// 哈希密码。返回 argon2 PHC 字符串（含 salt + 参数）。
pub fn hash_password(plain: &str) -> Result<String, PasswordError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| PasswordError::Hash(e.to_string()))
}

/// 校验密码。成功返回 Ok(())，失败返回 Err。
pub fn verify_password(plain: &str, phc: &str) -> Result<(), PasswordError> {
    let parsed = PasswordHash::new(phc).map_err(|_| PasswordError::Verify)?;
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .map_err(|_| PasswordError::Verify)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let h = hash_password("s3cret-pass").unwrap();
        assert!(verify_password("s3cret-pass", &h).is_ok());
        assert!(verify_password("wrong", &h).is_err());
    }
}
