//! 管理员用户 Repo，含 access token / refresh token 管理。

use crate::SqliteStore;
use serde_json;
use sqlx::Row;

/// 管理员用户记录（读模型）。
#[derive(Debug, Clone)]
pub struct AdminUserRecord {
    pub id: String,
    pub phone: String,
    pub password_hash: String,
    pub totp_secret: String,
    pub roles: Vec<String>,
    pub status: String,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
}

/// 管理员 access token 记录。
#[derive(Debug, Clone)]
pub struct AccessTokenRecord {
    pub admin_id: String,
    pub expires_at: i64,
    pub revoked: bool,
}

#[derive(Clone)]
pub struct AdminUserRepo {
    store: SqliteStore,
}

impl AdminUserRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    /// 创建管理员。roles 序列化为 JSON 数组字符串存储。
    pub async fn create(
        &self,
        id: &str,
        phone: &str,
        password_hash: &str,
        totp_secret: &str,
        roles: &[&str],
        now: i64,
    ) -> anyhow::Result<()> {
        let roles_json = serde_json::to_string(roles)?;
        sqlx::query(
            "INSERT INTO admin_users (id, phone, password_hash, totp_secret, roles, status, created_at) \
             VALUES (?, ?, ?, ?, ?, 'active', ?)",
        )
        .bind(id)
        .bind(phone)
        .bind(password_hash)
        .bind(totp_secret)
        .bind(&roles_json)
        .bind(now)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }

    /// 按 phone 查询管理员。
    pub async fn find_by_phone(&self, phone: &str) -> anyhow::Result<Option<AdminUserRecord>> {
        let row = sqlx::query(
            "SELECT id, phone, password_hash, totp_secret, roles, status, created_at, last_login_at \
             FROM admin_users WHERE phone = ?",
        )
        .bind(phone)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_admin_user(&r)))
    }

    /// 按 id 查询管理员。
    pub async fn find_by_id(&self, id: &str) -> anyhow::Result<Option<AdminUserRecord>> {
        let row = sqlx::query(
            "SELECT id, phone, password_hash, totp_secret, roles, status, created_at, last_login_at \
             FROM admin_users WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_admin_user(&r)))
    }

    /// 更新最后登录时间。
    pub async fn update_last_login(&self, id: &str, now: i64) -> anyhow::Result<()> {
        sqlx::query("UPDATE admin_users SET last_login_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        Ok(())
    }

    /// 设置状态。返回是否命中行。
    pub async fn set_status(&self, id: &str, status: &str, now: i64) -> anyhow::Result<bool> {
        let res = sqlx::query("UPDATE admin_users SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        let _ = now; // status 表无独立 updated_at
        Ok(res.rows_affected() > 0)
    }

    /// 重置 TOTP 密钥。返回是否命中行。
    pub async fn reset_totp(&self, id: &str, new_secret: &str, now: i64) -> anyhow::Result<bool> {
        let res = sqlx::query("UPDATE admin_users SET totp_secret = ? WHERE id = ?")
            .bind(new_secret)
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        let _ = now;
        Ok(res.rows_affected() > 0)
    }

    // ---- access token ----

    /// 创建 access token。
    pub async fn create_access_token(
        &self,
        token_hash: &str,
        admin_id: &str,
        expires_at: i64,
        now: i64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO admin_access_tokens (token_hash, admin_id, expires_at, created_at, revoked) \
             VALUES (?, ?, ?, ?, 0)",
        )
        .bind(token_hash)
        .bind(admin_id)
        .bind(expires_at)
        .bind(now)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }

    /// 查询 access token。
    pub async fn find_access_token(
        &self,
        token_hash: &str,
    ) -> anyhow::Result<Option<AccessTokenRecord>> {
        let row = sqlx::query(
            "SELECT admin_id, expires_at, revoked FROM admin_access_tokens WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| AccessTokenRecord {
            admin_id: r.get("admin_id"),
            expires_at: r.get("expires_at"),
            revoked: r.get::<i64, _>("revoked") != 0,
        }))
    }

    /// 吊销单个 access token。
    pub async fn revoke_access_token(&self, token_hash: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE admin_access_tokens SET revoked = 1 WHERE token_hash = ?")
            .bind(token_hash)
            .execute(&self.store.pool)
            .await?;
        Ok(())
    }

    /// 吊销某管理员名下所有 access token。
    pub async fn revoke_all_access_tokens(&self, admin_id: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE admin_access_tokens SET revoked = 1 WHERE admin_id = ?")
            .bind(admin_id)
            .execute(&self.store.pool)
            .await?;
        Ok(())
    }

    // ---- refresh token ----

    /// 创建 refresh token。
    pub async fn create_refresh_token(
        &self,
        token_hash: &str,
        admin_id: &str,
        access_token_hash: &str,
        expires_at: i64,
        now: i64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO admin_refresh_tokens (token_hash, admin_id, access_token_hash, expires_at, created_at, revoked) \
             VALUES (?, ?, ?, ?, ?, 0)",
        )
        .bind(token_hash)
        .bind(admin_id)
        .bind(access_token_hash)
        .bind(expires_at)
        .bind(now)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }

    /// 查询 refresh token，返回 (admin_id, access_token_hash, expires_at, revoked)。
    pub async fn find_refresh_token(
        &self,
        token_hash: &str,
    ) -> anyhow::Result<Option<(String, String, i64, bool)>> {
        let row = sqlx::query(
            "SELECT admin_id, access_token_hash, expires_at, revoked FROM admin_refresh_tokens WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| {
            (
                r.get("admin_id"),
                r.get("access_token_hash"),
                r.get("expires_at"),
                r.get::<i64, _>("revoked") != 0,
            )
        }))
    }

    /// 吊销 refresh token。
    pub async fn revoke_refresh_token(&self, token_hash: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE admin_refresh_tokens SET revoked = 1 WHERE token_hash = ?")
            .bind(token_hash)
            .execute(&self.store.pool)
            .await?;
        Ok(())
    }
}

