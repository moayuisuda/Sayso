//! 应用设置的唯一数据源：普通字段存 `settings.json`，API Key 存 `config.json`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::asr::source;
use crate::llm::{Endpoint, LlmProvider, ReasoningLevel};
use crate::protocol::{InputSource, Language, LanguagePair};
use crate::store::write_atomically;

/// 统一入口键。键码与修饰位是平台的原生值（macOS 为 kVK 键码与 CGEventFlags），core 只做存储与比较。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct Hotkey {
    pub key_code: u16,
    /// 组合键要求的修饰位；单修饰键热键为 0。
    pub modifiers: u64,
}

/// 入口键的触发方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyStyle {
    /// 按住入口键说话，松开结束。
    Hold,
    /// 按一下开始，再按一下结束。
    Toggle,
}

/// 路由：只按入口键为 `Direct`；按住入口键期间再按某个键为 `Key`，这个键即路由键。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Enum)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RouteKey {
    Direct,
    Key { code: u16 },
}

/// 一个路由及其输入来源。路由上的全部命令收到同一份输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct Route {
    pub key: RouteKey,
    pub input: InputSource,
}

/// 扩展命令的唯一标识：扩展 id + 命令 id。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CommandKey {
    pub extension_id: String,
    pub command_id: String,
}

/// 命令绑定到的路由；没有绑定的命令不会被触发。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct CommandBinding {
    pub command: CommandKey,
    pub route: RouteKey,
}

