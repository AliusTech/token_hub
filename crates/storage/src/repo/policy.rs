//! 策略模板 Repo（可用模型集、月度上限）。

use crate::SqliteStore;
use domain::Policy;
use serde_json;
use sqlx::Row;

#[derive(Clone)]
pub struct PolicyRepo {
    store: SqliteStore,
}

impl PolicyRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        id: &str,
        name: &str,
        allowed_models: &[&str],
        monthly_credit_cap: Option<i64>,
        description: Option<&str>,
        now: i64,
    ) -> anyhow::Result<Policy> {
        let models_json = serde_json::to_string(allowed_models)?;
        sqlx::query(
            "INSERT INTO policies (id, name, allowed_models, monthly_credit_cap, description, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(&models_json)
        .bind(monthly_credit_cap)
        .bind(description)
        .bind(now)
        .bind(now)
        .execute(&self.store.pool)
        .await?;

        Ok(Policy {
            id: id.to_string(),
            name: name.to_string(),
            allowed_models: allowed_models.iter().map(|s| (*s).to_string()).collect(),
            monthly_credit_cap,
            description: description.map(String::from),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Option<Policy>> {
        let row = sqlx::query(
            "SELECT id, name, allowed_models, monthly_credit_cap, description, created_at, updated_at \
             FROM policies WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_policy(&r)))
    }

    pub async fn get_by_name(&self, name: &str) -> anyhow::Result<Option<Policy>> {
        let row = sqlx::query(
            "SELECT id, name, allowed_models, monthly_credit_cap, description, created_at, updated_at \
             FROM policies WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_policy(&r)))
    }

    pub async fn list(&self) -> anyhow::Result<Vec<Policy>> {
        let rows = sqlx::query(
            "SELECT id, name, allowed_models, monthly_credit_cap, description, created_at, updated_at \
             FROM policies ORDER BY created_at DESC",
        )
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows.iter().map(map_policy).collect())
    }

    pub async fn update(
        &self,
        id: &str,
        name: &str,
        allowed_models: &[&str],
        monthly_credit_cap: Option<i64>,
        description: Option<&str>,
        now: i64,
    ) -> anyhow::Result<bool> {
        let models_json = serde_json::to_string(allowed_models)?;
        let res = sqlx::query(
            "UPDATE policies SET name = ?, allowed_models = ?, monthly_credit_cap = ?, description = ?, updated_at = ? \
             WHERE id = ?",
        )
        .bind(name)
        .bind(&models_json)
        .bind(monthly_credit_cap)
        .bind(description)
        .bind(now)
        .bind(id)
        .execute(&self.store.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let res = sqlx::query("DELETE FROM policies WHERE id = ?")
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}

fn map_policy(r: &sqlx::sqlite::SqliteRow) -> Policy {
    let allowed_models: String = r.get("allowed_models");
    let allowed_models: Vec<String> = serde_json::from_str(&allowed_models).unwrap_or_default();
    Policy {
        id: r.get("id"),
        name: r.get("name"),
        allowed_models,
        monthly_credit_cap: r.get("monthly_credit_cap"),
        description: r.get("description"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_id() -> String {
        format!("pol_{}", uuid::Uuid::new_v4())
    }

    #[tokio::test]
    async fn create_and_get() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = PolicyRepo::new(store);
        let id = new_id();
        let p = repo
            .create(
                &id,
                "default",
                &["gpt-4", "claude"],
                Some(100_000),
                Some("d"),
                1,
            )
            .await
            .unwrap();
        assert_eq!(p.name, "default");
        assert_eq!(
            p.allowed_models,
            vec!["gpt-4".to_string(), "claude".to_string()]
        );
        assert_eq!(p.monthly_credit_cap, Some(100_000));

        let got = repo.get(&id).await.unwrap().unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.allowed_models.len(), 2);
    }

    #[tokio::test]
    async fn get_by_name_works() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = PolicyRepo::new(store);
        let id = new_id();
        repo.create(&id, "enterprise", &["gpt-4"], None, None, 1)
            .await
            .unwrap();
        let got = repo.get_by_name("enterprise").await.unwrap().unwrap();
        assert_eq!(got.id, id);
        assert!(repo.get_by_name("missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn update_changes_fields() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = PolicyRepo::new(store);
        let id = new_id();
        repo.create(&id, "to_update", &["a"], Some(100), None, 1)
            .await
            .unwrap();
        assert!(repo
            .update(&id, "renamed", &["b", "c"], Some(500), Some("desc"), 2)
            .await
            .unwrap());
        let got = repo.get(&id).await.unwrap().unwrap();
        assert_eq!(got.name, "renamed");
        assert_eq!(got.allowed_models, vec!["b".to_string(), "c".to_string()]);
        assert_eq!(got.monthly_credit_cap, Some(500));
        assert_eq!(got.description.as_deref(), Some("desc"));
        assert_eq!(got.updated_at, 2);
    }

    #[tokio::test]
    async fn delete_removes_row() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = PolicyRepo::new(store);
        let id = new_id();
        repo.create(&id, "to_delete", &["a"], None, None, 1)
            .await
            .unwrap();
        assert!(repo.delete(&id).await.unwrap());
        assert!(repo.get(&id).await.unwrap().is_none());
        // 二次删除应返回 false
        assert!(!repo.delete(&id).await.unwrap());
    }

    #[tokio::test]
    async fn list_returns_all() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = PolicyRepo::new(store);
        let id1 = new_id();
        let id2 = new_id();
        repo.create(&id1, "p1", &["a"], None, None, 1)
            .await
            .unwrap();
        repo.create(&id2, "p2", &["b"], None, None, 2)
            .await
            .unwrap();
        let all = repo.list().await.unwrap();
        assert_eq!(all.len(), 2);
    }
}
