//! 审计/日志批量写入 sink。
//!
//! channel + 后台 worker：每 100 条或 1 秒 flush 一次。

use storage::{AuditLogRecord, AuditLogRepo, UsageLogEntry, UsageLogRepo};
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

/// 提交给 worker 的命令。
pub enum AuditCommand {
    UsageLog(UsageLogEntry),
    AuditLog(AuditLogRecord),
}

/// 审计 sink 句柄（clone 可廉价共享）。
#[derive(Clone)]
pub struct AuditSink {
    tx: mpsc::Sender<AuditCommand>,
}

impl AuditSink {
    pub fn new(tx: mpsc::Sender<AuditCommand>) -> Self {
        Self { tx }
    }

    /// 提交一条 usage 日志（非阻塞；channel 满时等待背压）。
    pub async fn log_usage(&self, entry: UsageLogEntry) {
        if self.tx.send(AuditCommand::UsageLog(entry)).await.is_err() {
            tracing::warn!("audit channel closed, usage log dropped");
        }
    }

    /// 提交一条审计日志。
    pub async fn log_audit(&self, record: AuditLogRecord) {
        if self.tx.send(AuditCommand::AuditLog(record)).await.is_err() {
            tracing::warn!("audit channel closed, audit log dropped");
        }
    }
}

/// 启动后台 worker。返回 sink 句柄。
/// worker 每 `batch_size` 条或 `flush_interval` flush 一次。
/// channel 容量 `capacity`：满时背压（调用方 await）。
pub fn start_audit_worker(
    store: storage::SqliteStore,
    batch_size: usize,
    flush_interval: Duration,
    capacity: usize,
) -> AuditSink {
    let (tx, rx) = mpsc::channel(capacity);
    let sink = AuditSink::new(tx);
    tokio::spawn(run_worker(rx, store, batch_size, flush_interval));
    sink
}

async fn run_worker(
    mut rx: mpsc::Receiver<AuditCommand>,
    store: storage::SqliteStore,
    batch_size: usize,
    flush_interval: Duration,
) {
    let usage_repo = UsageLogRepo::new(store.clone());
    let audit_repo = AuditLogRepo::new(store.clone());

    let mut usage_batch: Vec<UsageLogEntry> = Vec::with_capacity(batch_size);
    let mut audit_batch: Vec<AuditLogRecord> = Vec::with_capacity(batch_size);
    let mut ticker = time::interval(flush_interval);

    loop {
        tokio::select! {
            biased; // 优先处理 channel 命令（含关闭检测），避免 tick 分支饿死 recv
            // 收到命令
            cmd = rx.recv() => {
                match cmd {
                    Some(AuditCommand::UsageLog(e)) => {
                        usage_batch.push(e);
                        if usage_batch.len() >= batch_size {
                            flush_usage(&usage_repo, &mut usage_batch).await;
                        }
                    }
                    Some(AuditCommand::AuditLog(r)) => {
                        audit_batch.push(r);
                        if audit_batch.len() >= batch_size {
                            flush_audit(&audit_repo, &mut audit_batch).await;
                        }
                    }
                    None => {
                        // channel 关闭，flush 剩余并退出
                        flush_usage(&usage_repo, &mut usage_batch).await;
                        flush_audit(&audit_repo, &mut audit_batch).await;
                        tracing::info!("audit worker drained and exiting");
                        return;
                    }
                }
            }
            // 定时 flush
            _ = ticker.tick() => {
                if !usage_batch.is_empty() {
                    flush_usage(&usage_repo, &mut usage_batch).await;
                }
                if !audit_batch.is_empty() {
                    flush_audit(&audit_repo, &mut audit_batch).await;
                }
            }
        }
    }
}

async fn flush_usage(repo: &UsageLogRepo, batch: &mut Vec<UsageLogEntry>) {
    if batch.is_empty() {
        return;
    }
    let count = batch.len();
    match repo.batch_insert(batch).await {
        Ok(()) => {
            tracing::debug!(count, "usage logs flushed");
            batch.clear();
        }
        Err(e) => {
            tracing::error!(error = %e, count, "failed to flush usage logs, will fallback");
            // fallback：落盘到文件（保证不丢）
            fallback_usage(batch);
            batch.clear();
        }
    }
}

async fn flush_audit(repo: &AuditLogRepo, batch: &mut Vec<AuditLogRecord>) {
    if batch.is_empty() {
        return;
    }
    let count = batch.len();
    match repo.batch_insert(batch).await {
        Ok(()) => {
            tracing::debug!(count, "audit logs flushed");
            batch.clear();
        }
        Err(e) => {
            tracing::error!(error = %e, count, "failed to flush audit logs, will fallback");
            fallback_audit(batch);
            batch.clear();
        }
    }
}

/// fallback：将失败的日志写入文件（/data 或当前目录下的 audit-fallback）。
fn fallback_usage(batch: &[UsageLogEntry]) {
    let path = fallback_path("usage");
    let content: Vec<String> = batch
        .iter()
        .map(|e| serde_json::to_string(e).unwrap_or_else(|_| "<serialize failed>".to_string()))
        .collect();
    write_fallback(&path, &content);
}

