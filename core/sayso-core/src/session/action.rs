//! 执行块上的操作。候选框与扩展页签共用。

use std::sync::Arc;

use super::APP_SOURCE;
use crate::app::Inner;
use crate::extensions::Route;
use crate::protocol::{ActionInvocation, ActionKind, ActionOrigin, BlockAction, Payload};
use crate::store::StoreRecord;

/// 操作发生的位置。
pub(crate) enum ActionContext {
    Candidate {
        surface_id: String,
    },
    /// 来自页签时，带上条目对应的数据记录。
    Tab {
        record: StoreRecord,
    },
}

impl Inner {
    /// `source` 为块的来源（内置来源或扩展 id）。
    pub(crate) fn perform_action(
        self: &Arc<Self>,
        source: &str,
        block_id: String,
        action: BlockAction,
        context: ActionContext,
    ) {
        // 内置来源拥有全部能力；扩展只能使用声明过的能力。
        if let Some(required) = action.required_capability() {
            let granted = source == APP_SOURCE
                || self.extensions.read().unwrap().get(source).is_some_and(|plugin| plugin.manifest.grants(required));
            if !granted {
                return;
            }
        }
        let surface_id = match &context {
            ActionContext::Candidate { surface_id } => Some(surface_id.clone()),
            ActionContext::Tab { .. } => None,
        };
        match action.kind {
            ActionKind::Speak { text, language } => {
                self.platform.speak(text, language, self.settings().speech_rate);
            }
            ActionKind::Copy { text } => self.platform.copy_text(text),
            ActionKind::Open { url } => self.platform.open_url(url),
            ActionKind::Insert { text } => {
                // 候选框不会成为前台窗口，点击时前台应用仍是用户正在输入的应用。
                let destination = self.platform.frontmost_app();
                let (inner, by) = (self.clone(), source.to_string());
                self.runtime.spawn(async move { inner.insert(text, destination, by, surface_id).await });
            }
            ActionKind::Invoke { payload } => {
                let (origin, record) = match context {
                    ActionContext::Candidate { .. } => (ActionOrigin::Candidate, None),
                    ActionContext::Tab { record } => (ActionOrigin::Tab, Some(record)),
                };
                let payload = Payload::Action { action: ActionInvocation { block_id, payload, origin, record } };
                let route = Route::Target { extension_id: source.into(), reports_failure: false };
                self.dispatch(payload, surface_id.as_deref(), route);
            }
            // 删除需要二次确认并展示失败原因，由页签确认后调用 `Core::delete_record`。
            ActionKind::Delete => {}
        }
    }
}
