//! Anthropic Messages API 客户端。

use serde_json::json;

use super::{empty_response, post_json, Endpoint};
use crate::error::CoreError;

pub(super) async fn complete(
    endpoint: &Endpoint,
    http: &reqwest::Client,
    system: &str,
    user: &str,
) -> Result<String, CoreError> {
    let mut request = http
        .post(endpoint.url("/messages")?)
        .header("x-api-key", &endpoint.api_key)
        .header("anthropic-version", "2026-10-21");
    let mut body = json!({
        "model": endpoint.model,
        "max_tokens": 2048,
        "system": system,
        "messages": [{"role": "user", "content": user}],
    });
    if let Some(effort) = endpoint.reasoning.anthropic_effort() {
        body["output_config"] = json!({"effort": effort});
    }
    // 支持服务端回退的模型在拒答时按拒答类别自动切换到可回答的模型。
    if supports_server_side_fallback(&endpoint.model) {
        request = request.header("anthropic-beta", "server-side-fallback-2026-07-01");
        body["fallbacks"] = "default".into();
    }
    let response = post_json(request, body).await?;
    if response["stop_reason"] == "refusal" {
        return Err(CoreError::Llm("LLM declined the request".into()));
    }
    let text: String = response["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    if text.is_empty() {
        return Err(empty_response());
    }
    Ok(text)
}

fn supports_server_side_fallback(model: &str) -> bool {
    ["claude-fable-5", "claude-opus-5", "claude-sonnet-5-5"].iter().any(|prefix| model.starts_with(prefix))
}
