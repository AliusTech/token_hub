//! 计量来源决策：优先上游 usage，无则 tiktoken 兜底。
//!
//! 主路径（>95%）：上游返回 usage → 直接用（权威）。
//! 兜底：自部署/小众模型不返回 usage → tiktoken 估算（标记 fallback）。

use crate::estimator::estimate_prompt_tokens;
use crate::rates::UsageTokens;
use domain::UsageSource;

/// 上游返回的 usage（OpenAI 格式）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct UpstreamUsage {
    #[serde(default)]
    pub prompt_tokens: Option<i64>,
    #[serde(default)]
    pub completion_tokens: Option<i64>,
    #[serde(default)]
    pub total_tokens: Option<i64>,
}

/// 决策后的用量 + 来源标记。
#[derive(Debug, Clone)]
pub struct ResolvedUsage {
    pub tokens: UsageTokens,
    pub source: UsageSource,
}

/// 优先用上游 usage；若上游未返回，用 tiktoken 兜底估算。
/// `prompt_text` / `completion_text` 用于兜底估算（仅在上游无 usage 时使用）。
pub fn resolve_usage(
    upstream: Option<&UpstreamUsage>,
    prompt_text: &str,
    completion_text: &str,
) -> ResolvedUsage {
    if let Some(u) = upstream {
        // 上游返回了 prompt_tokens（主路径）
        if u.prompt_tokens.is_some() || u.completion_tokens.is_some() {
            return ResolvedUsage {
                tokens: UsageTokens {
                    prompt_tokens: u.prompt_tokens.unwrap_or(0),
                    completion_tokens: u.completion_tokens.unwrap_or(0),
                },
                source: UsageSource::Upstream,
            };
        }
    }
    // 兜底：tiktoken 估算
    ResolvedUsage {
        tokens: UsageTokens {
            prompt_tokens: estimate_prompt_tokens(prompt_text),
            completion_tokens: estimate_prompt_tokens(completion_text),
        },
        source: UsageSource::TiktokenFallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_usage_preferred() {
        let upstream = UpstreamUsage {
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            total_tokens: Some(150),
        };
        let resolved = resolve_usage(Some(&upstream), "some prompt", "some response");
        assert_eq!(resolved.tokens.prompt_tokens, 100);
        assert_eq!(resolved.tokens.completion_tokens, 50);
        assert_eq!(resolved.source, UsageSource::Upstream);
    }

    #[test]
    fn tiktoken_fallback_when_no_usage() {
        let resolved = resolve_usage(None, "Hello world test", "Response here");
        assert_eq!(resolved.source, UsageSource::TiktokenFallback);
        assert!(resolved.tokens.prompt_tokens > 0);
        assert!(resolved.tokens.completion_tokens > 0);
    }

    #[test]
    fn tiktoken_fallback_when_usage_empty() {
        // 上游返回 usage 但字段全 None
        let upstream = UpstreamUsage::default();
        let resolved = resolve_usage(Some(&upstream), "Hello", "World");
        assert_eq!(resolved.source, UsageSource::TiktokenFallback);
    }

    #[test]
    fn upstream_partial_usage_still_authoritative() {
        // 只有 prompt_tokens 也用上游（completion 补 0）
        let upstream = UpstreamUsage {
            prompt_tokens: Some(200),
            completion_tokens: None,
            total_tokens: None,
        };
        let resolved = resolve_usage(Some(&upstream), "x", "y");
        assert_eq!(resolved.tokens.prompt_tokens, 200);
        assert_eq!(resolved.tokens.completion_tokens, 0);
        assert_eq!(resolved.source, UsageSource::Upstream);
    }
}
