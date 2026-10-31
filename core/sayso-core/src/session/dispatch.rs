//! 把事件分发给扩展，并执行扩展在处理中输出的指令与请求。

use std::sync::Arc;

use async_trait::async_trait;

use super::{failure_block, Phase, Surface, APP_SOURCE};
use crate::app::Inner;
use crate::error::CoreError;
use crate::extensions::Route;
use crate::l10n::tr;
use crate::plugin::{run, Host, Instruction, RunResult, RunStatus};
use crate::protocol::{AppInfo, Delivery, Event, Input, Payload, Translation};
use crate::store::StoreRecord;

impl Inner {
    /// 把事件异步分发给投递范围内已启用的订阅者，不阻塞调用方。`surface_id` 为事件所在的候选框。
    pub(crate) fn dispatch(self: &Arc<Self>, payload: Payload, surface_id: Option<&str>, route: Route) {
        let settings = self.settings();
        let mut event = Event::new(settings.languages(), None, payload);
        let recipients = self.extensions.read().unwrap().recipients(&settings, event.event_type(), &route);
        if recipients.is_empty() {
            return;
        }
        // 记录正在处理的扩展：它们处理完之前候选框不会自动隐藏。
        let ids: Vec<String> = recipients.iter().map(|plugin| plugin.id().to_string()).collect();
        event.surface = surface_id.and_then(|id| {
            self.update_surface(id, |surface| {
                surface.mark_running(ids);
                surface.snapshot()
            })
        });
        let surface_id = event.surface.as_ref().map(|surface| surface.id.clone());
        let command = match &event.payload {
            Payload::Command { input, source, .. } => {
                Some(CommandContext { input: input.clone(), source: source.clone() })
            }
            _ => None,
        };

        for plugin in recipients {
            let manifest = &plugin.manifest;
            let host = Arc::new(DispatchHost {
                inner: self.clone(),
                extension_id: manifest.id.clone(),
                // 内置扩展的内容视同应用自身，不显示扩展名称。
                title: (!manifest.is_built_in()).then(|| manifest.name.clone()),
                surface_id: surface_id.clone(),
                command: command.clone(),
            });
            let (inner, event, route, surface_id) = (self.clone(), event.clone(), route.clone(), surface_id.clone());
            self.runtime.spawn(async move {
                let result = run(&plugin, &event, host).await;
                inner.finish_run(result, surface_id.as_deref(), &route);
            });
        }
    }

    /// 修改指定候选框；它已不在展示时返回 None。
    pub(crate) fn update_surface<R>(&self, id: &str, change: impl FnOnce(&mut Surface) -> R) -> Option<R> {
        self.update_session(|session| match &mut session.phase {
            Phase::Presenting(surface) if surface.id == id => Some(change(surface)),
            _ => None,
        })
    }

    /// 写入前台应用，并向其他扩展广播 `inserted`。
    pub(crate) async fn insert(
        self: &Arc<Self>,
        text: String,
        destination: Option<AppInfo>,
        by: String,
        surface_id: Option<String>,
    ) -> Result<Delivery, CoreError> {
        let delivery = self.platform.insert_text(text.clone(), destination.clone()).await?;
        let payload = Payload::Inserted { text, destination, delivery, by: by.clone() };
        self.dispatch(payload, surface_id.as_deref(), Route::Republished { by });
        Ok(delivery)
    }

    /// 记录执行结果；命令失败时把原因显示在候选框中。
    fn finish_run(&self, result: RunResult, surface_id: Option<&str>, route: &Route) {
        if result.status != RunStatus::Succeeded || !result.warnings.is_empty() {
            log::error!("extension {} {:?} {:?}", result.extension_id, result.status, result.warnings);
        }
        let failure = match route {
            Route::Target { reports_failure: true, .. } => self.failure_message(&result),
            _ => None,
        };
        if let Some(id) = surface_id {
            self.update_surface(id, |surface| {
                if let Some(message) = failure {
                    surface.upsert(failure_block(message), APP_SOURCE, None);
                }
                surface.mark_finished(&result.extension_id);
            });
        }
        self.extensions.write().unwrap().record_result(result);
        self.observer.extensions_changed();
    }

    fn failure_message(&self, result: &RunResult) -> Option<String> {
        let reason = match &result.status {
            RunStatus::Succeeded => return None,
            RunStatus::TimedOut => tr("超时", "timed out"),
            RunStatus::Failed { message } => message.clone(),
        };
        let extensions = self.extensions.read().unwrap();
        let name = extensions.get(&result.extension_id).map_or(result.extension_id.as_str(), |p| &p.manifest.name);
        Some(tr(&format!("「{name}」失败：{reason}"), &format!("“{name}” failed: {reason}")))
    }
}

/// 触发命令的输入与前台应用：`publish` 的翻译事件与 `insert` 的写入目标都来自这里。
#[derive(Clone)]
struct CommandContext {
    input: Input,
    source: Option<AppInfo>,
}

/// 一个扩展处理一次事件时的宿主。能力校验已由插件运行时完成。
struct DispatchHost {
    inner: Arc<Inner>,
    extension_id: String,
    title: Option<String>,
    surface_id: Option<String>,
    command: Option<CommandContext>,
}

#[async_trait]
impl Host for DispatchHost {
    fn apply(&self, instruction: Instruction) -> Result<(), CoreError> {
        match instruction {
            Instruction::UpsertBlock(block) => {
                let id = self
                    .surface_id
                    .as_deref()
                    .ok_or_else(|| CoreError::InvalidInstruction("no candidate box for this event".into()))?;
                self.inner.update_surface(id, |surface| surface.upsert(block, &self.extension_id, self.title.clone()));
            }
            Instruction::RemoveBlock(block_id) => {
                if let Some(id) = &self.surface_id {
                    self.inner.update_surface(id, |surface| surface.remove(&block_id, &self.extension_id));
                }
            }
            Instruction::Store(write) => {
                self.inner.stores.apply(&self.extension_id, write)?;
                self.inner.observer.store_changed(self.extension_id.clone());
            }
            Instruction::Speak { text, language } => {
                let settings = self.inner.settings();
                if settings.auto_speak {
                    self.inner.speak(text, language, self.surface_id.as_deref());
                }
            }
            Instruction::PublishTranslation { pairs, context } => {
                let command = self
                    .command
                    .as_ref()
                    .ok_or_else(|| CoreError::InvalidInstruction("publish needs a command event".into()))?;
                let translation =
                    Translation { input: command.input.clone(), pairs, context, source: command.source.clone() };
                let route = Route::Republished { by: self.extension_id.clone() };
                self.inner.dispatch(Payload::Translation(translation), self.surface_id.as_deref(), route);
            }
        }
        Ok(())
    }

    async fn complete(&self, system: String, user: String) -> Result<String, CoreError> {
        let endpoint = self.inner.llm_endpoint().ok_or(CoreError::ModelNotConfigured)?;
        endpoint.complete(&self.inner.http, &system, &user).await
    }

    /// 写入触发命令时的前台应用。
    async fn insert(&self, text: String) -> Result<Delivery, CoreError> {
        let command = self.command.as_ref().ok_or(CoreError::InsertOutsideCommand)?;
        let by = self.extension_id.clone();
        self.inner.insert(text, command.source.clone(), by, self.surface_id.clone()).await
    }

    fn records(&self, collection: &str) -> Result<Vec<StoreRecord>, CoreError> {
        self.inner.stores.records(&self.extension_id, collection)
    }
}
