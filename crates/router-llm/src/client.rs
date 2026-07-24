//! 上游 LLM 客户端：调用供应商 chat completions，解析响应 + usage。
//!
//! 支持非流式（一次性返回）和流式（SSE）两种模式。
//! 流式模式累计各 chunk 的 usage（OpenAI 在最后一个 chunk 返回 usage）。

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// 调用上游的错误。
#[derive(Debug, Error)]
pub enum UpstreamError {
    #[error("upstream returned status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("upstream request timeout")]
    Timeout,
    #[error("upstream connection error: {0}")]
    Connect(String),
    #[error("other upstream error: {0}")]
    Other(String),
}

/// OpenAI 兼容的 chat 请求。
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// OpenAI 兼容的非流式响应。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatResponse {
    pub id: String,
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub usage: Option<ResponseUsage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatChoice {
    pub index: i32,
    pub message: Option<ChatMessage>,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ResponseUsage {
    #[serde(default)]
    pub prompt_tokens: i64,
    #[serde(default)]
    pub completion_tokens: i64,
    #[serde(default)]
    pub total_tokens: i64,
}

/// 上游客户端（封装 reqwest，带超时）。
#[derive(Clone)]
pub struct UpstreamClient {
    http: Client,
}

impl UpstreamClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client build");
        Self { http }
    }

    /// 非流式调用上游 chat completions。
    /// `base_url` 如 `https://api.openai.com/v1`，`api_key` 为供应商凭证。
    pub async fn chat(
        &self,
        base_url: &str,
        api_key: &str,
        req: &ChatRequest,
    ) -> Result<ChatResponse, UpstreamError> {
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .bearer_auth(api_key)
            .json(req)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    UpstreamError::Timeout
                } else if e.is_connect() {
                    UpstreamError::Connect(e.to_string())
                } else {
                    UpstreamError::Other(e.to_string())
                }
            })?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let body = resp.text().await.unwrap_or_default();
            return Err(UpstreamError::Status { status, body });
        }

        resp.json::<ChatResponse>()
            .await
            .map_err(|e| UpstreamError::Other(format!("response parse: {e}")))
    }

    /// 流式调用上游，返回原始 SSE 字节流（供 handler 转发）。
    /// 流式 usage 在 SSE 的最后一个 data 帧里（OpenAI 格式）。
    pub async fn chat_stream(
        &self,
        base_url: &str,
        api_key: &str,
        req: &ChatRequest,
    ) -> Result<reqwest::Response, UpstreamError> {
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .bearer_auth(api_key)
            .json(req)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    UpstreamError::Timeout
                } else if e.is_connect() {
                    UpstreamError::Connect(e.to_string())
                } else {
                    UpstreamError::Other(e.to_string())
                }
            })?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let body = resp.text().await.unwrap_or_default();
            return Err(UpstreamError::Status { status, body });
        }
        Ok(resp)
    }
}

impl Default for UpstreamClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_request_serializes() {
        let req = ChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }],
            stream: Some(false),
            temperature: Some(0.7),
            max_tokens: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "gpt-4o");
        assert_eq!(json["stream"], false);
        // max_tokens=None 且 skip_serializing_if → 字段被省略
        assert!(json.get("max_tokens").is_none());
    }

    #[test]
    fn response_parses_with_usage() {
        let json = r#"{
            "id": "chatcmpl-1",
            "choices": [{"index": 0, "message": {"role":"assistant","content":"hi"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        }"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, "chatcmpl-1");
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(resp.choices[0].message.as_ref().unwrap().content, "hi");
    }

    #[test]
    fn response_parses_without_usage() {
        let json = r#"{
            "id": "chatcmpl-2",
            "choices": [{"index": 0, "message": {"role":"assistant","content":"hi"}}]
        }"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.usage.is_none());
    }
}
