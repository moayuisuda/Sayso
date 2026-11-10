//! 输入会话：快捷键触发录音识别或读取选区，文本就绪后广播 input。

mod action;
mod dispatch;
mod speech;
mod surface;

use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) use action::ActionContext;
pub(crate) use surface::{failure_block, hold, Surface, APP_SOURCE};
pub use surface::{Attachment, SurfaceBlock, SurfaceSection, SurfaceView};

use dispatch::PreparedDispatch;
use crate::app::Inner;
use crate::asr::{source, AsrConfig, PartialSink, Recognition};
use crate::extensions::Route;
use crate::l10n::tr;
use crate::protocol::{AppInfo, Input, InputSource, Payload};
use crate::settings::{Hotkey, Settings};

/// 候选框自动隐藏的检查间隔。
const DISMISS_CHECK: Duration = Duration::from_millis(250);

/// 交给 UI 的会话状态。
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SessionState {
    Idle,
    /// 录音中；`partial` 为实时识别文本。
    Recording {
        partial: String,
        /// 录音入口的名称。
        command: String,
        /// 开始录音的快捷键，用于提示如何结束。
        shortcut: Hotkey,
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

impl Phase {
    /// 日志中的阶段名。
    fn label(&self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Recording { .. } => "recording",
            Phase::Processing { .. } => "processing",
            Phase::Presenting(_) => "presenting",
        }
    }
}

#[derive(Default)]
pub(crate) struct Session {
    pub(crate) phase: Phase,
    /// 每次开始新流程递增，用于丢弃过期的异步回调。
    generation: u64,
    recording_shortcut: Option<Hotkey>,
    /// 触发本次流程时的前台应用，扩展写入的目标。候选框不会成为前台窗口，因此触发时取得即可。
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
                SessionState::Recording {
                    partial: partial.clone(),
                    command: tr("语音输入", "Voice input"),
                    shortcut: self.recording_shortcut.expect("a recording always starts from an input shortcut"),
                }
            }
            Phase::Processing { message } => SessionState::Processing { message: message.clone() },
            Phase::Presenting(surface) => SessionState::Presenting { surface: surface.view() },
        }
    }
}

impl Inner {
    /// 修改会话并推送新状态。推送在锁内进行，保证 UI 收到的状态顺序与修改顺序一致。
    pub(crate) fn update_session<R>(&self, change: impl FnOnce(&mut Session) -> R) -> R {
        let mut session = self.session.lock().unwrap();
        let result = change(&mut session);
        self.observer.session_changed(session.state());
        result
    }

    pub(crate) fn trigger(self: &Arc<Self>, key: Hotkey) {
        let settings = self.settings();
        let Some(input) = settings.input_for(key) else { return };
        self.update_session(|session| {
            log::info!("trigger {key:?} input={:?} phase={}", input, session.phase.label());
            if !settings.languages().is_valid() {
                return self.present_failure(
                    session,
                    tr("目标语言与本地语言不能相同", "Target and native languages must differ"),
                );
            }
            match input {
                InputSource::Speech => {
                    if !session.is_active() {
                        self.start_recording(session, key, &settings);
                    }
                }
                InputSource::Selection => {
                    if matches!(session.phase, Phase::Recording { .. }) {
                        self.cancel_flow(session);
                    }
                    if !session.is_active() {
                        self.read_selection(session);
                    }
                }
            }
        });
    }

    pub(crate) fn toggle(self: &Arc<Self>, key: Hotkey) {
        let recording = matches!(self.session.lock().unwrap().phase, Phase::Recording { .. });
        let speech = self.settings().input_for(key) == Some(InputSource::Speech);
        if recording && speech {
            return self.stop();
        }
        self.trigger(key);
    }

