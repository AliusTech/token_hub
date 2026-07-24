//! 告警通知器（Notifier trait + Console 实现）。
//!
//! 用于 80% 供应商额度告警。MVP 用 Console（写日志）。
//! 未来可扩展 SmsNotifier / EmailNotifier / WebhookNotifier。

use async_trait::async_trait;

/// 通知器接口。
#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify(&self, title: &str, message: &str);
}

/// 控制台通知器（写 tracing 日志）。MVP 默认实现。
pub struct ConsoleNotifier;

#[async_trait]
impl Notifier for ConsoleNotifier {
    async fn notify(&self, title: &str, message: &str) {
        tracing::warn!(title, message, "告警通知");
    }
}

/// 静默通知器（不做任何事，用于测试/关闭告警）。
pub struct NoopNotifier;

#[async_trait]
impl Notifier for NoopNotifier {
    async fn notify(&self, _title: &str, _message: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn console_notifier_does_not_panic() {
        let n = ConsoleNotifier;
        n.notify("test", "hello").await;
        // 无 panic 即通过
    }

    #[tokio::test]
    async fn noop_notifier_silent() {
        let n = NoopNotifier;
        n.notify("test", "hello").await;
    }
}