fn fallback_audit(batch: &[AuditLogRecord]) {
    let path = fallback_path("audit");
    let content: Vec<String> = batch
        .iter()
        .map(|r| serde_json::to_string(r).unwrap_or_else(|_| "<serialize failed>".to_string()))
        .collect();
    write_fallback(&path, &content);
}

fn fallback_path(kind: &str) -> std::path::PathBuf {
    let dir = std::env::var("AUDIT_FALLBACK_DIR").unwrap_or_else(|_| "/data".to_string());
    std::path::Path::new(&dir).join(format!("audit-fallback-{kind}.jsonl"))
}

fn write_fallback(path: &std::path::Path, lines: &[String]) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        for line in lines {
            let _ = writeln!(f, "{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn worker_batches_and_flushes() {
        let store = storage::connect_in_memory().await.unwrap();
        // 先建 account 行（usage_logs 不强制 FK，但 account_usage_summary 需要）
        let now = 1_700_000_000_000i64;
        sqlx::query("INSERT INTO accounts (id, status, created_at, updated_at) VALUES ('acct_t', 'active', ?, ?)")
            .bind(now).bind(now)
            .execute(&store.pool).await.unwrap();

        let sink = start_audit_worker(store.clone(), 5, Duration::from_millis(200), 100);

        // 提交 5 条（达到 batch_size）
        for _ in 0..5 {
            let entry = UsageLogEntry {
                id: Uuid::new_v4().to_string(),
                account_id: "acct_t".to_string(),
                logical_model: "basic".to_string(),
                provider_id: None,
                upstream_model: None,
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                total_tokens: Some(15),
                credits_cost: 1,
                usage_source: domain::UsageSource::Upstream,
                status: "success".to_string(),
                source_ip: None,
                created_at: now,
            };
            sink.log_usage(entry).await;
        }

        // 等待 worker flush
        tokio::time::sleep(Duration::from_millis(400)).await;

        let repo = UsageLogRepo::new(store.clone());
        let logs = repo.list(Some("acct_t"), None, 100, 0).await.unwrap();
        assert_eq!(logs.len(), 5, "all 5 logs should be flushed");
    }

    #[tokio::test]
    async fn worker_flushes_on_interval() {
        let store = storage::connect_in_memory().await.unwrap();
        let now = 1_700_000_000_000i64;
        sqlx::query("INSERT INTO accounts (id, status, created_at, updated_at) VALUES ('acct_i', 'active', ?, ?)")
            .bind(now).bind(now)
            .execute(&store.pool).await.unwrap();

        // batch_size 设很大，靠 interval flush
        let sink = start_audit_worker(store.clone(), 10000, Duration::from_millis(100), 100);
        let entry = UsageLogEntry {
            id: Uuid::new_v4().to_string(),
            account_id: "acct_i".to_string(),
            logical_model: "x".to_string(),
            provider_id: None,
            upstream_model: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            credits_cost: 0,
            usage_source: domain::UsageSource::Upstream,
            status: "success".to_string(),
            source_ip: None,
            created_at: now,
        };
        sink.log_usage(entry).await;

        // 等待 interval flush
        tokio::time::sleep(Duration::from_millis(300)).await;
        let repo = UsageLogRepo::new(store.clone());
        let logs = repo.list(Some("acct_i"), None, 100, 0).await.unwrap();
        assert_eq!(logs.len(), 1, "log should be flushed by interval");
    }

    #[tokio::test]
    async fn worker_drains_on_close() {
        let store = storage::connect_in_memory().await.unwrap();
        let now = 1_700_000_000_000i64;
        sqlx::query("INSERT INTO accounts (id, status, created_at, updated_at) VALUES ('acct_d', 'active', ?, ?)")
            .bind(now).bind(now)
            .execute(&store.pool).await.unwrap();

        let (tx, rx) = mpsc::channel(100);
        let sink = AuditSink::new(tx.clone());
        // 启动 worker
        let store2 = store.clone();
        let handle = tokio::spawn(run_worker(rx, store2, 10000, Duration::from_secs(60)));

        let entry = UsageLogEntry {
            id: Uuid::new_v4().to_string(),
            account_id: "acct_d".to_string(),
            logical_model: "x".to_string(),
            provider_id: None,
            upstream_model: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            credits_cost: 0,
            usage_source: domain::UsageSource::Upstream,
            status: "success".to_string(),
            source_ip: None,
            created_at: now,
        };
        sink.log_usage(entry).await;
        // 关闭所有 sender（sink 持有 tx 的克隆，必须一起 drop）
        drop(sink);
        drop(tx);
        // 等待 worker 退出（drain）
        let _ = handle.await;

        let repo = UsageLogRepo::new(store.clone());
        let logs = repo.list(Some("acct_d"), None, 100, 0).await.unwrap();
        assert_eq!(logs.len(), 1, "log should be drained on close");
    }
}
