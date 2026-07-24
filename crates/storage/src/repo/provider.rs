//! 供应商凭证、额度与映射 Repo。

use crate::SqliteStore;
use domain::{ModelProvider, ProviderCredential, ProviderStatus, RoutingStrategy};
use sqlx::Row;
use uuid::Uuid;

#[derive(Clone)]
pub struct ProviderRepo {
    store: SqliteStore,
}

impl ProviderRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        id: &str,
        name: &str,
        provider_type: &str,
        base_url: &str,
        api_key_enc: &str,
        quota_limit: Option<i64>,
        quota_threshold: i32,
        now: i64,
    ) -> anyhow::Result<ProviderCredential> {
        sqlx::query(
            "INSERT INTO provider_credentials \
             (id, name, provider_type, base_url, api_key_enc, status, disabled_reason, disabled_at, \
              quota_limit, quota_used, quota_threshold, quota_alert_sent, quota_synced_at, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 'active', NULL, NULL, ?, 0, ?, 0, NULL, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(provider_type)
        .bind(base_url)
        .bind(api_key_enc)
        .bind(quota_limit)
        .bind(quota_threshold)
        .bind(now)
        .bind(now)
        .execute(&self.store.pool)
        .await?;
        Ok(ProviderCredential {
            id: id.to_string(),
            name: name.to_string(),
            provider_type: provider_type.to_string(),
            base_url: base_url.to_string(),
            api_key_enc: api_key_enc.to_string(),
            status: ProviderStatus::Active,
            disabled_reason: None,
            disabled_at: None,
            quota_limit,
            quota_used: 0,
            quota_threshold,
            quota_alert_sent: false,
            quota_synced_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Option<ProviderCredential>> {
        let row = sqlx::query(
            "SELECT id, name, provider_type, base_url, api_key_enc, status, disabled_reason, disabled_at, \
                    quota_limit, quota_used, quota_threshold, quota_alert_sent, quota_synced_at, created_at, updated_at \
             FROM provider_credentials WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_provider(&r)))
    }

    pub async fn list(&self) -> anyhow::Result<Vec<ProviderCredential>> {
        let rows = sqlx::query(
            "SELECT id, name, provider_type, base_url, api_key_enc, status, disabled_reason, disabled_at, \
                    quota_limit, quota_used, quota_threshold, quota_alert_sent, quota_synced_at, created_at, updated_at \
             FROM provider_credentials ORDER BY created_at",
        )
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows.iter().map(map_provider).collect())
    }

    /// api_key 为 None 时不更新密钥。
    pub async fn update(
        &self,
        id: &str,
        name: &str,
        base_url: &str,
        api_key_enc: Option<&str>,
        quota_limit: Option<i64>,
        quota_threshold: i32,
        now: i64,
    ) -> anyhow::Result<bool> {
        let res = if let Some(key) = api_key_enc {
            sqlx::query(
                "UPDATE provider_credentials SET name = ?, base_url = ?, api_key_enc = ?, quota_limit = ?, quota_threshold = ?, updated_at = ? \
                 WHERE id = ?",
            )
            .bind(name)
            .bind(base_url)
            .bind(key)
            .bind(quota_limit)
            .bind(quota_threshold)
            .bind(now)
            .bind(id)
            .execute(&self.store.pool)
            .await?
        } else {
            sqlx::query(
                "UPDATE provider_credentials SET name = ?, base_url = ?, quota_limit = ?, quota_threshold = ?, updated_at = ? \
                 WHERE id = ?",
            )
            .bind(name)
            .bind(base_url)
            .bind(quota_limit)
            .bind(quota_threshold)
            .bind(now)
            .bind(id)
            .execute(&self.store.pool)
            .await?
        };
        Ok(res.rows_affected() > 0)
    }

    pub async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let res = sqlx::query("DELETE FROM provider_credentials WHERE id = ?")
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// status="active" 时清空禁用原因/时间；否则记录禁用信息。
    pub async fn set_status(
        &self,
        id: &str,
        status: &str,
        reason: Option<&str>,
        now: i64,
    ) -> anyhow::Result<bool> {
        let res = if status == "active" {
            sqlx::query(
                "UPDATE provider_credentials SET status = 'active', disabled_reason = NULL, disabled_at = NULL, updated_at = ? \
                 WHERE id = ?",
            )
            .bind(now)
            .bind(id)
            .execute(&self.store.pool)
            .await?
        } else {
            sqlx::query(
                "UPDATE provider_credentials SET status = 'disabled', disabled_reason = ?, disabled_at = ?, updated_at = ? \
                 WHERE id = ?",
            )
            .bind(reason)
            .bind(now)
            .bind(now)
            .bind(id)
            .execute(&self.store.pool)
            .await?
        };
        Ok(res.rows_affected() > 0)
    }
}

fn map_provider(r: &sqlx::sqlite::SqliteRow) -> ProviderCredential {
    let status: String = r.get("status");
    let alert_sent: i64 = r.get("quota_alert_sent");
    ProviderCredential {
        id: r.get("id"),
        name: r.get("name"),
        provider_type: r.get("provider_type"),
        base_url: r.get("base_url"),
        api_key_enc: r.get("api_key_enc"),
        status: if status == "active" {
            ProviderStatus::Active
        } else {
            ProviderStatus::Disabled
        },
        disabled_reason: r.get("disabled_reason"),
        disabled_at: r.get("disabled_at"),
        quota_limit: r.get("quota_limit"),
        quota_used: r.get("quota_used"),
        quota_threshold: r.get("quota_threshold"),
        quota_alert_sent: alert_sent != 0,
        quota_synced_at: r.get("quota_synced_at"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

#[derive(Clone)]
pub struct ProviderQuotaRepo {
    store: SqliteStore,
}

impl ProviderQuotaRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    /// 累加用量并返回最新额度状态：(used, limit, threshold, alert_sent)。
    pub async fn add_usage(
        &self,
        provider_id: &str,
        tokens: i64,
        now: i64,
    ) -> anyhow::Result<(i64, Option<i64>, i32, bool)> {
        let row = sqlx::query(
            "UPDATE provider_credentials SET quota_used = quota_used + ?, updated_at = ? \
             WHERE id = ? \
             RETURNING quota_used, quota_limit, quota_threshold, quota_alert_sent",
        )
        .bind(tokens)
        .bind(now)
        .bind(provider_id)
        .fetch_one(&self.store.pool)
        .await?;
        let used: i64 = row.get("quota_used");
        let limit: Option<i64> = row.get("quota_limit");
        let threshold: i32 = row.get("quota_threshold");
        let alert_sent: i64 = row.get("quota_alert_sent");
        Ok((used, limit, threshold, alert_sent != 0))
    }

    pub async fn mark_alert_sent(&self, provider_id: &str, now: i64) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE provider_credentials SET quota_alert_sent = 1, updated_at = ? WHERE id = ?",
        )
        .bind(now)
        .bind(provider_id)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }

    /// 用官方数据校准已用量，并重置告警标志（允许新用量后再次告警）。
    pub async fn sync_quota(
        &self,
        provider_id: &str,
        official_used: i64,
        now: i64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE provider_credentials SET quota_used = ?, quota_synced_at = ?, quota_alert_sent = 0, updated_at = ? \
             WHERE id = ?",
        )
        .bind(official_used)
        .bind(now)
        .bind(now)
        .bind(provider_id)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }

    pub async fn get_quota(
        &self,
        provider_id: &str,
    ) -> anyhow::Result<Option<(i64, Option<i64>, i32, bool)>> {
        let row = sqlx::query(
            "SELECT quota_used, quota_limit, quota_threshold, quota_alert_sent \
             FROM provider_credentials WHERE id = ?",
        )
        .bind(provider_id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| {
            let alert_sent: i64 = r.get("quota_alert_sent");
            (
                r.get("quota_used"),
                r.get("quota_limit"),
                r.get("quota_threshold"),
                alert_sent != 0,
            )
        }))
    }
}

#[derive(Clone)]
pub struct ModelProviderMappingRepo {
    store: SqliteStore,
}

impl ModelProviderMappingRepo {
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
        Ok(rows.iter().map(map_mapping).collect())
    }

    pub async fn list_all(&self) -> anyhow::Result<Vec<ModelProvider>> {
        let rows = sqlx::query(
            "SELECT id, logical_model_id, provider_id, upstream_model, level, weight, strategy, enabled, created_at \
             FROM model_providers ORDER BY created_at",
        )
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows.iter().map(map_mapping).collect())
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Option<ModelProvider>> {
        let row = sqlx::query(
            "SELECT id, logical_model_id, provider_id, upstream_model, level, weight, strategy, enabled, created_at \
             FROM model_providers WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_mapping(&r)))
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

fn map_mapping(r: &sqlx::sqlite::SqliteRow) -> ModelProvider {
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
    async fn provider_crud() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = ProviderRepo::new(store);
        let p = repo
            .create(
                "prov_1",
                "openai-primary",
                "openai",
                "https://api.openai.com",
                "enc-secret",
                Some(1_000_000),
                80,
                100,
            )
            .await
            .unwrap();
        assert_eq!(p.id, "prov_1");
        assert_eq!(p.provider_type, "openai");
        assert_eq!(p.status, ProviderStatus::Active);
        assert_eq!(p.quota_limit, Some(1_000_000));
        assert_eq!(p.quota_threshold, 80);
        assert!(!p.quota_alert_sent);
        assert!(p.disabled_at.is_none());

        let got = repo.get("prov_1").await.unwrap().unwrap();
        assert_eq!(got.api_key_enc, "enc-secret");
        assert!(repo.list().await.unwrap().len() >= 1);

        // 不更新密钥（None）
        assert!(
            repo.update("prov_1", "renamed", "https://v2", None, Some(2_000_000), 90, 200)
                .await
                .unwrap()
        );
        let after = repo.get("prov_1").await.unwrap().unwrap();
        assert_eq!(after.name, "renamed");
        assert_eq!(after.base_url, "https://v2");
        assert_eq!(after.api_key_enc, "enc-secret", "key unchanged");
        assert_eq!(after.quota_limit, Some(2_000_000));
        assert_eq!(after.quota_threshold, 90);

        // 更新密钥
        assert!(
            repo.update("prov_1", "renamed", "https://v2", Some("new-enc"), None, 90, 300)
                .await
                .unwrap()
        );
        let after2 = repo.get("prov_1").await.unwrap().unwrap();
        assert_eq!(after2.api_key_enc, "new-enc");

        // 禁用
        assert!(
            repo.set_status("prov_1", "disabled", Some("quota_exhausted"), 400)
                .await
                .unwrap()
        );
        let dis = repo.get("prov_1").await.unwrap().unwrap();
        assert_eq!(dis.status, ProviderStatus::Disabled);
        assert_eq!(dis.disabled_reason.as_deref(), Some("quota_exhausted"));
        assert_eq!(dis.disabled_at, Some(400));

        // 重新启用应清空禁用信息
        assert!(repo.set_status("prov_1", "active", None, 500).await.unwrap());
        let en = repo.get("prov_1").await.unwrap().unwrap();
        assert_eq!(en.status, ProviderStatus::Active);
        assert!(en.disabled_reason.is_none());
        assert!(en.disabled_at.is_none());

        assert!(repo.delete("prov_1").await.unwrap());
        assert!(repo.get("prov_1").await.unwrap().is_none());
        assert!(!repo.delete("prov_1").await.unwrap());
    }

    #[tokio::test]
    async fn quota_add_and_alert() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = ProviderRepo::new(store.clone());
        let q = ProviderQuotaRepo::new(store);
        repo.create("prov_q", "p", "openai", "u", "e", Some(1000), 80, 1)
            .await
            .unwrap();

        let (used, limit, threshold, alert) = q.add_usage("prov_q", 300, 2).await.unwrap();
        assert_eq!(used, 300);
        assert_eq!(limit, Some(1000));
        assert_eq!(threshold, 80);
        assert!(!alert);

        // 再次累加
        let (used2, _, _, _) = q.add_usage("prov_q", 250, 3).await.unwrap();
        assert_eq!(used2, 550);

        // get_quota 读取
        let g = q.get_quota("prov_q").await.unwrap().unwrap();
        assert_eq!(g.0, 550);
        assert_eq!(g.1, Some(1000));
        assert_eq!(g.2, 80);
        assert!(!g.3);

        // 标记告警
        q.mark_alert_sent("prov_q", 4).await.unwrap();
        let (_, _, _, alerted) = q.add_usage("prov_q", 0, 5).await.unwrap();
        assert!(alerted);

        // 同步校准后告警标志复位
        q.sync_quota("prov_q", 100, 6).await.unwrap();
        let g2 = q.get_quota("prov_q").await.unwrap().unwrap();
        assert_eq!(g2.0, 100);
        assert!(!g2.3, "alert flag reset after sync");
    }

    #[tokio::test]
    async fn mapping_crud() {
        let (store, model_id, provider_id) = setup().await;
        let repo = ModelProviderMappingRepo::new(store);

        let m = repo
            .create(&model_id, &provider_id, "claude-3-5-sonnet", 1, 80, "random", true, 1)
            .await
            .unwrap();
        assert_eq!(m.strategy, RoutingStrategy::Random);
        assert!(m.enabled);

        let got = repo.get(&m.id).await.unwrap().unwrap();
        assert_eq!(got.upstream_model, "claude-3-5-sonnet");
        assert_eq!(got.strategy, RoutingStrategy::Random);

        // 启用映射进入 list_by_logical_model
        assert_eq!(
            repo.list_by_logical_model(&model_id).await.unwrap().len(),
            1
        );

        // 禁用
        assert!(repo.update(&m.id, 2, 90, "sequential", false, 2).await.unwrap());
        assert!(repo.list_by_logical_model(&model_id).await.unwrap().is_empty());

        let got2 = repo.get(&m.id).await.unwrap().unwrap();
        assert_eq!(got2.level, 2);
        assert_eq!(got2.weight, 90);
        assert_eq!(got2.strategy, RoutingStrategy::Sequential);
        assert!(!got2.enabled);

        assert!(repo.list_all().await.unwrap().len() >= 1);
        assert!(repo.delete(&m.id).await.unwrap());
        assert!(repo.get(&m.id).await.unwrap().is_none());
    }
}
