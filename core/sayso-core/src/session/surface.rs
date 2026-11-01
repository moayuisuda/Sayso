//! 一次候选框的内容：按来源分组的块，以及决定何时自动隐藏的停留规则。

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::protocol::{Block, SurfaceSnapshot, Tone};

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

/// 候选框中某个来源的块。
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SurfaceSection {
    pub source_id: String,
    /// 扩展名称；内置来源不显示标题。
    pub title: Option<String>,
    pub blocks: Vec<Block>,
}

/// 交给 UI 渲染的候选框。
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SurfaceView {
    pub id: String,
    /// 还没有任何块时显示的文字，例如命令正在处理的输入。
    pub placeholder: Option<String>,
    pub sections: Vec<SurfaceSection>,
    /// 仍有扩展在处理本次事件。
    pub working: bool,
}

pub(crate) struct Surface {
    pub id: String,
    placeholder: Option<String>,
    sections: Vec<SurfaceSection>,
    /// 仍在处理本次事件的扩展。
    running: HashSet<String>,
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
            running: HashSet::new(),
            last_change: Instant::now(),
            hold,
        }
    }

    /// 失败提示；`input` 为失败前已得到的输入文字（如识别中断前的实时文字），保留给用户复制。
    pub fn failure(input: Option<String>, message: String) -> Self {
        let mut surface = Surface::new(input, hold::FAILURE);
        surface.upsert(failure_block(message), APP_SOURCE, None);
        surface
    }

    pub fn upsert(&mut self, block: Block, source: &str, title: Option<String>) {
        match self.sections.iter_mut().find(|section| section.source_id == source) {
            Some(section) => match section.blocks.iter_mut().find(|existing| existing.id == block.id) {
                Some(existing) => *existing = block,
                None => section.blocks.push(block),
            },
            None => self.sections.push(SurfaceSection { source_id: source.into(), title, blocks: vec![block] }),
        }
        self.touch();
    }

    pub fn remove(&mut self, block_id: &str, source: &str) {
        if let Some(index) = self.sections.iter().position(|section| section.source_id == source) {
            self.sections[index].blocks.retain(|block| block.id != block_id);
            if self.sections[index].blocks.is_empty() {
                self.sections.remove(index);
            }
        }
        self.touch();
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
                    .map(|block| Block { id: format!("{}/{}", section.source_id, block.id), ..block.clone() })
            })
            .collect();
        SurfaceSnapshot { id: self.id.clone(), blocks }
    }

    pub fn view(&self) -> SurfaceView {
        SurfaceView {
            id: self.id.clone(),
            placeholder: self.placeholder.clone(),
            sections: self.sections.clone(),
            working: !self.running.is_empty(),
        }
    }

    fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.sections.iter().flat_map(|section| section.blocks.iter())
    }

    /// 有等待点击的按钮时停留最久，有高亮译文时次之。
    fn required_hold(&self) -> Duration {
        if self.blocks().any(Block::has_pending_invoke) {
            self.hold.max(hold::PENDING_ACTION)
        } else if self.blocks().any(Block::has_highlight) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ActionKind, BlockAction, BlockContent};

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
        let BlockContent::Status { message, .. } = &surface.view().sections[0].blocks[0].content else { panic!() };
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

        let invoke = BlockAction { id: None, label: Some("Add".into()), kind: ActionKind::Invoke { payload: None } };
        surface.upsert(status("b", vec![invoke]), "ext.one", None);
        assert!(!surface.should_dismiss(false, later(10)));
        assert!(surface.should_dismiss(false, later(16)));
    }
}
