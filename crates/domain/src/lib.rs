//! 领域模型：TokenHub 核心业务实体与值对象。
//!
//! 本 crate 不依赖任何基础设施（DB/HTTP/Redis），只定义纯领域类型，
//! 供 storage / auth / billing / router-llm / api 等模块复用。

pub mod principal;
pub mod account;
pub mod token;
pub mod credit;
pub mod model;
pub mod provider;
pub mod usage;
pub mod policy;

pub use principal::{Principal, PrincipalKind, Scope};
pub use account::{Account, AccountStatus};
pub use token::{ApiToken, TokenStatus};
pub use credit::{Credits, CreditHold, HoldStatus, CreditTransaction};
pub use model::{LogicalModel, ModelProvider, RoutingStrategy};
pub use provider::{ProviderCredential, ProviderStatus};
pub use usage::{UsageLog, UsageSource};
pub use policy::Policy;
