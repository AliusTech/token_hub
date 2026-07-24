//! 审计与日志：mpsc channel + 批量写入 usage_logs / audit_logs。
//!
//! 设计：
//! - 调用方通过 AuditSink 提交日志（非阻塞，channel 满时背压等待或丢弃）。
//! - 后台 worker 每 N 条或每 T 秒批量 flush 到 SQLite。
//! - channel 崩溃/满时，fallback 落盘到文件，保证不丢。
//! - 关键计费数据（扣减）已实时落 credits 表，日志丢失不影响余额正确性。

pub mod sink;
pub mod notifier;

pub use sink::{AuditSink, AuditCommand, start_audit_worker};
pub use notifier::{Notifier, ConsoleNotifier};
