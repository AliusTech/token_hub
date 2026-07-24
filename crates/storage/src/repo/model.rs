//! 逻辑模型与模型-供应商映射 Repo。

use crate::SqliteStore;
use domain::{LogicalModel, ModelProvider, RoutingStrategy};
use sqlx::Row;
use uuid::Uuid;

#[derive(Clone)]
pub struct ModelRepo {
    store: SqliteStore,
}

impl ModelRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        id: &str,
        logical_name: &str,
        description: Option<&str>,
        input_rate_per_1k: i64,
        output_rate_per_1k: i64,
        now: i64,
    ) -> anyhow::Result<LogicalModel> {
        sqlx::query(
            "INSERT INTO models (id, logical_name, description, input_rate_per_1k, output_rate_per_1k, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 'active', ?, ?)",
        )
        .bind(id)
        .bind(logical_name)
        .bind(description)
        .bind(input_rate_per_1k)
        .bind(output_rate_per_1k)
        .bind(now)
        .bind(now)
        .execute(&self.store.pool)
        .await?;
        Ok(LogicalModel {
            id: id.to_string(),
            logical_name: logical_name.to_string(),
            description: description.map(String::from),
            input_rate_per_1k,
            output_rate_per_1k,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Option<LogicalModel>> {
        let row = sqlx::query(
            "SELECT id, logical_name, description, input_rate_per_1k, output_rate_per_1k, status, created_at, updated_at \
             FROM models WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_model(&r)))
    }

    pub async fn get_by_name(&self, logical_name: &str) -> anyhow::Result<Option<LogicalModel>> {
        let row = sqlx::query(
            "SELECT id, logical_name, description, input_rate_per_1k, output_rate_per_1k, status, created_at, updated_at \
             FROM models WHERE logical_name = ?",
        )
        .bind(logical_name)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_model(&r)))
    }

    pub async fn list(&self) -> anyhow::Result<Vec<LogicalModel>> {
        let rows = sqlx::query(
            "SELECT id, logical_name, description, input_rate_per_1k, output_rate_per_1k, status, created_at, updated_at \
             FROM models ORDER BY created_at",
        )
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows.iter().map(map_model).collect())
    }

    pub async fn update(
        &self,
        id: &str,
        logical_name: &str,
        description: Option<&str>,
        input_rate_per_1k: i64,
        output_rate_per_1k: i64,
        status: &str,
        now: i64,
    ) -> anyhow::Result<bool> {
        let res = sqlx::query(
            "UPDATE models SET logical_name = ?, description = ?, input_rate_per_1k = ?, output_rate_per_1k = ?, status = ?, updated_at = ? \
             WHERE id = ?",
        )
        .bind(logical_name)
        .bind(description)
        .bind(input_rate_per_1k)
        .bind(output_rate_per_1k)
        .bind(status)
        .bind(now)
        .bind(id)
        .execute(&self.store.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn set_rates(
        &self,
        id: &str,
        input_rate_per_1k: i64,
        output_rate_per_1k: i64,
        now: i64,
    ) -> anyhow::Result<bool> {
        let res = sqlx::query(
            "UPDATE models SET input_rate_per_1k = ?, output_rate_per_1k = ?, updated_at = ? WHERE id = ?",
        )
        .bind(input_rate_per_1k)
        .bind(output_rate_per_1k)
        .bind(now)
        .bind(id)
        .execute(&self.store.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let res = sqlx::query("DELETE FROM models WHERE id = ?")
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}

fn map_model(r: &sqlx::sqlite::SqliteRow) -> LogicalModel {
    LogicalModel {
        id: r.get("id"),
        logical_name: r.get("logical_name"),
        description: r.get("description"),
        input_rate_per_1k: r.get("input_rate_per_1k"),
        output_rate_per_1k: r.get("output_rate_per_1k"),
        status: r.get("status"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

#[derive(Clone)]
pub struct ModelProviderRepo {
    store: SqliteStore,
}

impl ModelProviderRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        logical_model_id: &str,
        provider_id: &str,
        upstream_model: &str,
        level: i32,
        weight: i32,
        strategy: &str,
        enabled: bool,
        now: i64,
    ) -> anyhow::Result<ModelProvider> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO model_providers (id, logical_model_id, provider_id, upstream_model, level, weight, strategy, enabled, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(logical_model_id)
        .bind(provider_id)
        .bind(upstream_model)
        .bind(level)
        .bind(weight)
        .bind(strategy)
        .bind(if enabled { 1 } else { 0 })
        .bind(now)
        .execute(&self.store.pool)
        .await?;
        Ok(ModelProvider {
            id,
            logical_model_id: logical_model_id.to_string(),
            provider_id: provider_id.to_string(),
            upstream_model: upstream_model.to_string(),
            level,
            weight,
            strategy: map_strategy(strategy),
            enabled,
            created_at: now,
        })
    }

    pub async fn list_by_logical_model(
        &self,
        logical_model_id: &str,
    ) -> anyhow::Result<Vec<ModelProvider>> {
        let rows = sqlx::query(
            "SELECT id, logical_model_id, provider_id, upstream_model, level, weight, strategy, enabled, created_at \
             FROM model_providers WHERE logical_model_id = ? AND enabled = 1 ORDER BY level, weight",
        )
        .bind(logical_model_id)
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows.iter().map(map_model_provider).collect())
    }

    pub async fn list_all(&self) -> anyhow::Result<Vec<ModelProvider>> {
        let rows = sqlx::query(
            "SELECT id, logical_model_id, provider_id, upstream_model, level, weight, strategy, enabled, created_at \
             FROM model_providers ORDER BY created_at",
        )
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows.iter().map(map_model_provider).collect())
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Option<ModelProvider>> {
        let row = sqlx::query(
            "SELECT id, logical_model_id, provider_id, upstream_model, level, weight, strategy, enabled, created_at \
             FROM model_providers WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_model_provider(&r)))
    }

    pub async fn update(
        &self,
        id: &str,
        level: i32,
        weight: i32,
        strategy: &str,
        enabled: bool,
        now: i64,
    ) -> anyhow::Result<bool> {
        let res = sqlx::query(
            "UPDATE model_providers SET level = ?, weight = ?, strategy = ?, enabled = ? WHERE id = ?",
        )
        .bind(level)
        .bind(weight)
        .bind(strategy)
        .bind(if enabled { 1 } else { 0 })
        .bind(id)
        .execute(&self.store.pool)
        .await?;
        // model_providers 表无 updated_at 列，now 参数保留以与其它 Repo 签名一致。
        let _ = now;
        Ok(res.rows_affected() > 0)
    }

    pub async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let res = sqlx::query("DELETE FROM model_providers WHERE id = ?")
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}

fn map_strategy(s: &str) -> RoutingStrategy {
    if s == "random" {
        RoutingStrategy::Random
    } else {
        RoutingStrategy::Sequential
    }
}

fn map_model_provider(r: &sqlx::sqlite::SqliteRow) -> ModelProvider {
    let strategy: String = r.get("strategy");
    let enabled: i64 = r.get("enabled");
    ModelProvider {
        id: r.get("id"),
        logical_model_id: r.get("logical_model_id"),
        provider_id: r.get("provider_id"),
        upstream_model: r.get("upstream_model"),
        level: r.get("level"),
        weight: r.get("weight"),
        strategy: map_strategy(&strategy),
        enabled: enabled != 0,
        created_at: r.get("created_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建依赖行：models + provider_credentials。
    async fn setup() -> (SqliteStore, String, String) {
        let store = crate::connect_in_memory().await.unwrap();
        let now: i64 = 1_700_000_000_000;
        let model_id = format!("mdl_{}", Uuid::new_v4());
        sqlx::query(
            "INSERT INTO models (id, logical_name, description, input_rate_per_1k, output_rate_per_1k, status, created_at, updated_at) \
             VALUES (?, ?, NULL, 1, 2, 'active', ?, ?)",
        )
        .bind(&model_id)
        .bind(format!("name_{}", Uuid::new_v4()))
        .bind(now)
        .bind(now)
        .execute(&store.pool)
        .await
        .unwrap();
        let provider_id = format!("prov_{}", Uuid::new_v4());
        sqlx::query(
            "INSERT INTO provider_credentials (id, name, provider_type, base_url, api_key_enc, status, quota_used, quota_threshold, quota_alert_sent, created_at, updated_at) \
             VALUES (?, ?, 'openai', 'https://api.openai.com', 'enc', 'active', 0, 80, 0, ?, ?)",
        )
        .bind(&provider_id)
        .bind(format!("pn_{}", Uuid::new_v4()))
        .bind(now)
        .bind(now)
        .execute(&store.pool)
        .await
        .unwrap();
        (store, model_id, provider_id)
    }

    #[tokio::test]
    async fn model_crud() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = ModelRepo::new(store);
        let m = repo
            .create("mdl_1", "basic", Some("基础模型"), 1, 2, 100)
            .await
            .unwrap();
        assert_eq!(m.id, "mdl_1");
        assert_eq!(m.logical_name, "basic");
        assert_eq!(m.input_rate_per_1k, 1);
        assert_eq!(m.output_rate_per_1k, 2);
        assert_eq!(m.status, "active");

        let got = repo.get("mdl_1").await.unwrap().unwrap();
        assert_eq!(got.description.as_deref(), Some("基础模型"));

        let by_name = repo.get_by_name("basic").await.unwrap().unwrap();
        assert_eq!(by_name.id, "mdl_1");

        assert!(repo.list().await.unwrap().len() >= 1);

        assert!(repo
            .update("mdl_1", "basic2", None, 3, 4, "disabled", 200)
            .await
            .unwrap());
        let after = repo.get("mdl_1").await.unwrap().unwrap();
        assert_eq!(after.logical_name, "basic2");
        assert_eq!(after.description, None);
        assert_eq!(after.input_rate_per_1k, 3);
        assert_eq!(after.output_rate_per_1k, 4);
        assert_eq!(after.status, "disabled");
        assert_eq!(after.updated_at, 200);

        assert!(repo.set_rates("mdl_1", 5, 6, 300).await.unwrap());
        let after2 = repo.get("mdl_1").await.unwrap().unwrap();
        assert_eq!(after2.input_rate_per_1k, 5);
        assert_eq!(after2.output_rate_per_1k, 6);

        assert!(repo.delete("mdl_1").await.unwrap());
        assert!(repo.get("mdl_1").await.unwrap().is_none());
        // 二次删除应返回 false
        assert!(!repo.delete("mdl_1").await.unwrap());
    }

    #[tokio::test]
    async fn model_provider_crud() {
        let (store, model_id, provider_id) = setup().await;
        let repo = ModelProviderRepo::new(store);

        let mp = repo
            .create(
                &model_id,
                &provider_id,
                "gpt-4o",
                1,
                100,
                "sequential",
                true,
                1,
            )
            .await
            .unwrap();
        assert_eq!(mp.logical_model_id, model_id);
        assert_eq!(mp.provider_id, provider_id);
        assert_eq!(mp.upstream_model, "gpt-4o");
        assert_eq!(mp.level, 1);
        assert_eq!(mp.weight, 100);
        assert_eq!(mp.strategy, RoutingStrategy::Sequential);
        assert!(mp.enabled);

        let got = repo.get(&mp.id).await.unwrap().unwrap();
        assert_eq!(got.upstream_model, "gpt-4o");

        // 启用映射应出现在 list_by_logical_model
        let active = repo.list_by_logical_model(&model_id).await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].strategy, RoutingStrategy::Sequential);

        // 禁用
        assert!(repo
            .update(&mp.id, 2, 50, "random", false, 2)
            .await
            .unwrap());
        let disabled = repo.list_by_logical_model(&model_id).await.unwrap();
        assert!(disabled.is_empty(), "disabled mapping should not be listed");

        let got2 = repo.get(&mp.id).await.unwrap().unwrap();
        assert_eq!(got2.level, 2);
        assert_eq!(got2.weight, 50);
        assert_eq!(got2.strategy, RoutingStrategy::Random);
        assert!(!got2.enabled);

        // list_all 包含禁用项
        assert!(repo.list_all().await.unwrap().len() >= 1);

        assert!(repo.delete(&mp.id).await.unwrap());
        assert!(repo.get(&mp.id).await.unwrap().is_none());
    }
}
