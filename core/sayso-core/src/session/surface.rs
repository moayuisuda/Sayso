//! 一次候选框的内容：按来源分组的块、其他扩展附加在块上的按钮，以及决定何时自动隐藏的停留规则。

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::CoreError;
use crate::protocol::{
    Block, BlockAction, BlockContent, Language, SurfaceSnapshot, Tone, TranslationPair, SAYSO_BLOCK_PREFIX,
};

/// Sayso 自身的状态提示（错误、扩展失败等）所用的块来源。
pub const APP_SOURCE: &str = "sayso.app";

/// 候选框展示时长：内容越多停留越久；鼠标悬停或扩展仍在处理时不隐藏。
pub mod hold {
    use std::time::Duration;

    pub const SPEECH: Duration = Duration::from_millis(1500);
    pub const FAILURE: Duration = Duration::from_secs(3);
    /// 有高亮翻译时给用户时间看清译文。
    pub const TRANSLATED: Duration = Duration::from_secs(5);
    pub const SELECTION: Duration = Duration::from_secs(8);
    /// 有等待点击的扩展按钮（如加入生词本）时给用户足够时间决定。
    pub const PENDING_ACTION: Duration = Duration::from_secs(15);
}

/// 其他扩展附加在块上的按钮，点击后回调给附加方而不是块的来源。
#[derive(Clone, Debug, PartialEq, Serialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// 附加按钮的扩展。
    pub source_id: String,
    /// 被附加的块的引用（`来源 id/块 id`），点击时作为 `blockId` 交给附加方。
    pub block_ref: String,
    pub actions: Vec<BlockAction>,
}

/// 候选框中的一个块，连同其他扩展附加在它上面的按钮。
#[derive(Clone, Debug, PartialEq, Serialize, uniffi::Record)]
pub struct SurfaceBlock {
    pub block: Block,
    /// 按附加先后排列，每个扩展至多一组。
    pub attachments: Vec<Attachment>,
}

impl SurfaceBlock {
    fn has_pending_invoke(&self) -> bool {
        self.block.has_pending_invoke()
            || self.attachments.iter().any(|attachment| BlockAction::any_pending_invoke(&attachment.actions))
    }
}

/// 候选框中某个来源的块。
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SurfaceSection {
    pub source_id: String,
    /// 扩展名称；内置来源不显示标题。
    pub title: Option<String>,
    pub blocks: Vec<SurfaceBlock>,
}

/// 交给 UI 渲染的候选框。
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SurfaceView {
    pub id: String,
    /// 还没有任何块时显示的文字，例如命令正在处理的输入。
    pub placeholder: Option<String>,
    pub sections: Vec<SurfaceSection>,
    /// 附加在整句卡片上、针对整次翻译的按钮，显示在底部操作栏，不随整句卡片显示。
    pub footer: Vec<Attachment>,
    /// 仍有扩展在处理本次事件。
    pub working: bool,
}

pub(crate) struct Surface {
    pub id: String,
    placeholder: Option<String>,
    sections: Vec<SurfaceSection>,
    /// 整句卡片的引用（`来源 id/块 id`）：附加在它们上面的按钮显示在底部操作栏。
    translation_cards: Vec<String>,
    /// 仍在处理本次事件的扩展。
    running: HashSet<String>,
    /// 发送端只由本候选框持有，关闭或替换候选框后，shell 的接收端立即得到关闭通知。
    shell_lifetime: tokio::sync::watch::Sender<()>,
    /// 最近一次内容变化的时间，用于决定何时自动隐藏。
    last_change: Instant,
    /// 本次流程的基础停留时长。
    hold: Duration,
}

impl Surface {
    pub fn new(placeholder: Option<String>, hold: Duration) -> Self {
        Surface {
            id: uuid::Uuid::new_v4().to_string(),
            placeholder,
            sections: Vec::new(),
            translation_cards: Vec::new(),
            running: HashSet::new(),
            shell_lifetime: tokio::sync::watch::channel(()).0,
            last_change: Instant::now(),
            hold,
        }
    }

    pub fn shell_lifetime(&self) -> tokio::sync::watch::Receiver<()> {
        self.shell_lifetime.subscribe()
    }

    /// 失败提示；`input` 为失败前已得到的输入文字（如识别中断前的实时文字），保留给用户复制。
    pub fn failure(input: Option<String>, message: String) -> Self {
        let mut surface = Surface::new(input, hold::FAILURE);
        surface.upsert(failure_block(message), APP_SOURCE, None);
        surface
    }

