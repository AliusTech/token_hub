//! tiktoken 估算（用于预冻结额度 + 无 usage 兜底）。
//!
//! 注意：tiktoken 对 OpenAI 模型准确，对 Claude/Gemini 等有偏差。
//! 此处仅用于"估算"，最终计费以上游 usage 为准（见 usage_source）。

use tiktoken_rs::cl100k_base;

/// 估算文本的 token 数（cl100k_base，GPT-3.5/4 系列）。
/// 用于预冻结时估算 prompt 的额度占用。
pub fn estimate_prompt_tokens(text: &str) -> i64 {
    let bpe = match cl100k_base() {
        Ok(bpe) => bpe,
        // BPE 初始化失败时退化为字符数粗估（最坏情况高估，宁可多冻不少冻）
        Err(_) => return text.chars().count() as i64,
    };
    match bpe.encode_with_special_tokens(text) {
        tokens => tokens.len() as i64,
    }
}

/// 估算多条消息的总 prompt token 数（近似 OpenAI 的计算方式）。
/// 每条消息约 4 个额外 token（role 标记 + 分隔符）。
pub fn estimate_messages_tokens(messages: &[String]) -> i64 {
    let bpe = match cl100k_base() {
        Ok(bpe) => bpe,
        Err(_) => return messages.iter().map(|m| m.chars().count() as i64).sum::<i64>() + messages.len() as i64 * 4,
    };
    let mut total = 0i64;
    for msg in messages {
        total += bpe.encode_with_special_tokens(msg).len() as i64 + 4;
    }
    total += 2; // 每个对话的 priming
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_nonzero() {
        let tokens = estimate_prompt_tokens("Hello world, this is a test of tokenization.");
        assert!(tokens > 0, "should estimate some tokens");
        // 这句话大概 8-10 token
        assert!(tokens < 30, "should be reasonable, got {tokens}");
    }

    #[test]
    fn empty_string_zero() {
        assert_eq!(estimate_prompt_tokens(""), 0);
    }

    #[test]
    fn messages_adds_overhead() {
        let tokens = estimate_messages_tokens(&["Hello".to_string(), "World".to_string()]);
        // 两条消息各至少 4 token overhead + 内容 + 2 priming
        assert!(tokens >= 10, "should include overhead, got {tokens}");
    }

    #[test]
    fn chinese_text_estimates() {
        let tokens = estimate_prompt_tokens("你好世界，这是一个测试");
        assert!(tokens > 0, "chinese text should estimate tokens, got {tokens}");
    }
}
