//! 调用日志（事实表）+ 预聚合（用户维度 / 供应商维度）。
//!
//! usage_logs 带 account_id + provider_id 双标签，支持多维度聚合。
//! 预聚合表 UPSERT 累加，避免扫描日志。批量 INSERT 提升吞吐。

use crate::SqliteStore;
use domain::{UsageLog, UsageSource};
use sqlx::Row;

/// 一条待写入的用量日志（批量插入用，避免每次构造完整 UsageLog）。
#[derive(Clone, serde::Serialize)]
pub struct UsageLogEntry {
    pub id: String,
    pub account_id: String,
    pub logical_model: String,
    pub provider_id: Option<String>,
    pub upstream_model: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub credits_cost: i64,
    pub usage_source: UsageSource,
    pub status: String,
    pub source_ip: Option<String>,
    pub created_at: i64,
}

#[derive(Clone)]
pub struct UsageLogRepo {
    store: SqliteStore,
}

impl UsageLogRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    /// 单条写入 + 预聚合更新（同一事务）。用于结算时同步落账。
    pub async fn insert_and_aggregate(&self, e: UsageLogEntry, period: &str) -> anyhow::Result<()> {
        let mut tx = self.store.pool.begin().await?;
        sqlx::query(
            "INSERT INTO usage_logs \
             (id, account_id, logical_model, provider_id, upstream_model, prompt_tokens, completion_tokens, total_tokens, credits_cost, usage_source, status, source_ip, created_at) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&e.id)
        .bind(&e.account_id)
        .bind(&e.logical_model)
        .bind(&e.provider_id)
        .bind(&e.upstream_model)
        .bind(e.prompt_tokens)
        .bind(e.completion_tokens)
        .bind(e.total_tokens)
        .bind(e.credits_cost)
        .bind(e.usage_source.as_str())
        .bind(&e.status)
        .bind(&e.source_ip)
        .bind(e.created_at)
        .execute(&mut *tx)
        .await?;

        // 用户维度预聚合
        let p = e.prompt_tokens.unwrap_or(0);
        let c = e.completion_tokens.unwrap_or(0);
        sqlx::query(
            "INSERT INTO account_usage_summary (account_id, period, prompt_tokens, completion_tokens, credits, calls) \
             VALUES (?, ?, ?, ?, ?, 1) \
             ON CONFLICT (account_id, period) DO UPDATE SET \
               prompt_tokens = prompt_tokens + excluded.prompt_tokens, \
               completion_tokens = completion_tokens + excluded.completion_tokens, \
               credits = credits + excluded.credits, \
               calls = calls + 1",
        )
        .bind(&e.account_id)
        .bind(period)
        .bind(p)
        .bind(c)
        .bind(e.credits_cost)
        .execute(&mut *tx)
        .await?;

        // 供应商维度预聚合（仅在 provider_id 存在时）
        if let Some(pid) = &e.provider_id {
            let total = e.total_tokens.unwrap_or(p + c);
            sqlx::query(
                "INSERT INTO provider_usage_summary (provider_id, period, tokens_used) \
                 VALUES (?, ?, ?) \
                 ON CONFLICT (provider_id, period) DO UPDATE SET \
                   tokens_used = tokens_used + excluded.tokens_used",
            )
            .bind(pid)
            .bind(period)
            .bind(total)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// 批量写入（异步审计用，不带预聚合；预聚合由结算路径负责）。
    pub async fn batch_insert(&self, entries: &[UsageLogEntry]) -> anyhow::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut tx = self.store.pool.begin().await?;
        for e in entries {
            sqlx::query(
                "INSERT INTO usage_logs \
                 (id, account_id, logical_model, provider_id, upstream_model, prompt_tokens, completion_tokens, total_tokens, credits_cost, usage_source, status, source_ip, created_at) \
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
            )
            .bind(&e.id)
            .bind(&e.account_id)
            .bind(&e.logical_model)
            .bind(&e.provider_id)
            .bind(&e.upstream_model)
            .bind(e.prompt_tokens)
            .bind(e.completion_tokens)
            .bind(e.total_tokens)
            .bind(e.credits_cost)
            .bind(e.usage_source.as_str())
            .bind(&e.status)
            .bind(&e.source_ip)
            .bind(e.created_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// 查询用户某时段汇总（从预聚合表读，O(1)）。
    pub async fn account_summary(
        &self,
        account_id: &str,
        period: &str,
    ) -> anyhow::Result<(i64, i64, i64, i64)> {
        // (prompt_tokens, completion_tokens, credits, calls)
        let row = sqlx::query(
            "SELECT prompt_tokens, completion_tokens, credits, calls FROM account_usage_summary \
             WHERE account_id = ? AND period = ?",
        )
        .bind(account_id)
        .bind(period)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| (r.get(0), r.get(1), r.get(2), r.get(3))).unwrap_or((0, 0, 0, 0)))
    }

    /// 供应商某时段总 token（成本侧）。
    pub async fn provider_summary(&self, provider_id: &str, period: &str) -> anyhow::Result<i64> {
        let row = sqlx::query(
            "SELECT tokens_used FROM provider_usage_summary WHERE provider_id = ? AND period = ?",
        )
        .bind(provider_id)
        .bind(period)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| r.get::<i64, _>(0)).unwrap_or(0))
    }

    /// 明细查询（倒序分页）。
    pub async fn list(
        &self,
        account_id: Option<&str>,
        provider_id: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<UsageLog>> {
        let rows = match (account_id, provider_id) {
            (Some(a), Some(p)) => sqlx::query(
                "SELECT id, account_id, logical_model, provider_id, upstream_model, prompt_tokens, completion_tokens, total_tokens, credits_cost, usage_source, status, source_ip, created_at \
                 FROM usage_logs WHERE account_id = ? AND provider_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(a).bind(p).bind(limit).bind(offset)
            .fetch_all(&self.store.pool).await?,
            (Some(a), None) => sqlx::query(
                "SELECT id, account_id, logical_model, provider_id, upstream_model, prompt_tokens, completion_tokens, total_tokens, credits_cost, usage_source, status, source_ip, created_at \
                 FROM usage_logs WHERE account_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(a).bind(limit).bind(offset)
            .fetch_all(&self.store.pool).await?,
            (None, Some(p)) => sqlx::query(
                "SELECT id, account_id, logical_model, provider_id, upstream_model, prompt_tokens, completion_tokens, total_tokens, credits_cost, usage_source, status, source_ip, created_at \
                 FROM usage_logs WHERE provider_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(p).bind(limit).bind(offset)
            .fetch_all(&self.store.pool).await?,
            (None, None) => sqlx::query(
                "SELECT id, account_id, logical_model, provider_id, upstream_model, prompt_tokens, completion_tokens, total_tokens, credits_cost, usage_source, status, source_ip, created_at \
                 FROM usage_logs ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(limit).bind(offset)
            .fetch_all(&self.store.pool).await?,
        };
        Ok(rows.iter().map(map_usage_log).collect())
    }
}

fn map_usage_log(r: &sqlx::sqlite::SqliteRow) -> UsageLog {
    UsageLog {
        id: r.get("id"),
        account_id: r.get("account_id"),
        logical_model: r.get("logical_model"),
        provider_id: r.get("provider_id"),
        upstream_model: r.get("upstream_model"),
        prompt_tokens: r.get("prompt_tokens"),
        completion_tokens: r.get("completion_tokens"),
        total_tokens: r.get("total_tokens"),
        credits_cost: r.get("credits_cost"),
        usage_source: UsageSource::from_db(r.get::<String, _>("usage_source").as_str()),
        status: r.get("status"),
        source_ip: r.get("source_ip"),
        created_at: r.get("created_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn entry(account_id: &str, provider_id: Option<&str>, cost: i64, ts: i64) -> UsageLogEntry {
        UsageLogEntry {
            id: Uuid::new_v4().to_string(),
            account_id: account_id.to_string(),
            logical_model: "expert".to_string(),
            provider_id: provider_id.map(String::from),
            upstream_model: Some("gpt-4o".to_string()),
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            total_tokens: Some(150),
            credits_cost: cost,
            usage_source: UsageSource::Upstream,
            status: "success".to_string(),
            source_ip: None,
            created_at: ts,
        }
    }

    #[tokio::test]
    async fn insert_and_aggregate_updates_summary() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = UsageLogRepo::new(store);
        let now: i64 = 1_700_000_000_000;
        repo.insert_and_aggregate(entry("acct_a", Some("prov_1"), 10, now), "202607").await.unwrap();
        repo.insert_and_aggregate(entry("acct_a", Some("prov_1"), 20, now + 1), "202607").await.unwrap();
        let (p, c, credits, calls) = repo.account_summary("acct_a", "202607").await.unwrap();
        assert_eq!(p, 200); // 2 * 100
        assert_eq!(c, 100); // 2 * 50
        assert_eq!(credits, 30);
        assert_eq!(calls, 2);
        let prov_tokens = repo.provider_summary("prov_1", "202607").await.unwrap();
        assert_eq!(prov_tokens, 300); // 2 * 150
    }

    #[tokio::test]
    async fn batch_insert_works() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = UsageLogRepo::new(store);
        let entries: Vec<_> = (0..50)
            .map(|i| entry(&format!("acct_{i}"), None, i, 1))
            .collect();
        repo.batch_insert(&entries).await.unwrap();
        let logs = repo.list(None, None, 100, 0).await.unwrap();
        assert_eq!(logs.len(), 50);
    }
}
