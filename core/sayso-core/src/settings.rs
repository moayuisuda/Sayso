//! 应用设置的唯一数据源：普通字段存 `settings.json`，API Key 存 `config.json`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::asr::source;
use crate::llm::{Endpoint, LlmProvider, ReasoningLevel};
use crate::protocol::{InputSource, Language, LanguagePair};
use crate::store::write_atomically;

/// 快捷键：单修饰键（如右 ⌥）或「修饰键 + 普通键」组合。键码与修饰位是平台的原生值（macOS 为 kVK 键码与 CGEventFlags），
/// core 只做存储与比较。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct Hotkey {
    pub key_code: u16,
    /// 组合键要求的修饰位；单修饰键热键为 0。
    pub modifiers: u64,
}

/// 语音快捷键的触发方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyStyle {
    /// 按住快捷键说话，松开结束。
    Hold,
    /// 按一下开始，再按一下结束。
    Toggle,
}

/// Sayso 输入入口的按键。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct InputShortcut {
    pub key: Hotkey,
    pub input: InputSource,
}

/// 平台提供的默认按键：各平台键码不同。
#[derive(Clone, Copy, Debug, uniffi::Record)]
pub struct PlatformDefaults {
    /// 混说翻译的默认快捷键（macOS 为右 Option）。
    pub speech_shortcut: Hotkey,
    /// 互译的默认快捷键（macOS 为 Option + T）。
    pub selection_shortcut: Hotkey,
    /// 平台语音合成的默认语速。
    pub speech_rate: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub target_language: Language,
    pub native_language: Language,
    pub hotkey_style: HotkeyStyle,
    pub shortcuts: Vec<InputShortcut>,
    /// 扩展输出朗读指令时自动朗读。
    pub auto_speak: bool,
    pub speech_rate: f32,
    /// 选用的识别服务 id，服务配置通过 asr_options 与密钥文件读取。
    pub asr_source: String,
    /// 各识别来源的非密钥配置项，按来源 id、配置项 key 存储；从未填写过的配置项取来源声明的默认值。
    pub asr_options: HashMap<String, HashMap<String, String>>,
    pub llm_provider: LlmProvider,
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_reasoning: ReasoningLevel,
    pub enabled_extensions: Vec<String>,
}

pub const SPEECH_TRANSLATE_ID: &str = "sayso.dictation";
pub const SELECTION_TRANSLATE_ID: &str = "sayso.selection";
pub const WORDBOOK_ID: &str = "sayso.wordbook";
pub const APP_CONTEXT_ID: &str = "sayso.context";

impl Settings {
    pub fn defaults(platform: PlatformDefaults) -> Self {
        let llm = LlmProvider::Openai;
        Settings {
            target_language: Language::English,
            native_language: Language::Chinese,
            hotkey_style: HotkeyStyle::Hold,
            shortcuts: vec![
                InputShortcut { key: platform.speech_shortcut, input: InputSource::Speech },
                InputShortcut { key: platform.selection_shortcut, input: InputSource::Selection },
            ],
            auto_speak: true,
            speech_rate: platform.speech_rate,
            asr_source: source::VOLCENGINE.into(),
            asr_options: HashMap::new(),
            llm_provider: llm,
            llm_base_url: llm.default_base_url().into(),
            llm_model: llm.default_model().into(),
            llm_reasoning: ReasoningLevel::ModelDefault,
            enabled_extensions: vec![
                SPEECH_TRANSLATE_ID.into(),
                SELECTION_TRANSLATE_ID.into(),
                WORDBOOK_ID.into(),
                APP_CONTEXT_ID.into(),
            ],
        }
    }

    pub fn languages(&self) -> LanguagePair {
        LanguagePair { target: self.target_language, native: self.native_language }
    }

    pub fn input_for(&self, key: Hotkey) -> Option<InputSource> {
        self.shortcuts.iter().find(|shortcut| shortcut.key == key).map(|shortcut| shortcut.input)
    }

    pub fn set_shortcut(&mut self, input: InputSource, key: Option<Hotkey>) {
        self.shortcuts.retain(|shortcut| shortcut.input != input && Some(shortcut.key) != key);
        if let Some(key) = key {
            self.shortcuts.push(InputShortcut { key, input });
        }
    }

