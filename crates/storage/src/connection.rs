//! SQLite 连接与 pragma 配置。

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

/// SQLite 存储句柄（包装连接池）。
#[derive(Clone)]
pub struct SqliteStore {
    pub pool: SqlitePool,
}

/// 连接磁盘上的 SQLite 文件，应用 pragma 并运行迁移。
pub async fn connect(database_url: &str) -> anyhow::Result<SqliteStore> {
    let opts = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;

    run_migrations(&pool).await?;
    Ok(SqliteStore { pool })
}

/// 内存数据库（测试用）。
pub async fn connect_in_memory() -> anyhow::Result<SqliteStore> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;

    run_migrations(&pool).await?;
    Ok(SqliteStore { pool })
}

/// 运行迁移。迁移文件位于 workspace 根的 migrations/ 目录。
async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    Ok(())
}
