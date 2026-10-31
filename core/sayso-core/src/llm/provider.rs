use super::WireFormat;

/// 内置的 LLM 服务预设。只提供协议、默认地址与默认模型，用户可在设置中覆盖地址和模型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum LlmProvider {
    Openai,
    Claude,
    Deepseek,
    Doubao,
    Bailian,
    Kimi,
    Zhipu,
    Gemini,
    Openrouter,
    Ollama,
    Custom,
}

/// 设置界面列出的服务信息。
#[derive(Clone, Debug, uniffi::Record)]
pub struct LlmProviderInfo {
    pub provider: LlmProvider,
    pub display_name: String,
    pub default_base_url: String,
    pub default_model: String,
    /// 本地服务无需 API Key。
    pub requires_api_key: bool,
}

impl LlmProvider {
    pub const ALL: [LlmProvider; 11] = [
        Self::Openai,
        Self::Claude,
        Self::Deepseek,
        Self::Doubao,
        Self::Bailian,
        Self::Kimi,
        Self::Zhipu,
        Self::Gemini,
        Self::Openrouter,
        Self::Ollama,
        Self::Custom,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Claude => "claude",
            Self::Deepseek => "deepseek",
            Self::Doubao => "doubao",
            Self::Bailian => "bailian",
            Self::Kimi => "kimi",
            Self::Zhipu => "zhipu",
            Self::Gemini => "gemini",
            Self::Openrouter => "openrouter",
            Self::Ollama => "ollama",
            Self::Custom => "custom",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Openai => "OpenAI",
            Self::Claude => "Claude (Anthropic)",
            Self::Deepseek => "DeepSeek",
            Self::Doubao => "火山方舟 / Volcengine Ark",
            Self::Bailian => "阿里云百炼 / Bailian",
            Self::Kimi => "Kimi (Moonshot)",
            Self::Zhipu => "智谱 GLM / Zhipu",
            Self::Gemini => "Gemini (Google)",
            Self::Openrouter => "OpenRouter",
            Self::Ollama => "Ollama (Local)",
            Self::Custom => "Custom (OpenAI Compatible)",
        }
    }

    pub fn wire_format(self) -> WireFormat {
        if self == Self::Claude {
            WireFormat::Anthropic
        } else {
            WireFormat::OpenAICompatible
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::Openai => "https://api.openai.com/v1",
            Self::Claude => "https://api.anthropic.com/v1",
            Self::Deepseek => "https://api.deepseek.com",
            Self::Doubao => "https://ark.cn-beijing.volces.com/api/v3",
            Self::Bailian => "https://dashscope.aliyuncs.com/compatible-mode/v1",
            Self::Kimi => "https://api.moonshot.ai/v1",
            Self::Zhipu => "https://open.bigmodel.cn/api/paas/v4",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
            Self::Openrouter => "https://openrouter.ai/api/v1",
            Self::Ollama => "http://localhost:11434/v1",
            Self::Custom => "",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Openai => "gpt-5.4-mini",
            Self::Claude => "claude-opus-5-5",
            Self::Deepseek => "deepseek-v4-flash",
            Self::Doubao => "doubao-seed-2-0-mini-260428",
            Self::Bailian => "qwen3.6-flash",
            Self::Kimi => "kimi-k2.6",
            Self::Zhipu => "glm-4.7-flash",
            Self::Gemini => "gemini-3.5-flash",
            Self::Openrouter => "openai/gpt-5.4-mini",
            Self::Ollama => "qwen3",
            Self::Custom => "",
        }
    }

    pub fn requires_api_key(self) -> bool {
        self != Self::Ollama
    }
}

#[uniffi::export]
pub fn llm_providers() -> Vec<LlmProviderInfo> {
    LlmProvider::ALL
        .into_iter()
        .map(|provider| LlmProviderInfo {
            provider,
            display_name: provider.display_name().into(),
            default_base_url: provider.default_base_url().into(),
            default_model: provider.default_model().into(),
            requires_api_key: provider.requires_api_key(),
        })
        .collect()
}
