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

/// 扩展命令的唯一标识：扩展 id + 命令 id。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CommandKey {
    pub extension_id: String,
    pub command_id: String,
}

/// 快捷键触发的命令及命令收到的输入来源。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct Shortcut {
    pub command: CommandKey,
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
    /// 用户自述的身份或领域（如「我是软件工程师」），提供给识别与翻译模型以提升专业词汇的准确率。
    pub user_context: String,
    pub hotkey_style: HotkeyStyle,
    /// 以快捷键为键：一个快捷键只触发一个命令；一个命令也只有一个快捷键，没有快捷键的命令不会被触发。
    #[serde(with = "shortcut_entries")]
    pub shortcuts: HashMap<Hotkey, Shortcut>,
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

/// 翻译背景的长度上限（字符），取各识别服务上下文限制中最小的千问 400 字。
pub const USER_CONTEXT_LIMIT: usize = 400;

pub const SPEECH_TRANSLATE_ID: &str = "sayso.dictation";
pub const SELECTION_TRANSLATE_ID: &str = "sayso.selection";
pub const WORDBOOK_ID: &str = "sayso.wordbook";

impl Settings {
    pub fn defaults(platform: PlatformDefaults) -> Self {
        let llm = LlmProvider::Openai;
        let translate =
            |extension_id: &str| CommandKey { extension_id: extension_id.into(), command_id: "translate".into() };
        Settings {
            target_language: Language::English,
            native_language: Language::Chinese,
            user_context: String::new(),
            hotkey_style: HotkeyStyle::Hold,
            shortcuts: HashMap::from([
                (
                    platform.speech_shortcut,
                    Shortcut { command: translate(SPEECH_TRANSLATE_ID), input: InputSource::Speech },
                ),
                (
                    platform.selection_shortcut,
                    Shortcut { command: translate(SELECTION_TRANSLATE_ID), input: InputSource::Selection },
                ),
            ]),
            auto_speak: true,
            speech_rate: platform.speech_rate,
            asr_source: source::VOLCENGINE.into(),
            asr_options: HashMap::new(),
            llm_provider: llm,
            llm_base_url: llm.default_base_url().into(),
            llm_model: llm.default_model().into(),
            llm_reasoning: ReasoningLevel::ModelDefault,
            enabled_extensions: vec![SPEECH_TRANSLATE_ID.into(), SELECTION_TRANSLATE_ID.into(), WORDBOOK_ID.into()],
        }
    }

    pub fn languages(&self) -> LanguagePair {
        LanguagePair { target: self.target_language, native: self.native_language }
    }

    /// 命令的快捷键。
    pub fn shortcut_of(&self, command: &CommandKey) -> Option<Hotkey> {
        self.shortcuts.iter().find(|(_, shortcut)| &shortcut.command == command).map(|(key, _)| *key)
    }

    /// 设置命令的快捷键，`None` 为移除。快捷键已属于其他命令时从那个命令上移走；
    /// 改键时保留原输入来源，首次设置为语音。
    pub fn set_shortcut(&mut self, command: CommandKey, key: Option<Hotkey>) {
        let previous = self.shortcut_of(&command).and_then(|key| self.shortcuts.remove(&key));
        if let Some(key) = key {
            let input = previous.map_or(InputSource::Speech, |shortcut| shortcut.input);
            self.shortcuts.insert(key, Shortcut { command, input });
        }
    }

    /// 修改命令快捷键的输入来源；命令没有快捷键时不变。
    pub fn set_shortcut_input(&mut self, command: &CommandKey, input: InputSource) {
        if let Some(shortcut) = self.shortcuts.values_mut().find(|shortcut| &shortcut.command == command) {
            shortcut.input = input;
        }
    }

    /// 规整用户提交的设置：翻译背景截断到长度上限；切换翻译服务时载入该服务的默认地址与模型。
    pub fn normalized(mut self, previous: &Settings) -> Self {
        self.user_context = self.user_context.chars().take(USER_CONTEXT_LIMIT).collect();
        if self.llm_provider != previous.llm_provider {
            self.llm_base_url = self.llm_provider.default_base_url().into();
            self.llm_model = self.llm_provider.default_model().into();
        }
        self
    }
}

/// 快捷键表在 JSON 中存为 `[{ key, command, input }]`：JSON 对象的键只能是字符串。
mod shortcut_entries {
    use std::collections::HashMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{CommandKey, Hotkey, InputSource, Shortcut};

    #[derive(Serialize, Deserialize)]
    struct Entry {
        key: Hotkey,
        command: CommandKey,
        input: InputSource,
    }

    pub fn serialize<S: Serializer>(shortcuts: &HashMap<Hotkey, Shortcut>, serializer: S) -> Result<S::Ok, S::Error> {
        let entries: Vec<Entry> = shortcuts
            .iter()
            .map(|(key, shortcut)| Entry { key: *key, command: shortcut.command.clone(), input: shortcut.input })
            .collect();
        entries.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<HashMap<Hotkey, Shortcut>, D::Error> {
        let entries = Vec::<Entry>::deserialize(deserializer)?;
        Ok(entries
            .into_iter()
            .map(|entry| (entry.key, Shortcut { command: entry.command, input: entry.input }))
            .collect())
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

    fn translate(extension: &str) -> CommandKey {
        CommandKey { extension_id: extension.into(), command_id: "translate".into() }
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
    fn a_shortcut_moves_to_the_command_that_takes_it_and_keeps_input_on_rekey() {
        let mut settings = defaults();
        let speech = translate(SPEECH_TRANSLATE_ID);
        let selection = translate(SELECTION_TRANSLATE_ID);

        // 改键：保留原输入来源，重复提交不变。
        let other = Hotkey { key_code: 89, modifiers: 0x80000 };
        settings.set_shortcut(selection.clone(), Some(other));
        assert_eq!(settings.shortcuts[&other].input, InputSource::Selection);
        let rekeyed = settings.clone();
        settings.set_shortcut(selection.clone(), Some(other));
        assert_eq!(settings, rekeyed);

        // 占用其他命令的快捷键：原命令失去快捷键。
        let speech_key = settings.shortcut_of(&speech).unwrap();
        settings.set_shortcut(selection.clone(), Some(speech_key));
        assert_eq!(settings.shortcuts[&speech_key].command, selection);
        assert_eq!(settings.shortcut_of(&speech), None);
        assert_eq!(settings.shortcuts.len(), 1);

        // 首次设置为语音；移除后命令没有快捷键。
        settings.set_shortcut(speech.clone(), Some(other));
        assert_eq!(settings.shortcuts[&other].input, InputSource::Speech);
        settings.set_shortcut_input(&speech, InputSource::Selection);
        assert_eq!(settings.shortcuts[&other].input, InputSource::Selection);
        settings.set_shortcut(speech.clone(), None);
        assert_eq!(settings.shortcut_of(&speech), None);
        assert_eq!(settings.shortcuts.len(), 1);
    }

    #[test]
    fn shortcuts_round_trip_through_json() {
        let settings = defaults();
        let json = serde_json::to_value(&settings).unwrap();
        assert!(json["shortcuts"].as_array().unwrap().iter().any(|entry| entry["key"]["keyCode"] == 61));
        assert_eq!(serde_json::from_value::<Settings>(json).unwrap(), settings);
    }
}
