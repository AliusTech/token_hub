//! 积分余额、预冻结、流水。

use serde::{Deserialize, Serialize};

/// 账号积分余额（乐观锁：version 串行化并发扣减）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credits {
    pub account_id: String,
    /// 可用余额
    pub balance: i64,
    /// 冻结额（预冻结未结算）
    pub held: i64,
    /// 乐观锁版本号
    pub version: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HoldStatus {
    /// 已冻结
    Held,
    /// 已结算（按实际用量扣除）
    Settled,
    /// 已释放（未实际调用，退还）
    Released,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditHold {
    pub id: String,
    pub account_id: String,
    pub amount: i64,
    pub status: HoldStatus,
    pub request_id: Option<String>,
    pub created_at: i64,
    pub settled_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditTransaction {
    pub id: String,
    pub account_id: String,
    /// 正=充值/调整，负=扣减
    pub delta: i64,
    pub balance_after: i64,
    pub reason: Option<String>,
    /// admin id / service client_id / "system"
    pub operator: Option<String>,
    pub created_at: i64,
}
