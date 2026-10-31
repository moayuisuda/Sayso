//! 输入会话：候选框的唯一状态源。入口键触发一个路由后，按路由的输入来源准备好文本，再交给路由上的命令：
//! - 语音路由：录音 → 识别 → 交给命令 → 扩展输出块
//! - 选区路由：读取选区 → 交给命令 → 扩展输出块
//!
//! 按住入口键即开始为「直接」路由录音；之后按的语音路由键改变本次交给的命令，选区路由键则放弃录音、改为读取选区。

mod action;
mod dispatch;
mod speech;
mod surface;

use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) use action::ActionContext;
pub(crate) use surface::{failure_block, hold, Surface, APP_SOURCE};
pub use surface::{SurfaceSection, SurfaceView};

use crate::app::Inner;
use crate::asr::{source, AsrConfig, PartialSink, Recognition};
use crate::extensions::Route;
use crate::l10n::tr;
use crate::protocol::{AppInfo, CommandRef, Input, InputSource, Payload};
use crate::settings::{CommandKey, RouteKey, Settings};

/// 候选框自动隐藏的检查间隔。
const DISMISS_CHECK: Duration = Duration::from_millis(250);

/// 交给 UI 的会话状态。
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SessionState {
    Idle,
    /// 录音中；`partial` 为实时识别文本，`commands` 为本次将交给的命令名。
    Recording {
        partial: String,
        commands: Vec<String>,
    },
    /// 识别或读取选区中。
    Processing {
        message: String,
    },
    Presenting {
        surface: SurfaceView,
    },
}

#[derive(Default)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Recording {
        partial: String,
    },
    Processing {
        message: String,
    },
    Presenting(Surface),
}

#[derive(Default)]
pub(crate) struct Session {
    pub(crate) phase: Phase,
    /// 每次开始新流程递增，用于丢弃过期的异步回调。
    generation: u64,
    /// 本次的路由。
    routes: Vec<RouteKey>,
    /// 本次路由上的命令名，录音时显示。
    command_titles: Vec<String>,
    /// 触发本次流程时的前台应用，命令写入的目标。候选框不会成为前台窗口，因此触发时取得即可。
    source: Option<AppInfo>,
    recognition: Option<Recognition>,
    /// 正在读取选区的任务。取消时中止，平台随之放弃读取（如不再等待松开按键后模拟复制）。
    selection: Option<tokio::task::AbortHandle>,
    hovering: bool,
}

impl Session {
    pub fn is_active(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. } | Phase::Processing { .. })
    }

    pub fn state(&self) -> SessionState {
        match &self.phase {
            Phase::Idle => SessionState::Idle,
            Phase::Recording { partial } => {
                SessionState::Recording { partial: partial.clone(), commands: self.command_titles.clone() }
            }
            Phase::Processing { message } => SessionState::Processing { message: message.clone() },
            Phase::Presenting(surface) => SessionState::Presenting { surface: surface.view() },
        }
    }
}

/// 一次输入准备好后要交给命令的内容。
struct CommandInput {
    surface_id: String,
    input: Input,
    /// 本次路由上的命令，在候选框出现时即已记为处理中。
    commands: Vec<CommandKey>,
    source: Option<AppInfo>,
}

impl Inner {
    /// 修改会话并推送新状态。推送在锁内进行，保证 UI 收到的状态顺序与修改顺序一致。
    pub(crate) fn update_session<R>(&self, change: impl FnOnce(&mut Session) -> R) -> R {
        let mut session = self.session.lock().unwrap();
        let result = change(&mut session);
        self.observer.session_changed(session.state());
        result
    }

    pub(crate) fn trigger(self: &Arc<Self>, key: RouteKey) {
        let settings = self.settings();
        let Some(route) = settings.route(key) else { return };
        // 只按入口键而「直接」路由上没有命令时，可能只是在按别的快捷键，不打扰用户。
        if self.extensions.read().unwrap().commands_routed_to(&settings, &[key]).is_empty() {
            return;
        }
        self.update_session(|session| {
            if !settings.languages().is_valid() {
                return self.present_failure(
                    session,
                    tr("目标语言与本地语言不能相同", "Target and native languages must differ"),
                );
            }
            match route.input {
                InputSource::Speech => match session.phase {
                    // 按了语音路由键：本次交给路由键上的命令，不再交给「直接」路由。
                    Phase::Recording { .. } => {
                        session.routes.retain(|existing| *existing != RouteKey::Direct && *existing != key);
                        session.routes.push(key);
                        session.command_titles = self.command_titles(&settings, &session.routes);
                    }
                    _ if !session.is_active() => self.start_recording(session, key, &settings),
                    _ => {}
                },
                InputSource::Selection => {
                    if matches!(session.phase, Phase::Recording { .. }) {
                        self.cancel_flow(session);
                    }
                    if !session.is_active() {
                        self.read_selection(session, key);
                    }
                }
            }
        });
    }

