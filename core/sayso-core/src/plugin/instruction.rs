use std::collections::HashMap;

use serde::Deserialize;

use crate::error::CoreError;
use crate::protocol::{Block, BlockAction, Capability, Language, TranslationPair, SAYSO_BLOCK_PREFIX};
use crate::store::StoreWrite;

/// 插件经 `sayso.*` 输出的指令，宿主直接执行、没有回复。
#[derive(Clone, Debug, PartialEq)]
pub enum Instruction {
    /// 在本次候选框中新增块，或按 id 更新本插件已有的块。
    UpsertBlock(Block),
    RemoveBlock(String),
    /// 在本次候选框中其他来源的块（如词条卡片）上附加本插件的按钮，替换之前附加的那组；为空时撤下。
    AttachActions {
        /// 块引用：`来源 id/块 id`。
        block: String,
        actions: Vec<BlockAction>,
    },
    Store(StoreWrite),
    /// 自动朗读；用户关闭了自动朗读时不发声。
    Speak {
        text: String,
        language: Language,
    },
    /// 广播 `translated` hook；输入与来源由宿主从触发的命令补全。
    PublishTranslated {
        output: String,
        pairs: Vec<TranslationPair>,
        context: Option<String>,
        /// 广播者展示整次翻译的块 id（本插件的块，不带前缀）。
        card: String,
    },
    /// 替换提供给语音识别与翻译模型的背景。
    SetContext(String),
}

/// prelude 传来的 JSON 形式：`{"op": …, …字段}`。
#[derive(Deserialize)]
#[serde(tag = "op")]
enum Wire {
    #[serde(rename = "block.upsert")]
    BlockUpsert { block: Block },
    #[serde(rename = "block.remove")]
    BlockRemove { id: String },
    #[serde(rename = "block.attach")]
    BlockAttach { block: String, actions: Vec<BlockAction> },
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
    #[serde(rename = "context.set")]
    ContextSet { text: String },
}

/// 只能广播 `translated`；定向事件与 Sayso 自己发出的 `input`、`inserted` 不能由插件广播。
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum PublishedEvent {
    Translated {
        output: String,
        pairs: Vec<TranslationPair>,
        #[serde(default)]
        context: Option<String>,
        card: String,
    },
}

/// 扩展只能新增、移除自己的块：Sayso 代画的块（id 带 `SAYSO_BLOCK_PREFIX`）只能附加按钮。
fn own_block(id: &str) -> Result<(), CoreError> {
    if id.starts_with(SAYSO_BLOCK_PREFIX) {
        return Err(CoreError::InvalidInstruction(format!("block id {id} is reserved for Sayso")));
    }
    Ok(())
}

impl Instruction {
    pub fn parse(json: &str) -> Result<Self, CoreError> {
        let wire: Wire =
            serde_json::from_str(json).map_err(|error| CoreError::InvalidInstruction(error.to_string()))?;
        Ok(match wire {
            Wire::BlockUpsert { block } => Self::UpsertBlock(own_block(&block.id).map(|()| block)?),
            Wire::BlockRemove { id } => Self::RemoveBlock(own_block(&id).map(|()| id)?),
            Wire::BlockAttach { block, actions } => Self::AttachActions { block, actions },
            Wire::StorePut { collection, key, record } => Self::Store(StoreWrite::Put { collection, key, record }),
            Wire::StorePatch { collection, key, set, increment } => {
                Self::Store(StoreWrite::Patch { collection, key, set, increment })
            }
            Wire::StoreDelete { collection, key } => Self::Store(StoreWrite::Delete { collection, key }),
            Wire::Speak { text, language } => Self::Speak { text, language },
            Wire::Publish { event: PublishedEvent::Translated { output, pairs, context, card } } => {
                Self::PublishTranslated { output, pairs, context, card }
            }
            Wire::ContextSet { text } => Self::SetContext(text),
        })
    }

    /// 执行该指令需要的能力，包括块上操作所需的能力。
    pub fn required_capabilities(&self) -> Vec<Capability> {
        match self {
            Self::UpsertBlock(block) => block.required_capabilities(),
            Self::RemoveBlock(_) => vec![Capability::Blocks],
            Self::AttachActions { actions, .. } => BlockAction::capabilities_for(actions),
            Self::Store(_) => vec![Capability::Store],
            Self::Speak { .. } => vec![],
            Self::PublishTranslated { .. } => vec![Capability::Publish],
            Self::SetContext(_) => vec![Capability::Context],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_accepts_translated_only() {
        let publish = Instruction::parse(
            r#"{"op":"publish","event":{"type":"translated","output":"I have an apple.","pairs":[{"target":"an apple","native":"一个苹果"}],"card":"text"}}"#,
        )
        .unwrap();
        assert_eq!(publish.required_capabilities(), vec![Capability::Publish]);
        assert!(Instruction::parse(r#"{"op":"publish","event":{"type":"command","command":{"id":"x"}}}"#).is_err());
        assert!(Instruction::parse(r#"{"op":"publish","event":{"type":"inserted","text":"x"}}"#).is_err());
        assert!(Instruction::parse(r#"{"op":"publish","event":{"type":"input","input":{"text":"x","via":"speech"}}}"#)
            .is_err());
    }

    #[test]
    fn extensions_cannot_write_blocks_sayso_draws() {
        assert!(Instruction::parse(r#"{"op":"block.remove","id":"@term-0"}"#).is_err());
        assert!(Instruction::parse(r#"{"op":"block.remove","id":"term-0"}"#).is_ok());
    }
}
