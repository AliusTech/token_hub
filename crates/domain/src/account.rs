//! 账号（应用用户/业务账号）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    Active,
    Disabled,
}

impl Default for AccountStatus {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub external_id: Option<String>,
    pub status: AccountStatus,
    pub policy_id: Option<String>,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
