//! 账号 Repo。

use crate::SqliteStore;
use domain::{Account, AccountStatus};

#[derive(Clone)]
pub struct AccountRepo {
    store: SqliteStore,
}

impl AccountRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        id: &str,
        external_id: Option<&str>,
        policy_id: Option<&str>,
        note: Option<&str>,
        now: i64,
    ) -> anyhow::Result<Account> {
        sqlx::query(
            "INSERT INTO accounts (id, external_id, status, policy_id, note, created_at, updated_at) \
             VALUES (?, ?, 'active', ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(external_id)
        .bind(policy_id)
        .bind(note)
        .bind(now)
        .bind(now)
        .execute(&self.store.pool)
        .await?;
        Ok(Account {
            id: id.to_string(),
            external_id: external_id.map(String::from),
            status: AccountStatus::Active,
            policy_id: policy_id.map(String::from),
            note: note.map(String::from),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Option<Account>> {
        let row = sqlx::query(
            "SELECT id, external_id, status, policy_id, note, created_at, updated_at FROM accounts WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_account(&r)))
    }

    pub async fn list(
        &self,
        status: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<Account>> {
        let rows = if let Some(st) = status {
            sqlx::query(
                "SELECT id, external_id, status, policy_id, note, created_at, updated_at \
                 FROM accounts WHERE status = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(st)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.store.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, external_id, status, policy_id, note, created_at, updated_at \
                 FROM accounts ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.store.pool)
            .await?
        };
        Ok(rows.iter().map(map_account).collect())
    }

    pub async fn set_status(&self, id: &str, status: &str, now: i64) -> anyhow::Result<bool> {
        let res = sqlx::query("UPDATE accounts SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(now)
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn set_policy(
        &self,
        id: &str,
        policy_id: Option<&str>,
        now: i64,
    ) -> anyhow::Result<bool> {
        let res = sqlx::query("UPDATE accounts SET policy_id = ?, updated_at = ? WHERE id = ?")
            .bind(policy_id)
            .bind(now)
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}

fn map_account(r: &sqlx::sqlite::SqliteRow) -> Account {
    use sqlx::Row;
    let status: String = r.get("status");
    Account {
        id: r.get("id"),
        external_id: r.get("external_id"),
        status: if status == "active" {
            AccountStatus::Active
        } else {
            AccountStatus::Disabled
        },
        policy_id: r.get("policy_id"),
        note: r.get("note"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn account_crud() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = AccountRepo::new(store);
        let a = repo
            .create("acct_1", Some("zhangsan"), None, Some("note"), 1)
            .await
            .unwrap();
        assert_eq!(a.id, "acct_1");
        let got = repo.get("acct_1").await.unwrap().unwrap();
        assert_eq!(got.external_id.as_deref(), Some("zhangsan"));
        assert!(repo.set_status("acct_1", "disabled", 2).await.unwrap());
        let list = repo.list(Some("disabled"), 10, 0).await.unwrap();
        assert_eq!(list.len(), 1);
        let active = repo.list(Some("active"), 10, 0).await.unwrap();
        assert!(active.is_empty());
    }
}
