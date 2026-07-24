//! Repository 定义与 sqlx SQLite 实现。
//!
//! 每个聚合一套 Repo。所有 DB 访问通过这些类型，业务层不直接接触 sqlx。

pub mod account;
pub mod admin_user;
pub mod audit_log;
pub mod credits;
pub mod model;
pub mod policy;
pub mod provider;
pub mod service_account;
pub mod token;
pub mod usage_log;

pub use account::AccountRepo;
pub use admin_user::AdminUserRepo;
pub use audit_log::{AuditLogRepo, AuditLogRecord};
pub use credits::CreditsRepo;
pub use model::{ModelProviderRepo, ModelRepo};
pub use policy::PolicyRepo;
pub use provider::{ModelProviderMappingRepo, ProviderQuotaRepo, ProviderRepo};
pub use service_account::ServiceAccountRepo;
pub use token::TokenRepo;
pub use usage_log::{UsageLogEntry, UsageLogRepo};
