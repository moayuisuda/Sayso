//! 把事件分发给扩展，并执行扩展在处理中输出的指令与请求。

use std::sync::Arc;

use async_trait::async_trait;

use super::{failure_block, Phase, Surface, APP_SOURCE};
use crate::app::Inner;
use crate::error::CoreError;
use crate::extensions::Route;
use crate::l10n::tr;
use crate::llm::with_user_context;
use crate::plugin::{run, Host, Instruction, Plugin, RunResult, RunStatus};
use crate::protocol::{AppInfo, Delivery, Event, EventType, Input, Payload, Translation};
use crate::store::StoreRecord;

pub(crate) struct PreparedDispatch {
    event: Event,
    recipients: Vec<Plugin>,
    shell_lifetime: Option<tokio::sync::watch::Receiver<()>>,
}

impl PreparedDispatch {
    pub fn attach_surface(&mut self, surface: &mut Surface) {
        surface.mark_running(self.recipients.len());
        self.event.surface = Some(surface.snapshot());
        self.shell_lifetime = Some(surface.shell_lifetime());
    }
}

impl Inner {
    /// 把事件异步分发给投递范围内已启用的订阅者，不阻塞调用方。`surface_id` 为事件所在的候选框。
    pub(crate) fn dispatch(self: &Arc<Self>, payload: Payload, surface_id: Option<&str>, route: Route) {
        let mut dispatch = self.prepare_dispatch(payload, route);
        if dispatch.recipients.is_empty() {
            return;
        }
        if let Some(id) = surface_id {
            self.update_surface(id, |surface| dispatch.attach_surface(surface));
        }
        self.run_dispatch(dispatch);
    }

    pub(crate) fn prepare_dispatch(&self, payload: Payload, route: Route) -> PreparedDispatch {
        let settings = self.settings();
        let event = Event::new(settings.languages(), None, payload);
        let recipients = self.extensions.read().unwrap().recipients(&settings, event.event_type(), &route);
        PreparedDispatch { event, recipients, shell_lifetime: None }
    }

