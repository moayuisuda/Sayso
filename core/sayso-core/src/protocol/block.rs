use serde::{Deserialize, Deserializer, Serialize};

use super::{Capability, Json, Language};

/// 候选框与扩展页签中的最小界面单元。类型来自固定组件库，由平台 UI 统一渲染。
///
/// JSON 为扁平结构：`{"id","type", …类型字段, "actions":[…]}`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Block {
    pub id: String,
    #[serde(flatten)]
    pub content: BlockContent,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<BlockAction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Enum)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BlockContent {
    /// 一段可换行的富文本，片段可高亮、可点击。
    Text {
        runs: Vec<TextRun>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<TextRole>,
    },
    /// 词条：目标语言词句 · 释义，自带朗读按钮。
    Term {
        term: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gloss: Option<String>,
        /// 词条本身的语言，决定朗读语音。
        language: Language,
        /// 小字补充信息，如音标、词性、例句。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Card {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    Status {
        message: String,
        /// 图标名，平台各自映射到系统图标（macOS 为 SF Symbol）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<Tone>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum TextRole {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum TextStyle {
    Normal,
    Highlight,
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct TextRun {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<TextStyle>,
    /// 引用所在块 `actions` 中某个操作的 id，点击该片段即执行。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

/// 块上的可点击操作。JSON 为扁平结构：`{"type", "id"?, "label"?, …类型字段}`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct BlockAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub kind: ActionKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Enum)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ActionKind {
    Speak {
        text: String,
        language: Language,
    },
    Copy {
        text: String,
    },
    /// 只允许 http(s)，防止扩展借此启动本地程序或其他 URL Scheme。
    Open {
        #[serde(deserialize_with = "web_url")]
        url: String,
    },
    Insert {
        text: String,
    },
    Invoke {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload: Option<Json>,
    },
    Delete,
}

impl BlockAction {
    /// 使用该操作需要扩展声明的能力；None 表示无需额外能力。
    pub fn required_capability(&self) -> Option<Capability> {
        match self.kind {
            ActionKind::Insert { .. } => Some(Capability::Insert),
            ActionKind::Delete => Some(Capability::Store),
            ActionKind::Speak { .. }
            | ActionKind::Copy { .. }
            | ActionKind::Open { .. }
            | ActionKind::Invoke { .. } => None,
        }
    }
}

impl Block {
    pub fn status(id: &str, message: impl Into<String>, tone: Tone, icon: &str) -> Self {
        Block {
            id: id.into(),
            content: BlockContent::Status { message: message.into(), icon: Some(icon.into()), tone: Some(tone) },
            actions: vec![],
        }
    }

    /// 输出该块需要扩展声明的能力，包括块上操作所需的能力。
    pub fn required_capabilities(&self) -> Vec<Capability> {
        let mut capabilities = vec![Capability::Blocks];
        capabilities.extend(self.actions.iter().filter_map(BlockAction::required_capability));
        capabilities
    }

    pub fn has_pending_invoke(&self) -> bool {
        self.actions.iter().any(|action| matches!(action.kind, ActionKind::Invoke { .. }))
    }

    pub fn has_highlight(&self) -> bool {
        matches!(&self.content, BlockContent::Text { runs, .. } if runs.iter().any(|run| run.style == Some(TextStyle::Highlight)))
    }
}

/// 模板替换后的 URL 可能含空格、中文等字符，按 URL 规则编码非法字符后校验协议。
fn web_url<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let raw = String::deserialize(deserializer)?;
    let url = url::Url::parse(&raw).map_err(serde::de::Error::custom)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(serde::de::Error::custom("only http(s) URLs are allowed"));
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_flat_block_with_actions() {
        let block: Block = serde_json::from_value(json!({
            "id": "dict", "type": "card", "title": "serendipity", "subtitle": "n.",
            "actions": [{"type": "speak", "text": "serendipity", "language": "en"}, {"type": "insert", "text": "serendipity"}]
        }))
        .unwrap();
        assert_eq!(
            block.content,
            BlockContent::Card { title: "serendipity".into(), subtitle: Some("n.".into()), body: None }
        );
        assert_eq!(
            block.actions[0].kind,
            ActionKind::Speak { text: "serendipity".into(), language: Language::English }
        );
        assert_eq!(block.required_capabilities(), vec![Capability::Blocks, Capability::Insert]);
        // 线上格式往返不变。
        assert_eq!(serde_json::from_value::<Block>(serde_json::to_value(&block).unwrap()).unwrap(), block);
    }

    #[test]
    fn open_action_rejects_non_web_url_and_encodes_spaces() {
        let file = json!({"id": "x", "type": "status", "message": "hi", "actions": [{"type": "open", "url": "file:///etc/passwd"}]});
        assert!(serde_json::from_value::<Block>(file).is_err());
        let web: BlockAction =
            serde_json::from_value(json!({"type": "open", "url": "https://example.com/q?w=a b"})).unwrap();
        assert_eq!(web.kind, ActionKind::Open { url: "https://example.com/q?w=a%20b".into() });
    }
}
