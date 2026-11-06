//! 语音识别来源注册表：统一配置云端服务与通过 URL 接入的本地服务。
//!
//! 每个来源声明自己的配置项，设置中只存「选用的来源 + 各配置项的取值」，界面按声明通用渲染。
//! 密钥类配置项存 `config.json`，其余存 `settings.json`。只有线上协议按服务分别实现（见各服务模块）。

use std::collections::HashMap;

use super::AsrConfig;
use crate::l10n::tr;
use crate::protocol::Language;
use crate::settings::{SecretSlot, Secrets, Settings};

/// 识别服务的稳定 id。
pub const VOLCENGINE: &str = "sayso.volcengine";
pub const QWEN: &str = "sayso.qwen";
pub const SONIOX: &str = "sayso.soniox";
pub const OPENAI_COMPATIBLE: &str = "sayso.openai";

/// 内置来源共用的配置项 key。
const API_KEY: &str = "apiKey";
const MODEL: &str = "model";
const BASE_URL: &str = "baseUrl";

/// 一个识别来源：设置页识别服务列表中的一项。
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AsrSource {
    pub id: String,
    pub title: String,
    /// 来源说明，如支持哪些语言混说。
    pub caption: Option<String>,
    /// 能识别的语言。设置界面据此限制可选的目标语言与本地语言。
    pub languages: Vec<Language>,
    pub fields: Vec<AsrField>,
    /// 获取 API Key 等的外部链接。
    pub link: Option<AsrLink>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AsrField {
    pub key: String,
    pub title: String,
    pub kind: AsrFieldKind,
    /// 可选字段允许空字符串，必填校验统一由核心执行。
    pub required: bool,
    /// 未填写时的取值；选项字段为默认选项的值。
    pub default_value: String,
    pub caption: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AsrFieldKind {
    /// 存于 `config.json`。
    Secret,
    Text,
    Choice {
        options: Vec<AsrFieldOption>,
    },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AsrFieldOption {
    pub value: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AsrLink {
    pub title: String,
    pub url: String,
}

/// 支持的识别服务。顺序即设置页中的显示顺序。
#[derive(Clone, Copy)]
enum Service {
    Volcengine,
    Qwen,
    Soniox,
    OpenAiCompatible,
}

impl Service {
    const ALL: [Service; 4] = [Self::Volcengine, Self::Qwen, Self::Soniox, Self::OpenAiCompatible];

    fn find(id: &str) -> Option<Service> {
        Self::ALL.into_iter().find(|service| service.id() == id)
    }

    fn id(self) -> &'static str {
        match self {
            Self::Volcengine => VOLCENGINE,
            Self::Qwen => QWEN,
            Self::Soniox => SONIOX,
            Self::OpenAiCompatible => OPENAI_COMPATIBLE,
        }
    }

    fn source(self) -> AsrSource {
        let api_key = AsrField {
            key: API_KEY.into(),
            title: "API Key".into(),
            kind: AsrFieldKind::Secret,
            required: true,
            default_value: String::new(),
            caption: None,
        };
        let link = |title: String, url: &str| Some(AsrLink { title, url: url.into() });
        let get_key = || tr("获取 API Key", "Get an API key");
        match self {
            Self::Volcengine => AsrSource {
                id: self.id().into(),
                title: tr("火山引擎 · 豆包（流式）", "Volcengine Doubao (streaming)"),
                caption: Some(tr(
                    "只支持中文、英文与方言混说，不支持日语等其他语言。",
                    "Mixes Chinese, English and Chinese dialects only; other languages such as Japanese are not recognized.",
                )),
                languages: vec![Language::Chinese, Language::English],
                fields: vec![
                    api_key,
                    AsrField {
                        key: MODEL.into(),
                        required: true,
                        title: tr("识别模型", "Model"),
                        kind: AsrFieldKind::Choice {
                            options: vec![
                                AsrFieldOption {
                                    value: "volc.seedasr.sauc.duration".into(),
                                    title: tr("流式语音识别模型 2.0", "Streaming ASR 2.0"),
                                },
                                AsrFieldOption {
                                    value: "volc.bigasr.sauc.duration".into(),
                                    title: tr("流式语音识别大模型 1.0", "Streaming ASR 1.0"),
                                },
                            ],
                        },
                        default_value: "volc.seedasr.sauc.duration".into(),
                        caption: None,
                    },
                ],
                link: link(
                    tr("在火山引擎控制台获取 API Key", "Get an API key in the Volcengine console"),
                    "https://console.volcengine.com/speech/new",
                ),
            },
            Self::Qwen => AsrSource {
                id: self.id().into(),
                title: tr("阿里云百炼 · 千问 3.1 Message", "Alibaba Cloud Qwen 3.1 Message"),
                caption: Some(tr(
                    "面向短句语音输入，按目标语言与本地语言提供识别提示；临时文字可能修订，最终结果确认后再翻译。",
                    "Short-form voice input with target and native language hints. Interim text may change; translation waits for the final transcript.",
                )),
                languages: Language::ALL.to_vec(),
                fields: vec![api_key],
                link: link(
                    tr("在百炼控制台获取 API Key", "Get an API key in the Model Studio console"),
                    "https://bailian.console.aliyun.com/?tab=model#/api-key",
                ),
            },
            Self::Soniox => AsrSource {
                id: self.id().into(),
                title: tr("Soniox（流式）", "Soniox (streaming)"),
                caption: Some(tr(
                    "支持目标语言与本地语言在一句话内混说。",
                    "Your target and native languages can be mixed in one sentence.",
                )),
                languages: Language::ALL.to_vec(),
                fields: vec![api_key],
                link: link(get_key(), "https://console.soniox.com"),
            },
            Self::OpenAiCompatible => AsrSource {
                id: self.id().into(),
                title: tr("OpenAI 兼容 / 本地服务", "OpenAI compatible / local service"),
                caption: Some(tr(
                    "本地模型请先独立启动服务，再填写支持 /audio/transcriptions 的 Base URL；录音结束后上传识别。",
                    "Start your local service separately, then enter its Base URL supporting /audio/transcriptions. Audio is uploaded after recording.",
                )),
                languages: Language::ALL.to_vec(),
                fields: vec![
                    AsrField {
                        required: false,
                        caption: Some(tr("服务无需认证时可留空。", "Leave empty if the service does not require authentication.")),
                        ..api_key
                    },
                    AsrField {
                        key: BASE_URL.into(),
                        required: true,
                        title: "Base URL".into(),
                        kind: AsrFieldKind::Text,
                        default_value: "https://api.openai.com/v1".into(),
                        caption: None,
                    },
                    AsrField {
                        key: MODEL.into(),
                        required: true,
                        title: tr("模型", "Model"),
                        kind: AsrFieldKind::Text,
                        default_value: "gpt-4o-transcribe".into(),
                        caption: Some(tr(
                            "录音结束后整段上传识别，例如 OpenAI gpt-4o-transcribe、Groq whisper-large-v3。",
                            "Uploads the whole recording when you stop, e.g. OpenAI gpt-4o-transcribe or Groq whisper-large-v3.",
                        )),
                    },
                ],
                link: None,
            },
        }
    }

    /// 由全部填写完整的配置项生成识别配置。
    fn config(self, values: &HashMap<String, String>, settings: &Settings, context: &str) -> AsrConfig {
        let value = |key: &str| values[key].clone();
        let languages = settings.languages();
        let context = context.trim().to_string();
        match self {
            Self::Volcengine => AsrConfig::Volcengine { api_key: value(API_KEY), resource_id: value(MODEL), context },
            Self::Qwen => AsrConfig::Qwen { api_key: value(API_KEY), languages, context },
            Self::Soniox => AsrConfig::Soniox { api_key: value(API_KEY), languages, context },
            Self::OpenAiCompatible => AsrConfig::OpenAiCompatible {
                api_key: value(API_KEY),
                base_url: value(BASE_URL),
                model: value(MODEL),
                languages,
                context,
            },
        }
    }
}

/// 可选的识别服务，与扩展的安装和启停无关。
pub(crate) fn sources() -> Vec<AsrSource> {
    Service::ALL.into_iter().map(Service::source).collect()
}

pub(crate) fn find(id: &str) -> Option<AsrSource> {
    Service::find(id).map(Service::source)
}

/// 配置项的当前取值：密钥取自密钥文件，其余取自设置；从未填写过时为默认值。
pub(crate) fn field_value(source_id: &str, field: &AsrField, settings: &Settings, secrets: &Secrets) -> String {
    match field.kind {
        AsrFieldKind::Secret => secrets.get(SecretSlot::Asr { source: source_id.into(), field: field.key.clone() }),
        AsrFieldKind::Text | AsrFieldKind::Choice { .. } => settings
            .asr_options
            .get(source_id)
            .and_then(|values| values.get(&field.key))
            .cloned()
            .unwrap_or_else(|| field.default_value.clone()),
    }
}

/// 按设置生成本次录音的识别配置；来源不可用或有配置项未填写时为 None。
pub(crate) fn config(settings: &Settings, secrets: &Secrets, context: &str) -> Option<AsrConfig> {
    let id = settings.asr_source.as_str();
    let service = Service::find(id)?;
    let values = service
        .source()
        .fields
        .iter()
        .map(|field| {
            let value = field_value(id, field, settings, secrets);
            (!field.required || !value.trim().is_empty()).then(|| (field.key.clone(), value.trim().to_string()))
        })
        .collect::<Option<HashMap<_, _>>>()?;
    Some(service.config(&values, settings, context))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Hotkey, PlatformDefaults};

    fn settings(source: &str) -> Settings {
        let mut settings = Settings::defaults(PlatformDefaults {
            speech_shortcut: Hotkey { key_code: 61, modifiers: 0 },
            selection_shortcut: Hotkey { key_code: 17, modifiers: 0x80000 },
            speech_rate: 0.5,
        });
        settings.asr_source = source.into();
        settings
    }

    fn secret(source: &str, value: &str) -> Secrets {
        let mut secrets = Secrets::default();
        secrets.set(SecretSlot::Asr { source: source.into(), field: API_KEY.into() }, value.into());
        secrets
    }

    #[test]
    fn config_combines_secrets_stored_options_and_defaults() {
        let mut settings = settings(OPENAI_COMPATIBLE);
        assert!(
            matches!(config(&settings, &Secrets::default(), ""), Some(AsrConfig::OpenAiCompatible { api_key, .. }) if api_key.is_empty())
        );

        settings.asr_options.insert(OPENAI_COMPATIBLE.into(), HashMap::from([(MODEL.into(), " whisper-1 ".into())]));
        let Some(AsrConfig::OpenAiCompatible { api_key, base_url, model, .. }) =
            config(&settings, &secret(OPENAI_COMPATIBLE, "sk"), "")
        else {
            panic!("expected an OpenAI compatible config");
        };
        assert_eq!(
            (api_key.as_str(), base_url.as_str(), model.as_str()),
            ("sk", "https://api.openai.com/v1", "whisper-1")
        );

        // 用户清空的配置项不回落到默认值，视为未填写。
        settings.asr_options.insert(OPENAI_COMPATIBLE.into(), HashMap::from([(BASE_URL.into(), String::new())]));
        assert!(config(&settings, &secret(OPENAI_COMPATIBLE, "sk"), "").is_none());
    }

    #[test]
    fn qwen_requires_a_key_and_carries_the_context() {
        let settings = settings(QWEN);
        assert!(config(&settings, &Secrets::default(), "").is_none());
        let Some(AsrConfig::Qwen { context, .. }) =
            config(&settings, &secret(QWEN, "key"), " I am a software engineer. ")
        else {
            panic!("expected Qwen config")
        };
        assert_eq!(context, "I am a software engineer.");
    }
}
