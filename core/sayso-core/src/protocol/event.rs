use serde::{Deserialize, Serialize};

use super::{Block, Json, LanguagePair, SCHEMA_VERSION};
use crate::store::StoreRecord;

/// 扩展事件类型。新增事件只增加 case，已发布的名称不可修改。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    /// 用户触发了该扩展的命令（定向）。
    Command,
    /// 用户点击了该扩展块上的 invoke 操作（定向）。
    Action,
    /// 某个扩展产出了翻译词对（由扩展广播）。
    Translation,
    /// 某个扩展把文字写入了前台应用（由 Sayso 广播）。
    Inserted,
}

impl EventType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Action => "action",
            Self::Translation => "translation",
            Self::Inserted => "inserted",
        }
    }
}

/// 事件在协议中的名称，供界面显示。
#[uniffi::export]
pub fn event_name(event: EventType) -> String {
    event.name().to_owned()
}

/// 命令输入的来源，由路由决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum InputSource {
    /// 按住（或按一下开始 / 结束）入口键说的一句话，识别原文可能夹杂本地语言。
    Speech,
    /// 前台应用中当前选中的文本。
    Selection,
}

/// Sayso 交给命令的统一输入：语音与选区已经变成同一种文本。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Input {
    /// 已去除首尾空白，非空。
    pub text: String,
    pub via: InputSource,
}

/// 事件相关的前台应用。`id` 为平台的应用标识（macOS 为 Bundle ID）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct AppInfo {
    pub id: Option<String>,
    pub name: Option<String>,
}

/// 文字的投递方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum Delivery {
    /// 已粘贴进前台应用的输入框。
    Inserted,
    /// 没有可写入的外部应用，结果只留在剪贴板。
    CopiedToClipboard,
}

/// 事件发生时的候选框快照。扩展输出的块都显示在这个候选框中。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceSnapshot {
    pub id: String,
    /// 此刻候选框中已有的块，id 带「来源 id/」前缀。
    pub blocks: Vec<Block>,
}

/// 分发给扩展的事件。JS 处理函数收到它的 JSON：公共字段与事件字段平铺在同一层。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub timestamp: String,
    pub languages: LanguagePair,
    /// 来自页签的操作事件没有候选框。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<SurfaceSnapshot>,
    #[serde(flatten)]
    pub payload: Payload,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Payload {
    Command {
        command: CommandRef,
        input: Input,
        /// 触发命令时的前台应用。
        source: Option<AppInfo>,
    },
    Action {
        action: ActionInvocation,
    },
    Translation(Translation),
    Inserted {
        text: String,
        destination: Option<AppInfo>,
        delivery: Delivery,
        /// 发出写入的扩展。
        by: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandRef {
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionInvocation {
    /// 被点击的块；扩展的块 id 不带前缀，与扩展输出时一致。
    pub block_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Json>,
    pub origin: ActionOrigin,
    /// 来自页签时，被点击条目对应的数据记录。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<StoreRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionOrigin {
    Candidate,
    Tab,
}

/// 翻译结果：一组「目标语言 · 本地语言」词对，与输入来源无关。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Translation {
    pub input: Input,
    pub pairs: Vec<TranslationPair>,
    /// 词对所在的目标语言整句（语音输入写入的句子）；划词没有上下文时为 null。
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub source: Option<AppInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranslationPair {
    /// 目标语言一侧。
    pub target: String,
    /// 本地语言一侧。
    pub native: String,
}

impl Event {
    pub fn new(languages: LanguagePair, surface: Option<SurfaceSnapshot>, payload: Payload) -> Self {
        Event {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: crate::time_util::now_iso(),
            languages,
            surface,
            payload,
        }
    }

    pub fn event_type(&self) -> EventType {
        match self.payload {
            Payload::Command { .. } => EventType::Command,
            Payload::Action { .. } => EventType::Action,
            Payload::Translation(_) => EventType::Translation,
            Payload::Inserted { .. } => EventType::Inserted,
        }
    }

    /// 交给 JS 的 JSON，带协议版本。
    pub fn to_json(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).expect("event is serializable");
        value["schemaVersion"] = SCHEMA_VERSION.into();
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_event_is_flat_and_round_trips() {
        let event = Event::new(
            LanguagePair::default(),
            None,
            Payload::Command {
                command: CommandRef { id: "add".into() },
                input: Input { text: "买牛奶".into(), via: InputSource::Selection },
                source: Some(AppInfo { id: Some("com.apple.Notes".into()), name: Some("Notes".into()) }),
            },
        );
        let json = event.to_json();
        assert_eq!(json["type"], "command");
        assert_eq!(json["schemaVersion"], SCHEMA_VERSION);
        assert_eq!(json["input"]["via"], "selection");
        assert_eq!(json["languages"]["native"], "zh");
        assert_eq!(serde_json::from_value::<Event>(json).unwrap(), event);
    }
}