    pub(crate) fn run_dispatch(self: &Arc<Self>, dispatch: PreparedDispatch) {
        let PreparedDispatch { event, recipients, shell_lifetime } = dispatch;
        let surface_id = event.surface.as_ref().map(|surface| surface.id.clone());
        let input_context = match &event.payload {
            Payload::Input { input, source } => {
                Some(InputContext { input: input.clone(), source: source.clone() })
            }
            _ => None,
        };
        let focus = match &event.payload {
            Payload::Focus { app } => Some(app.clone()),
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
                input: input_context.clone(),
                focus: focus.clone(),
                shell_lifetime: shell_lifetime.clone(),
            });
            let reports_failure = event.event_type() == EventType::Input;
            let (inner, event, surface_id) = (self.clone(), event.clone(), surface_id.clone());
            self.runtime.spawn(async move {
                let result = run(&plugin, &event, host).await;
                inner.finish_run(result, surface_id.as_deref(), reports_failure);
            });
        }
    }

    /// 前台应用切换：同一应用的重复上报（如切换窗口）不广播。
    pub(crate) fn app_focused(self: &Arc<Self>, app: Option<AppInfo>) {
        let mut focused = self.focused.lock().unwrap();
        if focused.as_ref() == Some(&app) {
            return;
        }
        *focused = Some(app.clone());
        drop(focused);
        self.dispatch(Payload::Focus { app }, None, Route::Republished { by: APP_SOURCE.into() });
    }

    /// 让扩展按当前前台应用重新处理一次，用于它刚启用或数据刚变化时。
    pub(crate) fn refresh_focus(self: &Arc<Self>, extension_id: String) {
        let Some(app) = self.focused.lock().unwrap().clone() else { return };
        self.dispatch(Payload::Focus { app }, None, Route::Target { extension_id });
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
        let delivery = {
            let _insertion = self.insertion.lock().await;
            self.platform.insert_text(text.clone(), destination.clone()).await?
        };
        let payload = Payload::Inserted { text, destination, delivery, by: by.clone() };
        self.dispatch(payload, surface_id.as_deref(), Route::Republished { by });
        Ok(delivery)
    }

    /// 记录执行结果；处理输入失败时把原因显示在候选框中。
    fn finish_run(&self, result: RunResult, surface_id: Option<&str>, reports_failure: bool) {
        if result.status != RunStatus::Succeeded || !result.warnings.is_empty() {
            log::error!("extension {} {:?} {:?}", result.extension_id, result.status, result.warnings);
        }
        let failure = reports_failure.then(|| self.failure_message(&result)).flatten();
        if let Some(id) = surface_id {
            self.update_surface(id, |surface| {
                if let Some(message) = failure {
                    surface.upsert(failure_block(message), APP_SOURCE, None);
                }
                surface.mark_finished();
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

/// 广播的输入与触发时的前台应用：`publish` 的翻译事件与 `insert` 的写入目标都来自这里。
#[derive(Clone)]
struct InputContext {
    input: Input,
    source: Option<AppInfo>,
}

/// 一个扩展处理一次事件时的宿主。能力校验已由插件运行时完成。
struct DispatchHost {
    inner: Arc<Inner>,
    extension_id: String,
    title: Option<String>,
    surface_id: Option<String>,
    input: Option<InputContext>,
    /// 处理 `focus` 事件时，事件对应的应用。
    focus: Option<Option<AppInfo>>,
    shell_lifetime: Option<tokio::sync::watch::Receiver<()>>,
}

#[async_trait]
impl Host for DispatchHost {
    fn shell_lifetime(&self) -> Option<tokio::sync::watch::Receiver<()>> {
        self.shell_lifetime.clone()
    }

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
            Instruction::AttachActions { block, actions } => {
                // 候选框已关闭时没有可附加的块，与 RemoveBlock 一样静默结束。
                if let Some(id) = &self.surface_id {
                    self.inner
                        .update_surface(id, |surface| surface.attach(&block, &self.extension_id, actions))
                        .transpose()?;
                }
            }
            Instruction::Store(write) => {
                self.inner.stores.apply(&self.extension_id, write)?;
                self.inner.observer.store_changed(self.extension_id.clone());
            }
            Instruction::SetContext(text) => {
                // 每个事件各自运行，快速切换时旧事件可能后完成，不能覆盖当前应用的背景。
                let current =
                    self.focus.as_ref().is_none_or(|app| self.inner.focused.lock().unwrap().as_ref() == Some(app));
                if current {
                    self.inner.set_context(&text);
                }
            }
            Instruction::Speak { text, language } => {
                let settings = self.inner.settings();
                if settings.auto_speak {
                    self.inner.speak(text, language, self.surface_id.as_deref());
                }
            }
            Instruction::PublishTranslated { output, mut pairs, context, card } => {
                let input = self
                    .input
                    .as_ref()
                    .ok_or_else(|| CoreError::InvalidInstruction("publish needs an input event".into()))?;
                // 整句卡片由广播者画出，词条卡片由 Sayso 统一画出；订阅者只往卡片上附加按钮，两种卡片的引用都随事件广播。
                let mut card_ref = None;
                if let Some(id) = &self.surface_id {
                    let language = self.inner.settings().languages().target;
                    let cards = self.inner.update_surface(id, |surface| {
                        let whole = surface.mark_translation_card(&self.extension_id, &card)?;
                        let words = surface.present_pairs(&pairs, language, &self.extension_id, self.title.clone());
                        Ok::<_, CoreError>((whole, words))
                    });
                    if let Some(cards) = cards {
                        let (whole, words) = cards?;
                        card_ref = Some(whole);
                        for (pair, word) in pairs.iter_mut().zip(words) {
                            pair.card = Some(word);
                        }
                    }
                }
                let translation = Translation {
                    input: input.input.clone(),
                    output,
                    pairs,
                    context,
                    card: card_ref,
                    source: input.source.clone(),
                };
                let route = Route::Republished { by: self.extension_id.clone() };
                self.inner.dispatch(Payload::Translated(translation), self.surface_id.as_deref(), route);
            }
        }
        Ok(())
    }

    async fn complete(&self, system: String, user: String) -> Result<String, CoreError> {
        let endpoint = self.inner.llm_endpoint().ok_or(CoreError::ModelNotConfigured)?;
        let system = with_user_context(&system, &self.inner.context());
        endpoint.complete(&self.inner.http, &system, &user).await
    }

    /// 写入触发输入时的前台应用。
    async fn insert(&self, text: String) -> Result<Delivery, CoreError> {
        let input = self.input.as_ref().ok_or(CoreError::InsertOutsideInput)?;
        let by = self.extension_id.clone();
        self.inner.insert(text, input.source.clone(), by, self.surface_id.clone()).await
    }

    fn records(&self, collection: &str) -> Result<Vec<StoreRecord>, CoreError> {
        self.inner.stores.records(&self.extension_id, collection)
    }
}
