//! Service 账号 Repo（机器对机器认证）。

use crate::SqliteStore;
use serde_json;
use sqlx::Row;

#[derive(Clone)]
pub struct ServiceAccountRepo {
    store: SqliteStore,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ServiceAccountRecord {
    pub id: String,
    pub client_id: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub ip_whitelist: Option<Vec<String>>,
    pub public_key: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl ServiceAccountRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        id: &str,
        client_id: &str,
        client_secret_hash: &str,
        name: &str,
        scopes: &[&str],
        ip_whitelist: Option<&[&str]>,
        now: i64,
    ) -> anyhow::Result<ServiceAccountRecord> {
        let scopes_json = serde_json::to_string(scopes)?;
        let ip_json = match ip_whitelist {
            Some(ips) => Some(serde_json::to_string(ips)?),
            None => None,
        };
        sqlx::query(
            "INSERT INTO service_accounts \
             (id, client_id, client_secret_hash, name, scopes, ip_whitelist, public_key, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, NULL, 'active', ?, ?)",
        )
        .bind(id)
        .bind(client_id)
        .bind(client_secret_hash)
        .bind(name)
        .bind(&scopes_json)
        .bind(&ip_json)
        .bind(now)
        .bind(now)
        .execute(&self.store.pool)
        .await?;

        Ok(ServiceAccountRecord {
            id: id.to_string(),
            client_id: client_id.to_string(),
            name: name.to_string(),
            scopes: scopes.iter().map(|s| (*s).to_string()).collect(),
            ip_whitelist: ip_whitelist.map(|ips| ips.iter().map(|s| (*s).to_string()).collect()),
            public_key: None,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    /// 认证查询：返回 (id, secret_hash, scopes, status)。用于校验 client 凭据。
    pub async fn find_auth(
        &self,
        client_id: &str,
    ) -> anyhow::Result<Option<(String, String, Vec<String>, String)>> {
        let row = sqlx::query(
            "SELECT id, client_secret_hash, scopes, status FROM service_accounts WHERE client_id = ?",
        )
        .bind(client_id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| {
            let scopes: String = r.get("scopes");
            let scopes: Vec<String> =
                serde_json::from_str(&scopes).unwrap_or_default();
            (
                r.get("id"),
                r.get("client_secret_hash"),
                scopes,
                r.get("status"),
            )
        }))
    }

    pub async fn find_by_client_id(
        &self,
        client_id: &str,
    ) -> anyhow::Result<Option<ServiceAccountRecord>> {
        let row = sqlx::query(
            "SELECT id, client_id, name, scopes, ip_whitelist, public_key, status, created_at, updated_at \
             FROM service_accounts WHERE client_id = ?",
        )
        .bind(client_id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_service_account(&r)))
    }

    pub async fn find_by_id(&self, id: &str) -> anyhow::Result<Option<ServiceAccountRecord>> {
        let row = sqlx::query(
            "SELECT id, client_id, name, scopes, ip_whitelist, public_key, status, created_at, updated_at \
             FROM service_accounts WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| map_service_account(&r)))
    }

    pub async fn list(&self, limit: i64, offset: i64) -> anyhow::Result<Vec<ServiceAccountRecord>> {
        let rows = sqlx::query(
            "SELECT id, client_id, name, scopes, ip_whitelist, public_key, status, created_at, updated_at \
             FROM service_accounts ORDER BY created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows.iter().map(map_service_account).collect())
    }

    pub async fn update_scopes_ips(
        &self,
        id: &str,
        scopes: &[&str],
        ip_whitelist: Option<&[&str]>,
        now: i64,
    ) -> anyhow::Result<bool> {
        let scopes_json = serde_json::to_string(scopes)?;
        let ip_json = match ip_whitelist {
            Some(ips) => Some(serde_json::to_string(ips)?),
            None => None,
        };
        let res = sqlx::query(
            "UPDATE service_accounts SET scopes = ?, ip_whitelist = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&scopes_json)
        .bind(&ip_json)
        .bind(now)
        .bind(id)
        .execute(&self.store.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn reset_secret(
        &self,
        id: &str,
        new_secret_hash: &str,
        now: i64,
    ) -> anyhow::Result<bool> {
        let res =
            sqlx::query("UPDATE service_accounts SET client_secret_hash = ?, updated_at = ? WHERE id = ?")
                .bind(new_secret_hash)
                .bind(now)
                .bind(id)
                .execute(&self.store.pool)
                .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn set_status(&self, id: &str, status: &str, now: i64) -> anyhow::Result<bool> {
        let res = sqlx::query("UPDATE service_accounts SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(now)
            .bind(id)
            .execute(&self.store.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}

fn map_service_account(r: &sqlx::sqlite::SqliteRow) -> ServiceAccountRecord {
    let scopes: String = r.get("scopes");
    let scopes: Vec<String> = serde_json::from_str(&scopes).unwrap_or_default();
    let ip_whitelist: Option<String> = r.get("ip_whitelist");
    let ip_whitelist = ip_whitelist.and_then(|s| serde_json::from_str(&s).ok());
    ServiceAccountRecord {
        id: r.get("id"),
        client_id: r.get("client_id"),
        name: r.get("name"),
        scopes,
        ip_whitelist,
        public_key: r.get("public_key"),
        status: r.get("status"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_find() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = ServiceAccountRepo::new(store);
        let id = format!("sa_{}", uuid::Uuid::new_v4());
        let rec = repo
            .create(
                &id,
                "client_1",
                "hash_v1",
                "Test Bot",
                &["read", "write"],
                Some(&["10.0.0.0/8"]),
                1,
            )
            .await
            .unwrap();
        assert_eq!(rec.client_id, "client_1");
        assert_eq!(rec.scopes, vec!["read".to_string(), "write".to_string()]);
        assert_eq!(
            rec.ip_whitelist.as_deref(),
            Some(&["10.0.0.0/8".to_string()][..])
        );

        let got = repo.find_by_client_id("client_1").await.unwrap().unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.name, "Test Bot");

        let by_id = repo.find_by_id(&id).await.unwrap().unwrap();
        assert_eq!(by_id.client_id, "client_1");
    }

    #[tokio::test]
    async fn find_auth_returns_credentials() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = ServiceAccountRepo::new(store);
        let id = format!("sa_{}", uuid::Uuid::new_v4());
        repo.create(&id, "client_auth", "secret_hash_1", "Bot", &["read"], None, 1)
            .await
            .unwrap();
        let auth = repo.find_auth("client_auth").await.unwrap().unwrap();
        assert_eq!(auth.0, id);
        assert_eq!(auth.1, "secret_hash_1");
        assert_eq!(auth.2, vec!["read".to_string()]);
        assert_eq!(auth.3, "active");
    }

    #[tokio::test]
    async fn reset_secret_changes_hash() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = ServiceAccountRepo::new(store);
        let id = format!("sa_{}", uuid::Uuid::new_v4());
        repo.create(&id, "client_reset", "hash_old", "Bot", &["read"], None, 1)
            .await
            .unwrap();
        assert!(repo.reset_secret(&id, "hash_new", 2).await.unwrap());
        let auth = repo.find_auth("client_reset").await.unwrap().unwrap();
        assert_eq!(auth.1, "hash_new");
        let rec = repo.find_by_id(&id).await.unwrap().unwrap();
        assert_eq!(rec.updated_at, 2);
    }

    #[tokio::test]
    async fn update_scopes_ips_and_status() {
        let store = crate::connect_in_memory().await.unwrap();
        let repo = ServiceAccountRepo::new(store);
        let id = format!("sa_{}", uuid::Uuid::new_v4());
        repo.create(&id, "client_upd", "h", "Bot", &["read"], None, 1)
            .await
            .unwrap();
        assert!(
            repo.update_scopes_ips(&id, &["read", "admin"], Some(&["1.2.3.4"]), 2)
                .await
                .unwrap()
        );
        let rec = repo.find_by_id(&id).await.unwrap().unwrap();
        assert_eq!(rec.scopes, vec!["read".to_string(), "admin".to_string()]);
        assert_eq!(rec.ip_whitelist, Some(vec!["1.2.3.4".to_string()]));

        assert!(repo.set_status(&id, "disabled", 3).await.unwrap());
        let rec = repo.find_by_id(&id).await.unwrap().unwrap();
        assert_eq!(rec.status, "disabled");
    }
}