    /// 结束录音，等待识别结果后广播文本。
    pub(crate) fn stop(self: &Arc<Self>) {
        let finishing = self.update_session(|session| {
            log::info!("stop phase={} generation={}", session.phase.label(), session.generation);
            let Phase::Recording { partial } = &session.phase else { return None };
            let message = partial.clone();
            let Some(recognition) = session.recognition.take() else {
                log::error!("stop while recording without a recognition");
                return None;
            };
            self.stop_audio();
            session.phase = Phase::Processing { message };
            Some((recognition, session.generation))
        });
        let Some((recognition, generation)) = finishing else { return };
        let inner = self.clone();
        self.runtime.spawn(async move {
            let outcome = recognition.finish().await;
            match &outcome {
                Ok(transcript) => log::info!("recognition {generation} finished, {} chars", transcript.chars().count()),
                Err(error) => log::error!("recognition {generation} failed: {error}"),
            }
            let input = inner.update_session(|session| {
                if session.generation != generation {
                    log::info!("recognition {generation} result dropped, session moved to {}", session.generation);
                    return None;
                }
                match outcome {
                    Ok(transcript) if transcript.is_empty() => {
                        session.phase = Phase::Idle;
                        None
                    }
                    Ok(transcript) => Some(inner.present_input(session, transcript, InputSource::Speech, hold::SPEECH)),
                    // 识别最终失败时，保留松开按键前的实时文字，用户说的话不至于全部丢失。
                    Err(error) => {
                        let spoken = match &session.phase {
                            Phase::Processing { message } if !message.is_empty() => Some(message.clone()),
                            _ => None,
                        };
                        inner.present(session, Surface::failure(spoken, error.to_string()));
                        None
                    }
                }
            });
            if let Some(input) = input {
                inner.run_dispatch(input);
            }
        });
    }

    pub(crate) fn cancel(&self) {
        self.update_session(|session| {
            log::info!("cancel phase={} generation={}", session.phase.label(), session.generation);
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

    /// 开始新流程：记下前台应用，使旧流程的回调失效，停止上一次的朗读。
    fn begin_flow(&self, session: &mut Session) -> u64 {
        session.generation += 1;
        session.source = self.platform.frontmost_app();
        self.platform.stop_speaking();
        session.generation
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

    fn start_recording(self: &Arc<Self>, session: &mut Session, key: Hotkey, settings: &Settings) {
        let Some(config) = self.asr_config(settings) else {
            return self.present_failure(
                session,
                tr("请先在设置中配置语音识别服务", "Configure a speech recognition service in Settings first"),
            );
        };
        let generation = self.begin_flow(session);
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
        session.recording_shortcut = Some(key);
        session.phase = Phase::Recording { partial: String::new() };
        log::info!("recording {generation} started");
        self.warm_up_translation();
    }

    /// 停止录音：平台在返回前送完尾部音频，随后释放音频入口，识别服务收到音频结束。
    fn stop_audio(&self) {
        self.platform.stop_audio();
        *self.audio.lock().unwrap() = None;
    }

    fn read_selection(self: &Arc<Self>, session: &mut Session) {
        let generation = self.begin_flow(session);
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
                inner.run_dispatch(input);
            }
        });
        session.selection = Some(task.abort_handle());
    }

    /// 文本就绪后准备广播，在候选框首次展示时就标记全部订阅者正在处理。
    fn present_input(
        self: &Arc<Self>,
        session: &mut Session,
        text: String,
        via: InputSource,
        hold: Duration,
    ) -> PreparedDispatch {
        let mut surface = Surface::new(Some(text.clone()), hold);
        let payload = Payload::Input { input: Input { text, via }, source: session.source.clone() };
        let mut dispatch = self.prepare_dispatch(payload, Route::Republished { by: APP_SOURCE.into() });
        dispatch.attach_surface(&mut surface);
        self.present(session, surface);
        dispatch
    }

    fn present_failure(self: &Arc<Self>, session: &mut Session, message: String) {
        self.present(session, Surface::failure(None, message));
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
        source::config(settings, &self.secrets(), &self.context())
    }
}
