//! 存储层：Repository trait + sqlx SQLite 实现 + 迁移。
//!
//! 设计要点：
//! - 所有 DB 访问通过 Repository trait，业务层不直接接触 sqlx。
//! - 未来切换 PostgreSQL 只需新增实现，不动业务代码。
//! - SQLite 启用 WAL + foreign_keys + busy_timeout，缓解并发写。

pub mod connection;
pub mod repo;

pub use connection::{connect, connect_in_memory, SqliteStore};

// 顶层便捷 re-export
pub use repo::AccountRepo;
pub use repo::AdminUserRepo;
pub use repo::AuditLogRecord;
pub use repo::AuditLogRepo;
pub use repo::CreditsRepo;
pub use repo::ModelProviderRepo;
pub use repo::ModelRepo;
pub use repo::PolicyRepo;
pub use repo::ProviderQuotaRepo;
pub use repo::ProviderRepo;
pub use repo::ServiceAccountRepo;
pub use repo::TokenRepo;
pub use repo::UsageLogEntry;
pub use repo::UsageLogRepo;
