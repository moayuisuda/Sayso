use std::collections::HashMap;

use serde::Deserialize;

use crate::error::CoreError;
use crate::protocol::{Block, Capability, Language, TranslationPair};
use crate::store::StoreWrite;

/// 插件经 `sayso.*` 输出的指令，宿主直接执行、没有回复。
#[derive(Clone, Debug, PartialEq)]
pub enum Instruction {
    /// 在本次候选框中新增块，或按 id 更新本插件已有的块。
    UpsertBlock(Block),
    RemoveBlock(String),
    Store(StoreWrite),
    /// 自动朗读；用户关闭了自动朗读时不发声。
    Speak {
        text: String,
        language: Language,
    },
    /// 广播 `translation` 事件；输入与来源由宿主从触发的命令补全。
    PublishTranslation {
        pairs: Vec<TranslationPair>,
        context: Option<String>,
    },
}

/// prelude 传来的 JSON 形式：`{"op": …, …字段}`。
#[derive(Deserialize)]
#[serde(tag = "op")]
enum Wire {
    #[serde(rename = "block.upsert")]
    BlockUpsert { block: Block },
    #[serde(rename = "block.remove")]
    BlockRemove { id: String },
    #[serde(rename = "store.put")]
    StorePut { collection: String, key: String, record: serde_json::Map<String, serde_json::Value> },
    #[serde(rename = "store.patch")]
    StorePatch {
        collection: String,
        key: String,
        #[serde(default)]
        set: serde_json::Map<String, serde_json::Value>,
        #[serde(default)]
        increment: HashMap<String, f64>,
    },
    #[serde(rename = "store.delete")]
    StoreDelete { collection: String, key: String },
    #[serde(rename = "speak")]
    Speak { text: String, language: Language },
    #[serde(rename = "publish")]
    Publish { event: PublishedEvent },
}

/// 只能广播 `translation`；定向事件与 Sayso 自己发出的 `inserted` 不能由插件广播。
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum PublishedEvent {
    Translation {
        pairs: Vec<TranslationPair>,
        #[serde(default)]
        context: Option<String>,
    },
}

impl Instruction {
    pub fn parse(json: &str) -> Result<Self, CoreError> {
        let wire: Wire =
            serde_json::from_str(json).map_err(|error| CoreError::InvalidInstruction(error.to_string()))?;
        Ok(match wire {
            Wire::BlockUpsert { block } => Self::UpsertBlock(block),
            Wire::BlockRemove { id } => Self::RemoveBlock(id),
            Wire::StorePut { collection, key, record } => Self::Store(StoreWrite::Put { collection, key, record }),
            Wire::StorePatch { collection, key, set, increment } => {
                Self::Store(StoreWrite::Patch { collection, key, set, increment })
            }
            Wire::StoreDelete { collection, key } => Self::Store(StoreWrite::Delete { collection, key }),
            Wire::Speak { text, language } => Self::Speak { text, language },
            Wire::Publish { event: PublishedEvent::Translation { pairs, context } } => {
                Self::PublishTranslation { pairs, context }
            }
        })
    }

    /// 执行该指令需要的能力，包括块上操作所需的能力。
    pub fn required_capabilities(&self) -> Vec<Capability> {
        match self {
            Self::UpsertBlock(block) => block.required_capabilities(),
            Self::RemoveBlock(_) => vec![Capability::Blocks],
            Self::Store(_) => vec![Capability::Store],
            Self::Speak { .. } => vec![],
            Self::PublishTranslation { .. } => vec![Capability::Publish],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_accepts_translation_only() {
        let publish = Instruction::parse(
            r#"{"op":"publish","event":{"type":"translation","pairs":[{"target":"an apple","native":"一个苹果"}]}}"#,
        )
        .unwrap();
        assert_eq!(publish.required_capabilities(), vec![Capability::Publish]);
        assert!(Instruction::parse(r#"{"op":"publish","event":{"type":"command","command":{"id":"x"}}}"#).is_err());
        assert!(Instruction::parse(r#"{"op":"publish","event":{"type":"inserted","text":"x"}}"#).is_err());
    }
}