    pub(crate) fn toggle(self: &Arc<Self>, keys: Vec<RouteKey>) {
        let recording = matches!(self.session.lock().unwrap().phase, Phase::Recording { .. });
        if !recording {
            return keys.into_iter().for_each(|key| self.trigger(key));
        }
        keys.into_iter().filter(|key| *key != RouteKey::Direct).for_each(|key| self.trigger(key));
        self.stop();
    }

    /// 结束录音，等待识别结果后交给命令。
    pub(crate) fn stop(self: &Arc<Self>) {
        let finishing = self.update_session(|session| {
            let Phase::Recording { partial } = &session.phase else { return None };
            let message = partial.clone();
            let recognition = session.recognition.take()?;
            self.stop_audio();
            session.phase = Phase::Processing { message };
            Some((recognition, session.generation))
        });
        let Some((recognition, generation)) = finishing else { return };
        let inner = self.clone();
        self.runtime.spawn(async move {
            let outcome = recognition.finish().await;
            let input = inner.update_session(|session| {
                if session.generation != generation {
                    return None;
                }
                match outcome {
                    Ok(transcript) if transcript.is_empty() => {
                        session.phase = Phase::Idle;
                        None
                    }
                    Ok(transcript) => Some(inner.present_input(session, transcript, InputSource::Speech, hold::SPEECH)),
                    Err(error) => {
                        inner.present_failure(session, error.to_string());
                        None
                    }
                }
            });
            if let Some(input) = input {
                inner.invoke_commands(input);
            }
        });
    }

    pub(crate) fn cancel(&self) {
        self.update_session(|session| {
            if session.is_active() {
                self.cancel_flow(session);
            }
        });
    }

    pub(crate) fn dismiss(&self, surface_id: Option<&str>) {
        let mut session = self.session.lock().unwrap();
        let Phase::Presenting(surface) = &session.phase else { return };
        // 旧面板的事件既不改变下一轮会话，也不推送无效的状态通知。
        if surface_id.is_some_and(|id| id != surface.id) {
            return;
        }
        session.phase = Phase::Idle;
        session.hovering = false;
        self.observer.session_changed(SessionState::Idle);
    }

    pub(crate) fn set_hovering(&self, hovering: bool) {
        let mut session = self.session.lock().unwrap();
        session.hovering = hovering;
        if let (false, Phase::Presenting(surface)) = (hovering, &mut session.phase) {
            surface.touch();
        }
        // 悬停只影响内部停留计时，不改变可见内容，无需再次推送会话状态。
    }

    // MARK: 流程

    /// 开始新流程：记下本次路由与前台应用，使旧流程的回调失效，停止上一次的朗读。
    fn begin_flow(&self, session: &mut Session, key: RouteKey) -> u64 {
        session.routes = vec![key];
        session.command_titles = self.command_titles(&self.settings(), &session.routes);
        session.generation += 1;
        session.source = self.platform.frontmost_app();
        self.platform.stop_speaking();
        session.generation
    }

    fn command_titles(&self, settings: &Settings, routes: &[RouteKey]) -> Vec<String> {
        let extensions = self.extensions.read().unwrap();
        extensions
            .enabled_commands(settings)
            .filter(|(key, _)| settings.binding(key).is_some_and(|route| routes.contains(&route)))
            .map(|(_, spec)| spec.title.clone())
            .collect()
    }

    fn cancel_flow(&self, session: &mut Session) {
        session.generation += 1;
        if matches!(session.phase, Phase::Recording { .. }) {
            self.stop_audio();
        }
        if let Some(recognition) = session.recognition.take() {
            recognition.cancel();
        }
        if let Some(selection) = session.selection.take() {
            selection.abort();
        }
        session.phase = Phase::Idle;
    }

    fn start_recording(self: &Arc<Self>, session: &mut Session, key: RouteKey, settings: &Settings) {
        let Some(config) = self.asr_config(settings) else {
            return self.present_failure(
                session,
                tr("请先在设置中配置语音识别服务", "Configure a speech recognition service in Settings first"),
            );
        };
        let generation = self.begin_flow(session, key);
        let weak = Arc::downgrade(self);
        let partial: PartialSink = Arc::new(move |text| {
            let Some(inner) = weak.upgrade() else { return };
            inner.update_session(|session| {
                if session.generation == generation && matches!(session.phase, Phase::Recording { .. }) {
                    session.phase = Phase::Recording { partial: text };
                }
            });
        });
        // 先开始录音再连接识别服务，连接期间的音频留在通道中，避免吞掉开头。
        let recognition = Recognition::start(&self.runtime, config, self.http.clone(), partial);
        *self.audio.lock().unwrap() = Some(recognition.feed());
        if let Err(error) = self.platform.start_audio() {
            *self.audio.lock().unwrap() = None;
            recognition.cancel();
            return self.present_failure(session, error.to_string());
        }
        session.recognition = Some(recognition);
        session.phase = Phase::Recording { partial: String::new() };
        self.warm_up_translation();
    }

