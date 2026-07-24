//! 操作审计日志 Repo。
//!
//! 记录关键操作（汇率改/凭证轮换/token 吊销等），供审计追踪。

use crate::SqliteStore;
use sqlx::Row;
use uuid::Uuid;

/// 一条审计日志记录。
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditLogRecord {
    pub id: String,
    pub actor_kind: String, // admin/service/system
    pub actor_id: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub detail: Option<String>, // JSON
    pub source_ip: Option<String>,
    pub created_at: i64,
}

#[derive(Clone)]
pub struct AuditLogRepo {
    store: SqliteStore,
}

impl AuditLogRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    /// 单条写入。
    pub async fn insert(
        &self,
        actor_kind: &str,
        actor_id: Option<&str>,
        action: &str,
        target_type: Option<&str>,
        target_id: Option<&str>,
        detail: Option<&str>,
        source_ip: Option<&str>,
        now: i64,
    ) -> anyhow::Result<String> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO audit_logs (id, actor_kind, actor_id, action, target_type, target_id, detail, source_ip, created_at) \
             VALUES (?,?,?,?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(actor_kind)
        .bind(actor_id)
        .bind(action)
        .bind(target_type)
        .bind(target_id)
        .bind(detail)
        .bind(source_ip)
        .bind(now)
        .execute(&self.store.pool)
        .await?;
        Ok(id)
    }

    /// 批量写入（异步审计用）。
    pub async fn batch_insert(&self, records: &[AuditLogRecord]) -> anyhow::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let mut tx = self.store.pool.begin().await?;
        for r in records {
            sqlx::query(
                "INSERT INTO audit_logs (id, actor_kind, actor_id, action, target_type, target_id, detail, source_ip, created_at) \
                 VALUES (?,?,?,?,?,?,?,?,?)",
            )
            .bind(&r.id)
            .bind(&r.actor_kind)
            .bind(&r.actor_id)
            .bind(&r.action)
            .bind(&r.target_type)
            .bind(&r.target_id)
            .bind(&r.detail)
            .bind(&r.source_ip)
            .bind(r.created_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// 查询（按时间倒序，可过滤 actor_kind / action）。
    pub async fn list(
        &self,
        actor_kind: Option<&str>,
        action: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<AuditLogRecord>> {
        let rows = match (actor_kind, action) {
            (Some(k), Some(a)) => sqlx::query(
                "SELECT id, actor_kind, actor_id, action, target_type, target_id, detail, source_ip, created_at \
                 FROM audit_logs WHERE actor_kind = ? AND action = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(k).bind(a).bind(limit).bind(offset)
            .fetch_all(&self.store.pool).await?,
            (Some(k), None) => sqlx::query(
                "SELECT id, actor_kind, actor_id, action, target_type, target_id, detail, source_ip, created_at \
                 FROM audit_logs WHERE actor_kind = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(k).bind(limit).bind(offset)
            .fetch_all(&self.store.pool).await?,
            (None, Some(a)) => sqlx::query(
                "SELECT id, actor_kind, actor_id, action, target_type, target_id, detail, source_ip, created_at \
                 FROM audit_logs WHERE action = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(a).bind(limit).bind(offset)
            .fetch_all(&self.store.pool).await?,
            (None, None) => sqlx::query(
                "SELECT id, actor_kind, actor_id, action, target_type, target_id, detail, source_ip, created_at \
                 FROM audit_logs ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(limit).bind(offset)
            .fetch_all(&self.store.pool).await?,
        };
        Ok(rows.iter().map(map_audit).collect())
    }
}

fn map_audit(r: &sqlx::sqlite::SqliteRow) -> AuditLogRecord {
    AuditLogRecord {
        id: r.get("id"),
        actor_kind: r.get("actor_kind"),
        actor_id: r.get("actor_id"),
        action: r.get("action"),
        target_type: r.get("target_type"),
        target_id: r.get("target_id"),
        detail: r.get("detail"),
        source_ip: r.get("source_ip"),
        created_at: r.get("created_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn audit_insert_and_list() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = AuditLogRepo::new(store);
        repo.insert(
            "admin",
            Some("adm_1"),
            "rate.update",
            Some("model"),
            Some("mdl_expert"),
            Some(r#"{"from":10,"to":20}"#),
            Some("10.0.0.1"),
            1,
        )
        .await
        .unwrap();
        repo.insert("system", None, "provider.disable", Some("provider"), Some("prov_1"), None, None, 2).await.unwrap();
        let all = repo.list(None, None, 10, 0).await.unwrap();
        assert_eq!(all.len(), 2);
        let by_action = repo.list(None, Some("rate.update"), 10, 0).await.unwrap();
        assert_eq!(by_action.len(), 1);
        assert_eq!(by_action[0].actor_id.as_deref(), Some("adm_1"));
        let by_kind = repo.list(Some("system"), None, 10, 0).await.unwrap();
        assert_eq!(by_kind.len(), 1);
    }

    #[tokio::test]
    async fn batch_insert_works() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = AuditLogRepo::new(store);
        let recs: Vec<_> = (0..30)
            .map(|i| AuditLogRecord {
                id: Uuid::new_v4().to_string(),
                actor_kind: "admin".to_string(),
                actor_id: Some("adm_1".to_string()),
                action: format!("op_{i}"),
                target_type: None,
                target_id: None,
                detail: None,
                source_ip: None,
                created_at: i,
            })
            .collect();
        repo.batch_insert(&recs).await.unwrap();
        let all = repo.list(None, None, 100, 0).await.unwrap();
        assert_eq!(all.len(), 30);
    }
}