    /// 新增块，或按 id 替换该来源已有的块；替换时保留其他扩展附加的按钮。
    pub fn upsert(&mut self, block: Block, source: &str, title: Option<String>) {
        match self.sections.iter_mut().find(|section| section.source_id == source) {
            Some(section) => match section.blocks.iter_mut().find(|existing| existing.block.id == block.id) {
                Some(existing) => existing.block = block,
                None => section.blocks.push(SurfaceBlock { block, attachments: vec![] }),
            },
            None => self.sections.push(SurfaceSection {
                source_id: source.into(),
                title,
                blocks: vec![SurfaceBlock { block, attachments: vec![] }],
            }),
        }
        self.touch();
    }

    /// 移除块，附加在它上面的按钮随之移除。
    pub fn remove(&mut self, block_id: &str, source: &str) {
        if let Some(index) = self.sections.iter().position(|section| section.source_id == source) {
            self.sections[index].blocks.retain(|item| item.block.id != block_id);
            if self.sections[index].blocks.is_empty() {
                self.sections.remove(index);
            }
        }
        self.touch();
    }

    /// 把来源 `source` 的块 `block_id` 登记为整句卡片，返回它的引用；块不在本候选框中时报错。
    pub fn mark_translation_card(&mut self, source: &str, block_id: &str) -> Result<String, CoreError> {
        let exists = self
            .sections
            .iter()
            .any(|section| section.source_id == source && section.blocks.iter().any(|item| item.block.id == block_id));
        if !exists {
            return Err(CoreError::InvalidInstruction(format!("no block {block_id} in this candidate box")));
        }
        let reference = block_ref(source, block_id);
        if !self.translation_cards.contains(&reference) {
            self.translation_cards.push(reference.clone());
        }
        Ok(reference)
    }

    /// 把 `source` 的按钮附加到 `block_ref` 指向的块上，替换它之前附加的那组；`actions` 为空时撤下。
    pub fn attach(&mut self, block_ref: &str, source: &str, actions: Vec<BlockAction>) -> Result<(), CoreError> {
        let not_found = || CoreError::InvalidInstruction(format!("no block {block_ref} in this candidate box"));
        let (owner, block_id) = block_ref.split_once('/').ok_or_else(not_found)?;
        let item = self
            .sections
            .iter_mut()
            .filter(|section| section.source_id == owner)
            .flat_map(|section| section.blocks.iter_mut())
            .find(|item| item.block.id == block_id)
            .ok_or_else(not_found)?;
        let position = item.attachments.iter().position(|attachment| attachment.source_id == source);
        match (position, actions.is_empty()) {
            (Some(index), true) => {
                item.attachments.remove(index);
            }
            (Some(index), false) => item.attachments[index].actions = actions,
            (None, false) => {
                item.attachments.push(Attachment { source_id: source.into(), block_ref: block_ref.into(), actions })
            }
            (None, true) => {}
        }
        self.touch();
        Ok(())
    }