fn map_admin_user(r: &sqlx::sqlite::SqliteRow) -> AdminUserRecord {
    let roles_json: String = r.get("roles");
    let roles: Vec<String> = serde_json::from_str(&roles_json).unwrap_or_default();
    AdminUserRecord {
        id: r.get("id"),
        phone: r.get("phone"),
        password_hash: r.get("password_hash"),
        totp_secret: r.get("totp_secret"),
        roles,
        status: r.get("status"),
        created_at: r.get("created_at"),
        last_login_at: r.get("last_login_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup() -> (AdminUserRepo, String) {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = AdminUserRepo::new(store);
        let id = format!("admin_{}", Uuid::new_v4());
        repo.create(
            &id,
            "13800000000",
            "$argon2id$hash",
            "TOTPSECRET",
            &["admin", "ops"],
            1_700_000_000_000,
        )
        .await
        .unwrap();
        (repo, id)
    }

    use uuid::Uuid;

    #[tokio::test]
    async fn create_find_by_phone_and_access_token_lifecycle() {
        let (repo, id) = setup().await;

        // 按 phone 查询
        let admin = repo.find_by_phone("13800000000").await.unwrap().unwrap();
        assert_eq!(admin.id, id);
        assert_eq!(admin.roles, vec!["admin".to_string(), "ops".to_string()]);
        assert_eq!(admin.status, "active");

        // 按 id 查询
        let by_id = repo.find_by_id(&id).await.unwrap().unwrap();
        assert_eq!(by_id.phone, "13800000000");

        // 创建 access token
        let tok = format!("tok_{}", Uuid::new_v4());
        repo.create_access_token(&tok, &id, 1_700_000_000_000 + 3600_000, 1)
            .await
            .unwrap();
        let rec = repo.find_access_token(&tok).await.unwrap().unwrap();
        assert_eq!(rec.admin_id, id);
        assert!(!rec.revoked);

        // 吊销
        repo.revoke_access_token(&tok).await.unwrap();
        let rec2 = repo.find_access_token(&tok).await.unwrap().unwrap();
        assert!(rec2.revoked);
    }
}
