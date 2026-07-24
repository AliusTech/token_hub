//! API Token Repo（应用用户的静态 Bearer 凭证）。

use crate::SqliteStore;
use domain::{ApiToken, TokenStatus};
use sqlx::Row;

#[derive(Clone)]
pub struct TokenRepo {
    store: SqliteStore,
}

impl TokenRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    /// 创建 token，返回构造好的领域对象。
    pub async fn create(
        &self,
        id: &str,
        token_hash: &str,
        prefix: &str,
        account_id: &str,
        name: Option<&str>,
        expires_at: Option<i64>,
        now: i64,
    ) -> anyhow::Result<ApiToken> {
        sqlx::query(
            "INSERT INTO api_tokens (id, token_hash, prefix, account_id, name, status, expires_at, created_at, revoked_at) \
             VALUES (?, ?, ?, ?, ?, 'active', ?, ?, NULL)",
        )
        .bind(id)
        .bind(token_hash)
        .bind(prefix)
        .bind(account_id)
        .bind(name)
        .bind(expires_at)
        .bind(now)
        .execute(&self.store.pool)
        .await?;
        Ok(ApiToken {
            id: id.to_string(),
            token_hash: token_hash.to_string(),
            prefix: prefix.to_string(),
            account_id: account_id.to_string(),
            name: name.map(String::from),
            status: TokenStatus::Active,
            expires_at,
            created_at: now,
            revoked_at: None,
        })
    }

    /// 按 token_hash 查询（返回原始行，由调用方判断有效性）。
    pub async fn find_by_hash(&self, token_hash: &str) -> anyhow::Result<Option<ApiToken>> {
        let row = sqlx::query(
            "SELECT id, token_hash, prefix, account_id, name, status, expires_at, created_at, revoked_at \
             FROM api_tokens WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_token(&r)))
    }

    /// 列出某账号下的 token。
    pub async fn list_by_account(&self, account_id: &str) -> anyhow::Result<Vec<ApiToken>> {
        let rows = sqlx::query(
            "SELECT id, token_hash, prefix, account_id, name, status, expires_at, created_at, revoked_at \
             FROM api_tokens WHERE account_id = ? ORDER BY created_at DESC",
        )
        .bind(account_id)
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows.iter().map(map_token).collect())
    }

    /// 分页列出全部 token。
    pub async fn list_all(&self, limit: i64, offset: i64) -> anyhow::Result<Vec<ApiToken>> {
        let rows = sqlx::query(
            "SELECT id, token_hash, prefix, account_id, name, status, expires_at, created_at, revoked_at \
             FROM api_tokens ORDER BY created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows.iter().map(map_token).collect())
    }

    /// 按 id 吊销：status='revoked', revoked_at=now。返回是否命中行。
    pub async fn revoke(&self, id: &str, now: i64) -> anyhow::Result<bool> {
        let res =
            sqlx::query("UPDATE api_tokens SET status = 'revoked', revoked_at = ? WHERE id = ?")
                .bind(now)
                .bind(id)
                .execute(&self.store.pool)
                .await?;
        Ok(res.rows_affected() > 0)
    }

    /// 按 token_hash 吊销：status='revoked', revoked_at=now。返回是否命中行。
    pub async fn revoke_by_hash(&self, token_hash: &str, now: i64) -> anyhow::Result<bool> {
        let res = sqlx::query(
            "UPDATE api_tokens SET status = 'revoked', revoked_at = ? WHERE token_hash = ?",
        )
        .bind(now)
        .bind(token_hash)
        .execute(&self.store.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }
}

fn map_token(r: &sqlx::sqlite::SqliteRow) -> ApiToken {
    let status: String = r.get("status");
    ApiToken {
        id: r.get("id"),
        token_hash: r.get("token_hash"),
        prefix: r.get("prefix"),
        account_id: r.get("account_id"),
        name: r.get("name"),
        status: if status == "active" {
            TokenStatus::Active
        } else {
            TokenStatus::Revoked
        },
        expires_at: r.get("expires_at"),
        created_at: r.get("created_at"),
        revoked_at: r.get("revoked_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    async fn setup() -> (TokenRepo, String) {
        let store = crate::connect_in_memory().await.unwrap();
        // 建依赖的 account 行（与 credits.rs 测试同款）
        let account_id = format!("acct_{}", Uuid::new_v4());
        let now: i64 = 1_700_000_000_000;
        sqlx::query(
            "INSERT INTO accounts (id, status, created_at, updated_at) VALUES (?, 'active', ?, ?)",
        )
        .bind(&account_id)
        .bind(now)
        .bind(now)
        .execute(&store.pool)
        .await
        .unwrap();
        let repo = TokenRepo::new(store);
        (repo, account_id)
    }

    #[tokio::test]
    async fn create_find_revoke_lifecycle() {
        let (repo, acct) = setup().await;
        let id = format!("tok_{}", Uuid::new_v4());
        let hash = format!("hash_{}", Uuid::new_v4());

        let tok = repo
            .create(&id, &hash, "th_live_", &acct, Some("my token"), None, 1)
            .await
            .unwrap();
        assert_eq!(tok.account_id, acct);
        assert_eq!(tok.status, TokenStatus::Active);

        // 按哈希查询
        let found = repo.find_by_hash(&hash).await.unwrap().unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.name.as_deref(), Some("my token"));
        assert_eq!(found.status, TokenStatus::Active);

        // 列表
        let list = repo.list_by_account(&acct).await.unwrap();
        assert_eq!(list.len(), 1);
        let all = repo.list_all(10, 0).await.unwrap();
        assert_eq!(all.len(), 1);

        // 按 id 吊销
        assert!(repo.revoke(&id, 2).await.unwrap());
        let after = repo.find_by_hash(&hash).await.unwrap().unwrap();
        assert_eq!(after.status, TokenStatus::Revoked);
        assert_eq!(after.revoked_at, Some(2));

        // 重复吊销仍返回 true（行存在）
        assert!(repo.revoke(&id, 3).await.unwrap());
    }

    #[tokio::test]
    async fn revoke_by_hash_works() {
        let (repo, acct) = setup().await;
        let id = format!("tok_{}", Uuid::new_v4());
        let hash = format!("hash_{}", Uuid::new_v4());
        repo.create(&id, &hash, "th_live_", &acct, None, None, 1)
            .await
            .unwrap();

        assert!(repo.revoke_by_hash(&hash, 5).await.unwrap());
        let after = repo.find_by_hash(&hash).await.unwrap().unwrap();
        assert_eq!(after.status, TokenStatus::Revoked);
        assert_eq!(after.revoked_at, Some(5));

        // 不存在的 hash 不命中
        assert!(!repo.revoke_by_hash("missing", 6).await.unwrap());
    }
}