/// 平台提供的默认按键：各平台键码不同。
#[derive(Clone, Copy, Debug, uniffi::Record)]
pub struct PlatformDefaults {
    /// 默认入口键（macOS 为右 Option）。
    pub trigger_key: Hotkey,
    /// 划词翻译的默认路由键（macOS 为 T）。
    pub selection_route_key: u16,
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
    pub trigger_key: Hotkey,
    /// 全部路由；「直接」路由总是存在。
    pub routes: Vec<Route>,
    pub command_bindings: Vec<CommandBinding>,
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
        let selection = RouteKey::Key { code: platform.selection_route_key };
        let translate =
            |extension_id: &str| CommandKey { extension_id: extension_id.into(), command_id: "translate".into() };
        Settings {
            target_language: Language::English,
            native_language: Language::Chinese,
            user_context: String::new(),
            hotkey_style: HotkeyStyle::Hold,
            trigger_key: platform.trigger_key,
            routes: vec![
                Route { key: RouteKey::Direct, input: InputSource::Speech },
                Route { key: selection, input: InputSource::Selection },
            ],
            command_bindings: vec![
                CommandBinding { command: translate(SPEECH_TRANSLATE_ID), route: RouteKey::Direct },
                CommandBinding { command: translate(SELECTION_TRANSLATE_ID), route: selection },
            ],
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

    pub fn route(&self, key: RouteKey) -> Option<Route> {
        self.routes.iter().copied().find(|route| route.key == key)
    }

    pub fn binding(&self, command: &CommandKey) -> Option<RouteKey> {
        self.command_bindings.iter().find(|binding| &binding.command == command).map(|binding| binding.route)
    }

    /// 改绑到新路由时继承原输入来源；首次绑定使用语音，已有目标路由的输入来源保持不变。
    pub fn bind_command(&mut self, command: CommandKey, route: Option<RouteKey>) {
        let input = self.binding(&command).and_then(|key| self.route(key)).map_or(InputSource::Speech, |r| r.input);
        self.command_bindings.retain(|binding| binding.command != command);
        if let Some(key) = route {
            if self.route(key).is_none() {
                self.routes.push(Route { key, input });
            }
            self.command_bindings.push(CommandBinding { command, route: key });
        }
        self.normalize_routes();
    }

    /// 输入来源属于路由，同一路由上的全部命令共享；路由仅由命令绑定操作创建。
    pub fn set_route_input(&mut self, key: RouteKey, input: InputSource) {
        if let Some(route) = self.routes.iter_mut().find(|route| route.key == key) {
            route.input = input;
        }
    }

    /// 规整用户提交的设置：翻译背景截断到长度上限；切换翻译服务时载入该服务的默认地址与模型；「直接」路由总是存在；
    /// 删除的路由键上的命令绑定随之移除，不再绑定命令的路由键随之删除。
    pub fn normalized(mut self, previous: &Settings) -> Self {
        self.user_context = self.user_context.chars().take(USER_CONTEXT_LIMIT).collect();
        if self.llm_provider != previous.llm_provider {
            self.llm_base_url = self.llm_provider.default_base_url().into();
            self.llm_model = self.llm_provider.default_model().into();
        }
        self.normalize_routes();
        self
    }

    fn normalize_routes(&mut self) {
        if self.route(RouteKey::Direct).is_none() {
            self.routes.insert(0, Route { key: RouteKey::Direct, input: InputSource::Speech });
        }
        let mut seen = Vec::new();
        self.routes.retain(|route| {
            let fresh = !seen.contains(&route.key);
            seen.push(route.key);
            fresh
        });
        let routes = self.routes.clone();
        self.command_bindings.retain(|binding| routes.iter().any(|route| route.key == binding.route));
        let bindings = self.command_bindings.clone();
        self.routes
            .retain(|route| route.key == RouteKey::Direct || bindings.iter().any(|binding| binding.route == route.key));
    }
}

/// 密钥项。账户名是 `config.json` 中的键，不可修改。
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SecretSlot {
    /// 识别来源的密钥类配置项。
    Asr { source: String, field: String },
    Llm { provider: LlmProvider },
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
            trigger_key: Hotkey { key_code: 61, modifiers: 0 },
            selection_route_key: 17,
            speech_rate: 0.5,
        })
    }

    #[test]
    fn normalizing_resets_llm_defaults_and_drops_orphan_bindings() {
        let previous = defaults();
        let mut edited = previous.clone();
        edited.llm_provider = LlmProvider::Claude;
        edited.llm_model = "stale".into();
        edited.routes.retain(|route| route.key == RouteKey::Direct);

        let normalized = edited.normalized(&previous);
        assert_eq!(normalized.llm_base_url, LlmProvider::Claude.default_base_url());
        assert_eq!(normalized.llm_model, LlmProvider::Claude.default_model());
        assert_eq!(normalized.command_bindings.len(), 1);
        assert_eq!(normalized.command_bindings[0].command.extension_id, SPEECH_TRANSLATE_ID);
    }

    fn translate(extension: &str) -> CommandKey {
        CommandKey { extension_id: extension.into(), command_id: "translate".into() }
    }

    #[test]
    fn rebinding_inherits_input_and_respects_an_existing_destination() {
        let mut settings = defaults();
        let selection = translate(SELECTION_TRANSLATE_ID);
        let original = settings.binding(&selection).unwrap();
        let destination = RouteKey::Key { code: 89 };

        settings.bind_command(selection.clone(), Some(destination));
        assert_eq!(settings.route(destination).unwrap().input, InputSource::Selection);
        assert!(settings.route(original).is_none());

        // 重复提交同一绑定不会新增路由或命令记录，也不会丢失输入来源。
        let rebound = settings.clone();
        settings.bind_command(selection.clone(), Some(destination));
        assert_eq!(settings, rebound);

        settings.bind_command(selection.clone(), Some(RouteKey::Direct));
        assert_eq!(settings.route(RouteKey::Direct).unwrap().input, InputSource::Speech);
        assert_eq!(settings.binding(&selection), Some(RouteKey::Direct));
        assert_eq!(settings.binding(&translate(SPEECH_TRANSLATE_ID)), Some(RouteKey::Direct));
        assert!(settings.route(destination).is_none());
    }

    #[test]
    fn shared_route_input_and_cleanup_follow_the_remaining_bindings() {
        let mut settings = defaults();
        let speech = translate(SPEECH_TRANSLATE_ID);
        let selection = translate(SELECTION_TRANSLATE_ID);
        let shared = RouteKey::Key { code: 77 };

        settings.bind_command(speech.clone(), Some(shared));
        settings.bind_command(selection.clone(), Some(shared));
        settings.set_route_input(shared, InputSource::Selection);
        assert_eq!(settings.route(settings.binding(&speech).unwrap()).unwrap().input, InputSource::Selection);
        assert_eq!(settings.route(settings.binding(&selection).unwrap()).unwrap().input, InputSource::Selection);

        settings.bind_command(speech.clone(), None);
        assert_eq!(settings.route(shared).unwrap().input, InputSource::Selection);
        settings.bind_command(selection, None);
        assert!(settings.route(shared).is_none());
        assert_eq!(settings.routes, vec![Route { key: RouteKey::Direct, input: InputSource::Speech }]);

        // 没有旧绑定时是首次创建路由，默认来源由核心确定为语音。
        settings.bind_command(speech.clone(), Some(shared));
        assert_eq!(settings.route(shared).unwrap().input, InputSource::Speech);
        settings.bind_command(speech, Some(RouteKey::Direct));
        let unchanged = settings.clone();
        settings.set_route_input(shared, InputSource::Selection);
        assert_eq!(settings, unchanged);
    }
}
