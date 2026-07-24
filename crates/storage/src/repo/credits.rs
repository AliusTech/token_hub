//! 积分余额（乐观锁）+ 预冻结 + 流水。
//!
//! 核心并发安全机制：所有扣减走 `UPDATE ... WHERE version=? RETURNING`，
//! 并发请求靠 version 串行化，要么成功要么返回 None（冲突/不足），由调用方重试或拒绝。

use crate::SqliteStore;
use domain::{CreditHold, CreditTransaction, Credits, HoldStatus};
use sqlx::Row;
use uuid::Uuid;

#[derive(Clone)]
pub struct CreditsRepo {
    store: SqliteStore,
}

impl CreditsRepo {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    /// 为账号初始化余额记录（创建账号时调用）。
    pub async fn init(&self, account_id: &str, now: i64) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO credits (account_id, balance, held, version, updated_at) \
             VALUES (?, 0, 0, 0, ?)",
        )
        .bind(account_id)
        .bind(now)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }

    /// 读取余额（含 version）。
    pub async fn get(&self, account_id: &str) -> anyhow::Result<Option<Credits>> {
        let row = sqlx::query(
            "SELECT account_id, balance, held, version, updated_at FROM credits WHERE account_id = ?",
        )
        .bind(account_id)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(row.map(|r| Credits {
            account_id: r.get("account_id"),
            balance: r.get("balance"),
            held: r.get("held"),
            version: r.get("version"),
            updated_at: r.get("updated_at"),
        }))
    }

    /// 充值/管理员调整（delta 可正可负），走乐观锁，记流水。
    /// 返回更新后的余额。若余额不足（delta<0 且 balance+delta<0）返回 None。
    pub async fn adjust(
        &self,
        account_id: &str,
        delta: i64,
        reason: Option<&str>,
        operator: Option<&str>,
        now: i64,
    ) -> anyhow::Result<Option<i64>> {
        let mut tx = self.store.pool.begin().await?;
        let current: Option<(i64, i64)> = sqlx::query(
            "SELECT balance, version FROM credits WHERE account_id = ?",
        )
        .bind(account_id)
        .fetch_optional(&mut *tx)
        .await?
        .map(|r| (r.get::<i64, _>("balance"), r.get::<i64, _>("version")));

        let Some((balance, version)) = current else {
            tx.rollback().await?;
            return Ok(None);
        };
        let new_balance = balance + delta;
        if new_balance < 0 {
            tx.rollback().await?;
            return Ok(None);
        }

        let res = sqlx::query(
            "UPDATE credits SET balance = ?, version = ?, updated_at = ? \
             WHERE account_id = ? AND version = ?",
        )
        .bind(new_balance)
        .bind(version + 1)
        .bind(now)
        .bind(account_id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }

        sqlx::query(
            "INSERT INTO credit_transactions (id, account_id, delta, balance_after, reason, operator, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(account_id)
        .bind(delta)
        .bind(new_balance)
        .bind(reason)
        .bind(operator)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some(new_balance))
    }

    /// 预冻结：从 balance 扣减估算额转入 held。乐观锁 CAS。
    /// 成功返回 (hold_id, new_version)，余额不足/并发冲突返回 None。
    pub async fn place_hold(
        &self,
        account_id: &str,
        amount: i64,
        request_id: Option<&str>,
        now: i64,
    ) -> anyhow::Result<Option<(String, i64)>> {
        let mut tx = self.store.pool.begin().await?;
        let row = sqlx::query(
            "SELECT balance, version FROM credits WHERE account_id = ?",
        )
        .bind(account_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let balance: i64 = row.get("balance");
        let version: i64 = row.get("version");
        if balance < amount {
            tx.rollback().await?;
            return Ok(None);
        }

        let res = sqlx::query(
            "UPDATE credits SET balance = balance - ?, held = held + ?, version = ?, updated_at = ? \
             WHERE account_id = ? AND version = ?",
        )
        .bind(amount)
        .bind(amount)
        .bind(version + 1)
        .bind(now)
        .bind(account_id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }

        let hold_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO credit_holds (id, account_id, amount, status, request_id, created_at) \
             VALUES (?, ?, ?, 'held', ?, ?)",
        )
        .bind(&hold_id)
        .bind(account_id)
        .bind(amount)
        .bind(request_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some((hold_id, version + 1)))
    }

    /// 结算：按实际用量扣减，释放冻结。actual_cost <= hold_amount 则退还差额，否则从 balance 补扣。
    /// 返回更新后的余额。使用 hold 行的 account_id，避免外部传错。
    pub async fn settle_hold(
        &self,
        hold_id: &str,
        actual_cost: i64,
        now: i64,
    ) -> anyhow::Result<Option<i64>> {
        let mut tx = self.store.pool.begin().await?;

        let hold = sqlx::query(
            "SELECT id, account_id, amount, status FROM credit_holds WHERE id = ?",
        )
        .bind(hold_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(hold) = hold else {
            tx.rollback().await?;
            return Ok(None);
        };
        let account_id: String = hold.get("account_id");
        let held_amount: i64 = hold.get("amount");
        let status: String = hold.get("status");
        if status != "held" {
            tx.rollback().await?;
            return Ok(None);
        }

        // 结算差额：held 释放 actual_cost 部分计入真实消费，剩余退还 balance
        let refund = held_amount - actual_cost; // actual>held 时为负（需补扣）
        // 乐观锁更新
        let row = sqlx::query(
            "SELECT balance, held, version FROM credits WHERE account_id = ?",
        )
        .bind(&account_id)
        .fetch_one(&mut *tx)
        .await?;
        let balance: i64 = row.get("balance");
        let held: i64 = row.get("held");
        let version: i64 = row.get("version");

        let new_held = held - held_amount;
        let new_balance = balance + refund;
        if new_balance < 0 || new_held < 0 {
            tx.rollback().await?;
            return Ok(None);
        }

        let res = sqlx::query(
            "UPDATE credits SET balance = ?, held = ?, version = ?, updated_at = ? \
             WHERE account_id = ? AND version = ?",
        )
        .bind(new_balance)
        .bind(new_held)
        .bind(version + 1)
        .bind(now)
        .bind(&account_id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }

        sqlx::query("UPDATE credit_holds SET status = 'settled', settled_at = ? WHERE id = ?")
            .bind(now)
            .bind(hold_id)
            .execute(&mut *tx)
            .await?;

        // 记流水（消费扣减）
        sqlx::query(
            "INSERT INTO credit_transactions (id, account_id, delta, balance_after, reason, operator, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&account_id)
        .bind(-actual_cost)
        .bind(new_balance)
        .bind("usage")
        .bind("system")
        .bind(now)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some(new_balance))
    }

    /// 释放冻结（未实际调用，全额退还）。
    pub async fn release_hold(&self, hold_id: &str, now: i64) -> anyhow::Result<Option<i64>> {
        let mut tx = self.store.pool.begin().await?;
        let hold = sqlx::query(
            "SELECT account_id, amount, status FROM credit_holds WHERE id = ?",
        )
        .bind(hold_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(hold) = hold else {
            tx.rollback().await?;
            return Ok(None);
        };
        let account_id: String = hold.get("account_id");
        let amount: i64 = hold.get("amount");
        let status: String = hold.get("status");
        if status != "held" {
            tx.rollback().await?;
            return Ok(None);
        }

        let row = sqlx::query("SELECT balance, held, version FROM credits WHERE account_id = ?")
            .bind(&account_id)
            .fetch_one(&mut *tx)
            .await?;
        let balance: i64 = row.get("balance");
        let held: i64 = row.get("held");
        let version: i64 = row.get("version");

        let res = sqlx::query(
            "UPDATE credits SET balance = ?, held = ?, version = ?, updated_at = ? \
             WHERE account_id = ? AND version = ?",
        )
        .bind(balance + amount)
        .bind(held - amount)
        .bind(version + 1)
        .bind(now)
        .bind(&account_id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }

        sqlx::query("UPDATE credit_holds SET status = 'released', settled_at = ? WHERE id = ?")
            .bind(now)
            .bind(hold_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(Some(balance + amount))
    }

    /// 查询流水（倒序，分页）。
    pub async fn transactions(
        &self,
        account_id: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<CreditTransaction>> {
        let rows = sqlx::query(
            "SELECT id, account_id, delta, balance_after, reason, operator, created_at \
             FROM credit_transactions WHERE account_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(account_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| CreditTransaction {
                id: r.get("id"),
                account_id: r.get("account_id"),
                delta: r.get("delta"),
                balance_after: r.get("balance_after"),
                reason: r.get("reason"),
                operator: r.get("operator"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    /// 查询账号的活跃 hold（状态为 held）。
    pub async fn active_holds(&self, account_id: &str) -> anyhow::Result<Vec<CreditHold>> {
        let rows = sqlx::query(
            "SELECT id, account_id, amount, status, request_id, created_at, settled_at \
             FROM credit_holds WHERE account_id = ? AND status = 'held' ORDER BY created_at",
        )
        .bind(account_id)
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| CreditHold {
                id: r.get("id"),
                account_id: r.get("account_id"),
                amount: r.get("amount"),
                status: HoldStatus::Held,
                request_id: r.get("request_id"),
                created_at: r.get("created_at"),
                settled_at: r.get("settled_at"),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup() -> (CreditsRepo, String) {
        let store = crate::connect_in_memory().await.unwrap();
        // 建依赖的 account 行
        let account_id = format!("acct_{}", Uuid::new_v4());
        let now: i64 = 1_700_000_000_000;
        sqlx::query("INSERT INTO accounts (id, status, created_at, updated_at) VALUES (?, 'active', ?, ?)")
            .bind(&account_id)
            .bind(now)
            .bind(now)
            .execute(&store.pool)
            .await
            .unwrap();
        let repo = CreditsRepo::new(store);
        repo.init(&account_id, now).await.unwrap();
        (repo, account_id)
    }

    #[tokio::test]
    async fn adjust_works() {
        let (repo, acct) = setup().await;
        let after = repo.adjust(&acct, 1000, Some("init"), Some("admin1"), 1).await.unwrap();
        assert_eq!(after, Some(1000));
        let after2 = repo.adjust(&acct, -300, Some("use"), Some("system"), 2).await.unwrap();
        assert_eq!(after2, Some(700));
        // 扣到负数应失败
        let fail = repo.adjust(&acct, -10000, None, None, 3).await.unwrap();
        assert_eq!(fail, None);
        let c = repo.get(&acct).await.unwrap().unwrap();
        assert_eq!(c.balance, 700);
    }

    #[tokio::test]
    async fn hold_and_settle_refunds_excess() {
        let (repo, acct) = setup().await;
        repo.adjust(&acct, 1000, None, None, 1).await.unwrap();
        let (hold_id, _) = repo.place_hold(&acct, 800, Some("req1"), 2).await.unwrap().unwrap();
        let c = repo.get(&acct).await.unwrap().unwrap();
        assert_eq!(c.balance, 200);
        assert_eq!(c.held, 800);
        // 实际只花了 300，退 500
        let new_bal = repo.settle_hold(&hold_id, 300, 3).await.unwrap().unwrap();
        assert_eq!(new_bal, 700);
        let c = repo.get(&acct).await.unwrap().unwrap();
        assert_eq!(c.balance, 700);
        assert_eq!(c.held, 0);
    }

    #[tokio::test]
    async fn hold_and_settle_charges_more() {
        let (repo, acct) = setup().await;
        repo.adjust(&acct, 1000, None, None, 1).await.unwrap();
        let (hold_id, _) = repo.place_hold(&acct, 300, None, 2).await.unwrap().unwrap();
        // 实际花 500 > 冻结 300，从 balance 补扣 200
        let new_bal = repo.settle_hold(&hold_id, 500, 3).await.unwrap().unwrap();
        assert_eq!(new_bal, 500);
    }

    #[tokio::test]
    async fn release_hold_refunds_full() {
        let (repo, acct) = setup().await;
        repo.adjust(&acct, 1000, None, None, 1).await.unwrap();
        let (hold_id, _) = repo.place_hold(&acct, 400, None, 2).await.unwrap().unwrap();
        let bal = repo.release_hold(&hold_id, 3).await.unwrap().unwrap();
        assert_eq!(bal, 1000);
        let c = repo.get(&acct).await.unwrap().unwrap();
        assert_eq!(c.held, 0);
    }

    /// 关键测试：高并发扣减，最终余额正确、无超卖、无负数。
    /// 10 个线程各扣 100，初始 1000，最终应为 0。
    #[tokio::test]
    async fn concurrent_adjust_no_oversell() {
        let (repo, acct) = setup().await;
        repo.adjust(&acct, 1000, None, None, 1).await.unwrap();

        // 复制 store 句柄给每个任务
        let store = repo.store.clone();
        let n = 10;
        let mut handles = Vec::new();
        for _ in 0..n {
            let store = store.clone();
            let acct = acct.clone();
            handles.push(tokio::spawn(async move {
                let r = CreditsRepo::new(store);
                loop {
                    match r.adjust(&acct, -100, None, Some("t"), 2).await.unwrap() {
                        Some(_) => return true,  // 成功扣一次
                        None => tokio::time::sleep(std::time::Duration::from_millis(1)).await,
                    }
                }
            }));
        }
        let mut successes = 0;
        for h in handles {
            if h.await.unwrap() {
                successes += 1;
            }
        }
        assert_eq!(successes, 10, "all 10 deductions should succeed");
        let c = repo.get(&acct).await.unwrap().unwrap();
        assert_eq!(c.balance, 0, "balance must be exactly 0");
        assert!(c.balance >= 0, "no negative balance");
    }

    /// 关键测试：高并发预冻结，总冻结不超过余额。
    /// 初始 1000，20 个任务各尝试冻结 100，应恰好 10 个成功。
    #[tokio::test]
    async fn concurrent_hold_no_oversell() {
        let (repo, acct) = setup().await;
        repo.adjust(&acct, 1000, None, None, 1).await.unwrap();

        let store = repo.store.clone();
        let n = 20;
        let mut handles = Vec::new();
        for _ in 0..n {
            let store = store.clone();
            let acct = acct.clone();
            handles.push(tokio::spawn(async move {
                let r = CreditsRepo::new(store);
                r.place_hold(&acct, 100, Some("c"), 2).await.unwrap().is_some()
            }));
        }
        let mut held = 0;
        for h in handles {
            if h.await.unwrap() {
                held += 1;
            }
        }
        assert_eq!(held, 10, "exactly 10 holds of 100 should succeed against 1000 balance");
        let c = repo.get(&acct).await.unwrap().unwrap();
        assert_eq!(c.balance, 0);
        assert_eq!(c.held, 1000);
    }
}