    /// 停止录音：平台在返回前送完尾部音频，随后释放音频入口，识别服务收到音频结束。
    fn stop_audio(&self) {
        self.platform.stop_audio();
        *self.audio.lock().unwrap() = None;
    }

    fn read_selection(self: &Arc<Self>, session: &mut Session, key: RouteKey) {
        let generation = self.begin_flow(session, key);
        self.warm_up_translation();
        session.phase = Phase::Processing { message: tr("读取选中文本…", "Reading selection…") };
        let inner = self.clone();
        let task = self.runtime.spawn(async move {
            let text = inner.platform.read_selection().await.trim().to_string();
            let input = inner.update_session(|session| {
                if session.generation != generation {
                    return None;
                }
                session.selection = None;
                if text.is_empty() {
                    inner.present_failure(session, tr("没有选中文本", "No text selected"));
                    return None;
                }
                Some(inner.present_input(session, text, InputSource::Selection, hold::SELECTION))
            });
            if let Some(input) = input {
                inner.invoke_commands(input);
            }
        });
        session.selection = Some(task.abort_handle());
    }

    /// 输入准备好：打开候选框（先显示输入原文），返回要交给命令的内容。
    /// 候选框出现时就把命令所属扩展记为处理中，界面从「处理中」直接过渡，不会先闪一次空的结果。
    fn present_input(
        self: &Arc<Self>,
        session: &mut Session,
        text: String,
        via: InputSource,
        hold: Duration,
    ) -> CommandInput {
        let commands = self.extensions.read().unwrap().commands_routed_to(&self.settings(), &session.routes);
        let mut surface = Surface::new(Some(text.clone()), hold);
        surface.mark_running(commands.iter().map(|key| key.extension_id.clone()));
        let surface_id = surface.id.clone();
        self.present(session, surface);
        CommandInput { surface_id, input: Input { text, via }, commands, source: session.source.clone() }
    }

    /// 把命令事件分别发给本次每个命令所属的扩展，各扩展并发处理；失败原因显示在候选框中。
    /// 同时向订阅者广播 `input` hook。
    fn invoke_commands(self: &Arc<Self>, command: CommandInput) {
        let hook = Payload::Input { input: command.input.clone(), source: command.source.clone() };
        self.dispatch(hook, Some(&command.surface_id), Route::Republished { by: APP_SOURCE.into() });
        for key in command.commands {
            let payload = Payload::Command {
                command: CommandRef { id: key.command_id },
                input: command.input.clone(),
                source: command.source.clone(),
            };
            let route = Route::Target { extension_id: key.extension_id, reports_failure: true };
            self.dispatch(payload, Some(&command.surface_id), route);
        }
    }

    fn present_failure(self: &Arc<Self>, session: &mut Session, message: String) {
        self.present(session, Surface::failure(message));
    }

    /// 展示候选框，并按停留规则在内容静止后自动隐藏。
    fn present(self: &Arc<Self>, session: &mut Session, surface: Surface) {
        let id = surface.id.clone();
        session.phase = Phase::Presenting(surface);
        let weak = Arc::downgrade(self);
        self.runtime.spawn(async move {
            let mut ticks = tokio::time::interval(DISMISS_CHECK);
            loop {
                ticks.tick().await;
                let Some(inner) = weak.upgrade() else { return };
                let mut session = inner.session.lock().unwrap();
                let hovering = session.hovering;
                // 候选框已被替换或关闭：本计时结束。
                let Phase::Presenting(surface) = &session.phase else { return };
                if surface.id != id {
                    return;
                }
                if surface.should_dismiss(hovering, Instant::now()) {
                    session.phase = Phase::Idle;
                    inner.observer.session_changed(session.state());
                    return;
                }
            }
        });
    }

    /// 预先连上翻译服务，正式请求时直接复用已建立的连接。
    fn warm_up_translation(&self) {
        let Some(endpoint) = self.llm_endpoint() else { return };
        let http = self.http.clone();
        self.runtime.spawn(async move { endpoint.warm_up(&http).await });
    }

    /// 按设置生成本次录音的识别配置；缺少必填项或来源不可用时为 None。
    fn asr_config(&self, settings: &Settings) -> Option<AsrConfig> {
        source::config(settings, &self.secrets())
    }
}
