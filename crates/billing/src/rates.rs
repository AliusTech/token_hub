//! 积分汇率计算（整数运算）。
//!
//! 汇率配置：每 1000 token 对应多少积分（input/output 可不同）。
//! credits = prompt_tokens * input_rate_per_1k / 1000
//!        + completion_tokens * output_rate_per_1k / 1000
//! 全程 i64 整数，避免浮点精度问题。

/// token 用量（来自上游 usage 或 tiktoken 估算）。
#[derive(Debug, Clone, Default)]
pub struct UsageTokens {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

impl UsageTokens {
    pub fn total(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// 按汇率计算积分消耗。向下取整（多退少不补，保护用户）。
pub fn compute_credits(
    usage: &UsageTokens,
    input_rate_per_1k: i64,
    output_rate_per_1k: i64,
) -> i64 {
    // 分开计算再相加，避免大数相除丢精度
    let input_credits = usage.prompt_tokens.saturating_mul(input_rate_per_1k) / 1000;
    let output_credits = usage.completion_tokens.saturating_mul(output_rate_per_1k) / 1000;
    input_credits + output_credits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_rate_calculation() {
        // 1000 input * 20/1k + 500 output * 60/1k = 20 + 30 = 50
        let usage = UsageTokens {
            prompt_tokens: 1000,
            completion_tokens: 500,
        };
        assert_eq!(compute_credits(&usage, 20, 60), 50);
    }

    #[test]
    fn fractional_floors_down() {
        // 100 input * 20/1k = 2000/1000 = 2
        // 50 output * 60/1k = 3000/1000 = 3
        let usage = UsageTokens {
            prompt_tokens: 100,
            completion_tokens: 50,
        };
        assert_eq!(compute_credits(&usage, 20, 60), 5);
        // 150 input * 20/1k = 3000/1000 = 3
        let usage2 = UsageTokens {
            prompt_tokens: 150,
            completion_tokens: 0,
        };
        assert_eq!(compute_credits(&usage2, 20, 60), 3);
    }

    #[test]
    fn sub_thousand_still_charges() {
        // 999 input * 1/1k = 0 (向下取整为0)，但 1001 * 1/1k = 1
        let usage = UsageTokens {
            prompt_tokens: 999,
            completion_tokens: 0,
        };
        assert_eq!(compute_credits(&usage, 1, 0), 0);
        let usage2 = UsageTokens {
            prompt_tokens: 1001,
            completion_tokens: 0,
        };
        assert_eq!(compute_credits(&usage2, 1, 0), 1);
    }

    #[test]
    fn zero_usage_zero_credits() {
        let usage = UsageTokens {
            prompt_tokens: 0,
            completion_tokens: 0,
        };
        assert_eq!(compute_credits(&usage, 100, 100), 0);
    }

    #[test]
    fn large_values_no_overflow() {
        // 1M tokens * 100/1k = 100000
        let usage = UsageTokens {
            prompt_tokens: 1_000_000,
            completion_tokens: 1_000_000,
        };
        assert_eq!(compute_credits(&usage, 100, 100), 200000);
    }

    #[test]
    fn separate_input_output_rates() {
        let usage = UsageTokens {
            prompt_tokens: 2000,
            completion_tokens: 1000,
        };
        // 2000*10/1k + 1000*30/1k = 20 + 30 = 50
        assert_eq!(compute_credits(&usage, 10, 30), 50);
    }
}
