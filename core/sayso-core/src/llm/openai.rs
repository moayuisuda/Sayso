//! OpenAI Chat Completions 协议客户端，覆盖 OpenAI、DeepSeek、火山方舟、百炼、Ollama 等兼容服务。

use serde_json::json;

use super::{empty_response, post_json, Endpoint};
use crate::error::CoreError;

pub(super) async fn complete(
    endpoint: &Endpoint,
    http: &reqwest::Client,
    system: &str,
    user: &str,
) -> Result<String, CoreError> {
    let mut request = http.post(endpoint.url("/chat/completions")?);
    if !endpoint.api_key.is_empty() {
        request = request.bearer_auth(&endpoint.api_key);
    }
    let mut body = json!({
        "model": endpoint.model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "stream": false,
    });
    if let Some(effort) = endpoint.reasoning.openai_effort() {
        body["reasoning_effort"] = effort.into();
    }
    let response = post_json(request, body).await?;
    response["choices"][0]["message"]["content"]
        .as_str()
        .filter(|content| !content.is_empty())
        .map(str::to_string)
        .ok_or_else(empty_response)
}
