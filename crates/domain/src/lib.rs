//! 领域模型：TokenHub 核心业务实体与值对象。
//!
//! 本 crate 不依赖任何基础设施（DB/HTTP/Redis），只定义纯领域类型，
//! 供 storage / auth / billing / router-llm / api 等模块复用。

pub mod account;
pub mod credit;
pub mod model;
pub mod policy;
pub mod principal;
pub mod provider;
pub mod token;
pub mod usage;

pub use account::{Account, AccountStatus};
pub use credit::{CreditHold, CreditTransaction, Credits, HoldStatus};
pub use model::{LogicalModel, ModelProvider, RoutingStrategy};
pub use policy::Policy;
pub use principal::{Principal, PrincipalKind, Scope};
pub use provider::{ProviderCredential, ProviderStatus};
pub use token::{ApiToken, TokenStatus};
pub use usage::{UsageLog, UsageSource};