    /// 规整用户提交的设置：切换翻译服务时载入该服务的默认地址与模型。
    pub fn normalized(mut self, previous: &Settings) -> Self {
        if self.llm_provider != previous.llm_provider {
            self.llm_base_url = self.llm_provider.default_base_url().into();
            self.llm_model = self.llm_provider.default_model().into();
        }
        self
    }
}

/// 密钥项。账户名是 `config.json` 中的键，不可修改。
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SecretSlot {
    /// 识别来源的密钥类配置项。
    Asr {
        source: String,
        field: String,
    },
    Llm {
        provider: LlmProvider,
    },
}

impl SecretSlot {
    pub fn account(&self) -> String {
        match self {
            Self::Asr { source, field } => format!("asr.{source}.{field}"),
            Self::Llm { provider } => format!("llm.{}", provider.key()),
        }
    }
}

/// 全部 API Key，按账户名存于 `config.json`，与可分享的 `settings.json` 分开。
/// 文件位于仅当前用户可访问的应用数据目录中。
#[derive(Default, Serialize, Deserialize)]
pub struct Secrets(HashMap<String, String>);

impl Secrets {
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(data) => serde_json::from_slice(&data).unwrap_or_else(|error| {
                log::error!("secrets file unreadable: {error}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        write_atomically(path, &serde_json::to_vec_pretty(self).expect("secrets are serializable"))
    }

    pub fn get(&self, slot: SecretSlot) -> String {
        self.0.get(&slot.account()).cloned().unwrap_or_default()
    }

    /// 空字符串删除该项。
    pub fn set(&mut self, slot: SecretSlot, value: String) {
        if value.is_empty() {
            self.0.remove(&slot.account());
        } else {
            self.0.insert(slot.account(), value);
        }
    }
}

/// 当前翻译模型端点；缺少必填项时为 None。
pub fn llm_endpoint(settings: &Settings, secrets: &Secrets) -> Option<Endpoint> {
    let provider = settings.llm_provider;
    let api_key = secrets.get(SecretSlot::Llm { provider });
    if settings.llm_base_url.is_empty()
        || settings.llm_model.is_empty()
        || (provider.requires_api_key() && api_key.is_empty())
    {
        return None;
    }
    Some(Endpoint {
        wire_format: provider.wire_format(),
        base_url: settings.llm_base_url.clone(),
        api_key,
        model: settings.llm_model.clone(),
        reasoning: settings.llm_reasoning,
    })
}

pub fn load(path: &PathBuf, defaults: PlatformDefaults) -> Settings {
    match std::fs::read(path) {
        Ok(data) => serde_json::from_slice(&data).unwrap_or_else(|error| {
            log::error!("settings file unreadable, using defaults: {error}");
            Settings::defaults(defaults)
        }),
        Err(_) => Settings::defaults(defaults),
    }
}

pub fn save(path: &PathBuf, settings: &Settings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomically(path, &serde_json::to_vec_pretty(settings).expect("settings are serializable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Settings {
        Settings::defaults(PlatformDefaults {
            speech_shortcut: Hotkey { key_code: 61, modifiers: 0 },
            selection_shortcut: Hotkey { key_code: 17, modifiers: 0x80000 },
            speech_rate: 0.5,
        })
    }

    #[test]
    fn normalizing_resets_llm_defaults() {
        let previous = defaults();
        let mut edited = previous.clone();
        edited.llm_provider = LlmProvider::Claude;
        edited.llm_model = "stale".into();

        let normalized = edited.normalized(&previous);
        assert_eq!(normalized.llm_base_url, LlmProvider::Claude.default_base_url());
        assert_eq!(normalized.llm_model, LlmProvider::Claude.default_model());
    }

    #[test]
    fn moving_an_input_shortcut_reassigns_its_source() {
        let mut settings = defaults();
        let key = settings.shortcuts[0].key;
        settings.set_shortcut(InputSource::Selection, Some(key));
        assert_eq!(settings.shortcuts, [InputShortcut { key, input: InputSource::Selection }]);
    }

    #[test]
    fn shortcuts_round_trip_through_json() {
        let settings = defaults();
        let json = serde_json::to_value(&settings).unwrap();
        assert!(json["shortcuts"][0]["key"]["keyCode"] == 61);
        assert_eq!(serde_json::from_value::<Settings>(json).unwrap(), settings);
    }
}