    /// 为翻译出的词对画词条卡片，归发布翻译的扩展所有。同一词条（忽略大小写与首尾标点）只画一张，
    /// 按顺序返回每个词对对应卡片的引用，供订阅者附加按钮。
    pub fn present_pairs(
        &mut self,
        pairs: &[TranslationPair],
        language: Language,
        source: &str,
        title: Option<String>,
    ) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        pairs
            .iter()
            .map(|pair| {
                let key = term_key(&pair.target);
                let index = keys.iter().position(|existing| *existing == key).unwrap_or_else(|| {
                    keys.push(key);
                    let card = Block {
                        id: format!("{SAYSO_BLOCK_PREFIX}term-{}", keys.len() - 1),
                        content: BlockContent::Term {
                            term: pair.target.clone(),
                            gloss: Some(pair.native.clone()),
                            language,
                            detail: None,
                        },
                        actions: vec![],
                    };
                    self.upsert(card, source, title.clone());
                    keys.len() - 1
                });
                block_ref(source, &format!("{SAYSO_BLOCK_PREFIX}term-{index}"))
            })
            .collect()
    }

    pub fn mark_running(&mut self, extension_ids: impl IntoIterator<Item = String>) {
        self.running.extend(extension_ids);
    }

    pub fn mark_finished(&mut self, extension_id: &str) {
        self.running.remove(extension_id);
        self.touch();
    }

    /// 重置自动隐藏计时，例如鼠标离开候选框时。
    pub fn touch(&mut self) {
        self.last_change = Instant::now();
    }

    /// 带来源前缀的全部块，作为事件中的 `surface.blocks`。
    pub fn snapshot(&self) -> SurfaceSnapshot {
        let blocks = self
            .sections
            .iter()
            .flat_map(|section| {
                section
                    .blocks
                    .iter()
                    .map(|item| Block { id: block_ref(&section.source_id, &item.block.id), ..item.block.clone() })
            })
            .collect();
        SurfaceSnapshot { id: self.id.clone(), blocks }
    }

    /// 整句卡片上附加的按钮移到底部操作栏，其余按钮随所在的块显示。
    pub fn view(&self) -> SurfaceView {
        let mut sections = self.sections.clone();
        let mut footer = Vec::new();
        for section in &mut sections {
            for item in &mut section.blocks {
                if self.translation_cards.contains(&block_ref(&section.source_id, &item.block.id)) {
                    footer.append(&mut item.attachments);
                }
            }
        }
        SurfaceView {
            id: self.id.clone(),
            placeholder: self.placeholder.clone(),
            sections,
            footer,
            working: !self.running.is_empty(),
        }
    }

    fn blocks(&self) -> impl Iterator<Item = &SurfaceBlock> {
        self.sections.iter().flat_map(|section| section.blocks.iter())
    }

    /// 有等待点击的按钮（含附加的按钮）时停留最久，有高亮译文时次之。
    fn required_hold(&self) -> Duration {
        if self.blocks().any(SurfaceBlock::has_pending_invoke) {
            self.hold.max(hold::PENDING_ACTION)
        } else if self.blocks().any(|item| item.block.has_highlight()) {
            self.hold.max(hold::TRANSLATED)
        } else {
            self.hold
        }
    }

    /// 没有悬停、没有扩展在处理，且内容静止超过停留时长时自动隐藏。
    pub fn should_dismiss(&self, hovering: bool, now: Instant) -> bool {
        !hovering && self.running.is_empty() && now.duration_since(self.last_change) >= self.required_hold()
    }
}

pub(crate) fn failure_block(message: String) -> Block {
    Block::status("status", message, Tone::Error, "exclamationmark.triangle.fill")
}

/// 跨来源引用块的形式：`来源 id/块 id`。扩展 id 不含 `/`，以第一个 `/` 分隔。
fn block_ref(source: &str, block_id: &str) -> String {
    format!("{source}/{block_id}")
}

