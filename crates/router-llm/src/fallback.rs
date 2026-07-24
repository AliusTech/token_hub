//! 自动降级：检测上游"额度耗尽/欠费"错误，决定是否切换到下一个 provider。
//!
//! 触发降级的错误类型：
//! - HTTP 402 Payment Required（欠费）
//! - HTTP 429 Too Many Requests（限流/配额耗尽，当 body 含 quota 关键词时）
//! - 响应体含 quota exceeded / insufficient quota / billing 关键词
//!
//! 不触发降级（属于调用方问题，应直接返回错误）：
//! - 400 参数错误、401 认证失败、404 模型不存在
//! - 5xx 服务器错误（应重试而非降级，但 MVP 可也降级）

use crate::client::UpstreamError;

/// 降级决策结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackOutcome {
    /// 应该降级到下一个 provider（额度类错误）
    ShouldDisableAndFallback { reason: String },
    /// 不应降级（调用方错误，直接返回）
    NoFallback,
}

/// 检测上游错误是否属于"额度耗尽"类，应触发降级 + 标记 disabled。
pub struct QuotaErrorDetector;

impl QuotaErrorDetector {
    /// 判断错误是否应触发 provider 禁用 + 降级。
    pub fn classify(err: &UpstreamError) -> FallbackOutcome {
        match err {
            UpstreamError::Status { status, body } => {
                match *status {
                    402 => FallbackOutcome::ShouldDisableAndFallback {
                        reason: format!("payment required (402): {}", truncate(body, 200)),
                    },
                    429 => {
                        // 429 可能是临时限流也可能是配额耗尽。
                        // body 含 quota/billing 关键词 → 配额耗尽（降级）。
                        // 否则视为临时限流（也降级，避免持续打同一个）。
                        let lower = body.to_lowercase();
                        if lower.contains("quota")
                            || lower.contains("billing")
                            || lower.contains("limit")
                            || lower.contains("exceeded")
                            || lower.contains("credit")
                        {
                            FallbackOutcome::ShouldDisableAndFallback {
                                reason: format!("quota exhausted (429): {}", truncate(body, 200)),
                            }
                        } else {
                            // 临时限流：也降级到下一个，但不一定永久 disable
                            // MVP：统一降级 + disable，由管理员恢复
                            FallbackOutcome::ShouldDisableAndFallback {
                                reason: format!("rate limited (429): {}", truncate(body, 200)),
                            }
                        }
                    }
                    403 => {
                        let lower = body.to_lowercase();
                        if lower.contains("quota") || lower.contains("billing") || lower.contains("limit") {
                            FallbackOutcome::ShouldDisableAndFallback {
                                reason: format!("forbidden-quota (403): {}", truncate(body, 200)),
                            }
                        } else {
                            FallbackOutcome::NoFallback
                        }
                    }
                    // 5xx：服务端错误，降级尝试其他 provider
                    s if s >= 500 => FallbackOutcome::ShouldDisableAndFallback {
                        reason: format!("upstream server error ({status})"),
                    },
                    // 400/401/404 等：调用方问题，不降级
                    _ => FallbackOutcome::NoFallback,
                }
            }
            UpstreamError::Timeout => {
                // 超时：降级尝试，但不算额度问题（不 disable）
                FallbackOutcome::ShouldDisableAndFallback {
                    reason: "upstream timeout".to_string(),
                }
            }
            UpstreamError::Connect(_) => FallbackOutcome::ShouldDisableAndFallback {
                reason: "upstream connection failed".to_string(),
            },
            UpstreamError::Other(_) => FallbackOutcome::NoFallback,
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payment_required_triggers_fallback() {
        let err = UpstreamError::Status {
            status: 402,
            body: "insufficient credits".to_string(),
        };
        assert!(matches!(
            QuotaErrorDetector::classify(&err),
            FallbackOutcome::ShouldDisableAndFallback { .. }
        ));
    }

    #[test]
    fn quota_429_triggers_fallback() {
        let err = UpstreamError::Status {
            status: 429,
            body: "You exceeded your current quota".to_string(),
        };
        assert!(matches!(
            QuotaErrorDetector::classify(&err),
            FallbackOutcome::ShouldDisableAndFallback { .. }
        ));
    }

    #[test]
    fn plain_429_also_falls_back() {
        let err = UpstreamError::Status {
            status: 429,
            body: "Too Many Requests".to_string(),
        };
        // 临时限流也降级（MVP 策略）
        assert!(matches!(
            QuotaErrorDetector::classify(&err),
            FallbackOutcome::ShouldDisableAndFallback { .. }
        ));
    }

    #[test]
    fn bad_request_no_fallback() {
        let err = UpstreamError::Status {
            status: 400,
            body: "invalid model".to_string(),
        };
        assert_eq!(QuotaErrorDetector::classify(&err), FallbackOutcome::NoFallback);
    }

    #[test]
    fn auth_error_no_fallback() {
        let err = UpstreamError::Status {
            status: 401,
            body: "invalid api key".to_string(),
        };
        assert_eq!(QuotaErrorDetector::classify(&err), FallbackOutcome::NoFallback);
    }

    #[test]
    fn server_error_triggers_fallback() {
        let err = UpstreamError::Status {
            status: 503,
            body: "service unavailable".to_string(),
        };
        assert!(matches!(
            QuotaErrorDetector::classify(&err),
            FallbackOutcome::ShouldDisableAndFallback { .. }
        ));
    }

    #[test]
    fn timeout_triggers_fallback() {
        let err = UpstreamError::Timeout;
        assert!(matches!(
            QuotaErrorDetector::classify(&err),
            FallbackOutcome::ShouldDisableAndFallback { .. }
        ));
    }

    #[test]
    fn forbidden_quota_triggers_fallback() {
        let err = UpstreamError::Status {
            status: 403,
            body: "billing limit reached".to_string(),
        };
        assert!(matches!(
            QuotaErrorDetector::classify(&err),
            FallbackOutcome::ShouldDisableAndFallback { .. }
        ));
    }

    #[test]
    fn forbidden_non_quota_no_fallback() {
        let err = UpstreamError::Status {
            status: 403,
            body: "access denied".to_string(),
        };
        assert_eq!(QuotaErrorDetector::classify(&err), FallbackOutcome::NoFallback);
    }
}