/// 词条去重键：忽略大小写与首尾的空白、标点。
fn term_key(term: &str) -> String {
    term.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ActionKind;

    fn status(id: &str, actions: Vec<BlockAction>) -> Block {
        Block { actions, ..Block::status(id, "x", Tone::Info, "") }
    }

    #[test]
    fn upsert_replaces_by_id_within_source_and_snapshot_prefixes_ids() {
        let mut surface = Surface::new(None, hold::SPEECH);
        surface.upsert(status("a", vec![]), "ext.one", Some("One".into()));
        surface.upsert(status("a", vec![]), "ext.two", None);
        surface.upsert(Block::status("a", "updated", Tone::Info, ""), "ext.one", Some("One".into()));

        let ids: Vec<_> = surface.snapshot().blocks.into_iter().map(|block| block.id).collect();
        assert_eq!(ids, ["ext.one/a", "ext.two/a"]);
        let BlockContent::Status { message, .. } = &surface.view().sections[0].blocks[0].block.content else {
            panic!()
        };
        assert_eq!(message, "updated");

        surface.remove("a", "ext.two");
        assert_eq!(surface.view().sections.len(), 1);
    }

    /// 停留时长随内容变化：等待点击的按钮停留最久；扩展处理中或悬停时不隐藏。
    #[test]
    fn hold_extends_for_pending_actions_and_waits_for_running_extensions() {
        let mut surface = Surface::new(None, hold::SPEECH);
        let later = |seconds: u64| Instant::now() + Duration::from_secs(seconds);
        assert!(surface.should_dismiss(false, later(2)));
        assert!(!surface.should_dismiss(true, later(2)));

        surface.mark_running(["ext.one".to_string()]);
        assert!(!surface.should_dismiss(false, later(60)));
        surface.mark_finished("ext.one");

        surface.upsert(status("b", vec![invoke("Add")]), "ext.one", None);
        assert!(!surface.should_dismiss(false, later(10)));
        assert!(surface.should_dismiss(false, later(16)));
    }

    fn invoke(label: &str) -> BlockAction {
        BlockAction { id: None, label: Some(label.into()), disabled: false, kind: ActionKind::Invoke { payload: None } }
    }

    /// 同一词条只画一张卡片；其他扩展附加的按钮按扩展替换、可撤下，卡片更新时保留，卡片移除时随之移除。
    #[test]
    fn pair_cards_dedupe_terms_and_keep_attachments_per_extension() {
        let mut surface = Surface::new(None, hold::SPEECH);
        let pair =
            |target: &str, native: &str| TranslationPair { target: target.into(), native: native.into(), card: None };
        let pairs = [pair("an apple", "一个苹果"), pair("bananas", "香蕉"), pair("An apple.", "一个苹果")];
        let cards = surface.present_pairs(&pairs, Language::English, "sayso.dictation", None);
        assert_eq!(cards, ["sayso.dictation/@term-0", "sayso.dictation/@term-1", "sayso.dictation/@term-0"]);
        assert_eq!(surface.view().sections[0].blocks.len(), 2);

        surface.attach(&cards[0], "sayso.wordbook", vec![invoke("Add")]).unwrap();
        surface.attach(&cards[0], "ext.notes", vec![invoke("Note")]).unwrap();
        surface.attach(&cards[0], "sayso.wordbook", vec![BlockAction { disabled: true, ..invoke("Added") }]).unwrap();
        surface.upsert(status("@term-0", vec![]), "sayso.dictation", None);
        let attachments = &surface.view().sections[0].blocks[0].attachments;
        let labels: Vec<_> =
            attachments.iter().map(|a| (a.source_id.as_str(), a.actions[0].label.as_deref())).collect();
        assert_eq!(labels, [("sayso.wordbook", Some("Added")), ("ext.notes", Some("Note"))]);
        assert_eq!(attachments[0].block_ref, "sayso.dictation/@term-0");

        surface.attach(&cards[0], "ext.notes", vec![]).unwrap();
        assert_eq!(surface.view().sections[0].blocks[0].attachments.len(), 1);
        assert!(surface.attach("sayso.dictation/missing", "ext.notes", vec![invoke("Note")]).is_err());
        surface.remove("@term-0", "sayso.dictation");
        assert!(surface.attach(&cards[0], "ext.notes", vec![invoke("Note")]).is_err());
    }

    /// 整句卡片上附加的按钮显示在底部操作栏，词条卡片上的仍随卡片显示。
    #[test]
    fn translation_card_attachments_move_to_footer() {
        let mut surface = Surface::new(None, hold::SPEECH);
        surface.upsert(status("text", vec![]), "sayso.dictation", None);
        let pairs = [TranslationPair { target: "an apple".into(), native: "一个苹果".into(), card: None }];
        let word = surface.present_pairs(&pairs, Language::English, "sayso.dictation", None).remove(0);
        let whole = surface.mark_translation_card("sayso.dictation", "text").unwrap();
        assert!(surface.mark_translation_card("sayso.dictation", "missing").is_err());
        surface.attach(&whole, "ext.notes", vec![invoke("Note all")]).unwrap();
        surface.attach(&word, "ext.notes", vec![invoke("Note")]).unwrap();

        let view = surface.view();
        assert_eq!(view.footer.len(), 1);
        assert_eq!(
            (view.footer[0].block_ref.as_str(), view.footer[0].source_id.as_str()),
            (whole.as_str(), "ext.notes")
        );
        let attached: Vec<_> = view.sections[0].blocks.iter().map(|item| item.attachments.len()).collect();
        assert_eq!(attached, [0, 1]);
    }

    /// 附加的等待点击按钮同样延长停留；已完成（不可点击）的按钮不算。
    #[test]
    fn attached_invoke_extends_hold_unless_disabled() {
        let mut surface = Surface::new(None, hold::SPEECH);
        let later = |seconds: u64| Instant::now() + Duration::from_secs(seconds);
        surface.upsert(status("a", vec![]), "ext.one", None);
        surface.attach("ext.one/a", "ext.two", vec![invoke("Add")]).unwrap();
        assert!(!surface.should_dismiss(false, later(10)));
        surface.attach("ext.one/a", "ext.two", vec![BlockAction { disabled: true, ..invoke("Added") }]).unwrap();
        assert!(surface.should_dismiss(false, later(10)));
    }
}
