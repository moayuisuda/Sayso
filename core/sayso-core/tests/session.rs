//! 输入会话的端到端行为：用测试平台与本地 HTTP 识别服务驱动 `Core`，断言路由、写入与候选框内容。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use sayso_core::asr::source::OPENAI_COMPATIBLE;
use sayso_core::protocol::{ActionKind, AppInfo, BlockAction, BlockContent, Delivery, InputSource, Language};
use sayso_core::session::{SessionState, SurfaceView};
use sayso_core::llm::LlmProvider;
use sayso_core::settings::{CommandKey, Hotkey, PlatformDefaults, RouteKey, SecretSlot};
use sayso_core::{Core, CoreObserver, Platform, PlatformError};

const SELECTION_KEY: u16 = 17;

#[derive(Default)]
struct FakePlatform {
    /// 选中的文本；None 表示读取一直等待（如用户还按着按键），直到被取消。
    selection: Option<String>,
    /// 开始与被中止的选区读取次数。
    selection_reads_started: AtomicUsize,
    selection_reads_aborted: Arc<AtomicUsize>,
    inserted: Mutex<Vec<(String, Option<AppInfo>)>>,
    insert_error: Mutex<Option<String>>,
    audio_stops: AtomicUsize,
    speech_error: Mutex<Option<String>>,
    spoken: Mutex<Vec<(String, Language)>>,
}

#[async_trait]
impl Platform for FakePlatform {
    fn start_audio(&self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn stop_audio(&self) {
        self.audio_stops.fetch_add(1, Ordering::SeqCst);
    }
    async fn read_selection(&self) -> String {
        self.selection_reads_started.fetch_add(1, Ordering::SeqCst);
        match &self.selection {
            Some(text) => text.clone(),
            None => {
                /// 读取被中止时 future 被丢弃，借此计数。
                struct Abort(Arc<AtomicUsize>);
                impl Drop for Abort {
                    fn drop(&mut self) {
                        self.0.fetch_add(1, Ordering::SeqCst);
                    }
                }
                let _abort = Abort(self.selection_reads_aborted.clone());
                std::future::pending().await
            }
        }
    }
    async fn insert_text(&self, text: String, destination: Option<AppInfo>) -> Result<Delivery, PlatformError> {
        if let Some(message) = self.insert_error.lock().unwrap().clone() {
            return Err(PlatformError::Failed { message });
        }
        self.inserted.lock().unwrap().push((text, destination));
        Ok(Delivery::Inserted)
    }
    fn frontmost_app(&self) -> Option<AppInfo> {
        Some(AppInfo { id: Some("com.example.notes".into()), name: Some("Notes".into()) })
    }
    fn speak(&self, text: String, language: Language, _rate: f32) -> Result<(), PlatformError> {
        self.spoken.lock().unwrap().push((text, language));
        match self.speech_error.lock().unwrap().clone() {
            Some(message) => Err(PlatformError::Failed { message }),
            None => Ok(()),
        }
    }
    fn stop_speaking(&self) {}
    fn copy_text(&self, _text: String) {}
    fn open_url(&self, _url: String) {}
}

#[derive(Default)]
struct RecordingObserver {
    states: Mutex<Vec<SessionState>>,
}

impl CoreObserver for RecordingObserver {
    fn session_changed(&self, state: SessionState) {
        self.states.lock().unwrap().push(state);
    }
    fn extensions_changed(&self) {}
    fn store_changed(&self, _extension_id: String) {}
    fn settings_changed(&self) {}
}

/// 在真实 TCP 连接上检查批量识别契约；Drop 唤醒 accept，避免未使用的服务线程泄漏。
struct LocalAsr {
    address: SocketAddr,
    stopping: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl LocalAsr {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stopping = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let worker = {
            let stopping = stopping.clone();
            let requests = requests.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let mut stream = stream.unwrap();
                    if stopping.load(Ordering::SeqCst) {
                        break;
                    }
                    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut request = Vec::new();
                    let (header_end, length) = loop {
                        let mut buffer = [0; 4096];
                        let count = stream.read(&mut buffer).unwrap();
                        assert_ne!(count, 0, "request closed before headers");
                        request.extend_from_slice(&buffer[..count]);
                        if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                            assert!(headers.starts_with("post /v1/audio/transcriptions http/1.1"));
                            assert!(!headers.contains("authorization:"), "empty API key must omit authentication");
                            let length = headers
                                .lines()
                                .find_map(|line| {
                                    line.strip_prefix("content-length:")
                                        .map(|value| value.trim().parse::<usize>().unwrap())
                                })
                                .expect("multipart body has a known length");
                            break (end + 4, length);
                        }
                    };
                    while request.len() < header_end + length {
                        let mut buffer = [0; 4096];
                        let count = stream.read(&mut buffer).unwrap();
                        assert_ne!(count, 0, "request closed before body");
                        request.extend_from_slice(&buffer[..count]);
                    }
                    let body = &request[header_end..header_end + length];
                    assert!(body.windows(4).any(|part| part == b"RIFF"));
                    assert!(body.windows(8).any(|part| part == b"WAVEfmt "));
                    let multipart = String::from_utf8_lossy(body);
                    assert!(multipart.contains("name=\"model\"\r\n\r\nlocal-test"));
                    requests.fetch_add(1, Ordering::SeqCst);
                    let response = r#"{"text":"I have a 苹果."}"#;
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
                }
            })
        };
        Self { address, stopping, requests, worker: Some(worker) }
    }
}

impl Drop for LocalAsr {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        let result = self.worker.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

/// 本地 HTTP 服务提供转写，数据目录只安装订阅 `input` hook 的 echo 扩展。
fn fixture(selection: Option<&str>) -> (tempfile::TempDir, Core, Arc<FakePlatform>, Arc<RecordingObserver>, LocalAsr) {
    let data = tempfile::tempdir().unwrap();
    let server = LocalAsr::start();
    let install = |name: &str, manifest: &str, files: &[(&str, &str)]| {
        let directory = data.path().join("Extensions").join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("extension.json"), manifest).unwrap();
        for (file, content) in files {
            std::fs::write(directory.join(file), content).unwrap();
        }
    };
    install(
        "echo",
        r#"{"schemaVersion":4,"id":"test.echo","name":"Echo","main":"main.js","events":["input"],
            "capabilities":["blocks"]}"#,
        &[(
            "main.js",
            r#"export function input(event) {
                 sayso.blocks.upsert({ id: "echo", type: "status", message: `${event.input.via}: ${event.input.text}` });
                 sayso.speak(event.input.text, "ko");
               }"#,
        )],
    );

    let platform = Arc::new(FakePlatform { selection: selection.map(Into::into), ..Default::default() });
    let observer = Arc::new(RecordingObserver::default());
    let defaults = PlatformDefaults {
        trigger_key: Hotkey { key_code: 61, modifiers: 0 },
        selection_route_key: SELECTION_KEY,
        speech_rate: 0.5,
    };
    let core = Core::new(data.path().to_string_lossy().into(), defaults, platform.clone(), observer.clone()).unwrap();
    let core = Arc::try_unwrap(core).ok().unwrap();

    let mut settings = core.settings();
    settings.asr_source = OPENAI_COMPATIBLE.into();
    settings.asr_options.insert(
        OPENAI_COMPATIBLE.into(),
        [("baseUrl".into(), format!("http://{}/v1", server.address)), ("model".into(), "local-test".into())].into(),
    );
    settings.enabled_extensions.push("test.echo".into());
    core.update_settings(settings).unwrap();
    (data, core, platform, observer, server)
}

/// 等待候选框满足条件，返回此时的内容。
fn wait_for_surface(core: &Core, condition: impl Fn(&SurfaceView) -> bool) -> SurfaceView {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let SessionState::Presenting { surface } = core.session_state() {
            if condition(&surface) {
                return surface;
            }
        }
        assert!(Instant::now() < deadline, "timed out; last state {:?}", core.session_state());
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn block_ids(surface: &SurfaceView, source: &str) -> Vec<String> {
    surface
        .sections
        .iter()
        .filter(|section| section.source_id == source)
        .flat_map(|section| section.blocks.iter().map(|block| block.id.clone()))
        .collect()
}

fn status_message(surface: &SurfaceView, source: &str) -> String {
    let section = surface.sections.iter().find(|section| section.source_id == source).expect("section exists");
    let BlockContent::Status { message, .. } = &section.blocks[0].content else { panic!("expected status block") };
    message.clone()
}

/// 直接路由：录音 → 本地 HTTP 服务 → 语音翻译。没有配置翻译模型时原样写入触发时的前台应用，并提示原因。
#[test]
fn speech_flow_inserts_transcript_into_the_triggering_app() {
    let (_data, core, platform, observer, server) = fixture(Some(""));

    core.trigger(RouteKey::Direct);
    assert_eq!(
        core.session_state(),
        SessionState::Recording {
            partial: String::new(), commands: vec!["翻译并输入 / Translate and type".into()]
        }
    );
    core.push_audio(vec![0; 16000]);
    core.stop();

    let surface = wait_for_surface(&core, |surface| block_ids(surface, "sayso.dictation").len() == 2);
    assert_eq!(block_ids(&surface, "sayso.dictation"), ["text", "notice"]);
    let inserted = platform.inserted.lock().unwrap().clone();
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    assert_eq!(inserted.len(), 1);
    assert_eq!(inserted[0].0, "I have a 苹果.");
    assert_eq!(inserted[0].1.as_ref().and_then(|app| app.id.as_deref()), Some("com.example.notes"));

    // 松开后从「处理中」直接过渡：候选框第一次出现时命令已记为处理中，不会先闪一次空的结果。
    let states = observer.states.lock().unwrap();
    let first = states.iter().find_map(|state| match state {
        SessionState::Presenting { surface } => Some(surface),
        _ => None,
    });
    assert!(first.is_some_and(|surface| surface.working), "{first:?}");
}

/// 平台写入异常必须穿过 Core 和插件边界，保留内容并显示真实原因。
#[test]
fn speech_flow_reports_the_real_insertion_error_and_keeps_the_transcript() {
    let (_data, core, platform, _, _server) = fixture(Some(""));
    *platform.insert_error.lock().unwrap() = Some("Clipboard snapshot failed".into());
    core.trigger(RouteKey::Direct);
    core.push_audio(vec![0; 16000]);
    core.stop();

    let surface =
        wait_for_surface(&core, |surface| !surface.working && block_ids(surface, "sayso.dictation").len() == 3);
    assert_eq!(block_ids(&surface, "sayso.dictation"), ["text", "insertion", "notice"]);
    assert!(platform.inserted.lock().unwrap().is_empty());
    let blocks = &surface.sections.iter().find(|section| section.source_id == "sayso.dictation").unwrap().blocks;
    let BlockContent::Text { runs, .. } = &blocks[0].content else { panic!("expected retained transcript") };
    assert_eq!(runs.iter().map(|run| run.text.as_str()).collect::<String>(), "I have a 苹果.");
    let BlockContent::Status { message, .. } = &blocks[1].content else { panic!("expected insertion error") };
    assert!(message.contains("Clipboard snapshot failed"));
}

/// 关闭事件只作用于对应结果框，迟到事件不得打断下一轮录音或识别。
#[test]
fn panel_dismissal_ignores_stale_ids_and_never_cancels_recording_or_processing() {
    let (_data, core, platform, observer, _server) = fixture(Some("괜찮아"));
    core.trigger(RouteKey::Key { code: SELECTION_KEY });
    let first = wait_for_surface(&core, |surface| !surface.working);
    let updates = observer.states.lock().unwrap().len();
    core.set_hovering(true);
    assert_eq!(observer.states.lock().unwrap().len(), updates, "hover must not publish unchanged content");
    core.dismiss_surface(first.id.clone());
    assert_eq!(core.session_state(), SessionState::Idle);

    core.trigger(RouteKey::Key { code: SELECTION_KEY });
    let second = wait_for_surface(&core, |surface| !surface.working);
    assert_ne!(first.id, second.id);
    let updates = observer.states.lock().unwrap().len();
    core.dismiss_surface(first.id);
    assert_eq!(observer.states.lock().unwrap().len(), updates, "stale dismissals must not publish state");
    assert!(matches!(core.session_state(), SessionState::Presenting { surface } if surface.id == second.id));
    core.trigger(RouteKey::Direct);
    core.dismiss_surface(second.id.clone());
    assert!(matches!(core.session_state(), SessionState::Recording { .. }));
    assert_eq!(platform.audio_stops.load(Ordering::SeqCst), 0);
    core.cancel();

    // 未完成的选区读取保证停在 Processing，覆盖迟到的鼠标事件与 Enter。
    let (_data, processing, _, _, _server) = fixture(None);
    processing.trigger(RouteKey::Key { code: SELECTION_KEY });
    processing.dismiss_surface(second.id);
    assert!(matches!(processing.session_state(), SessionState::Processing { .. }));
    processing.cancel();
}

/// 录音中按语音路由键：本次只交给该路由的命令，不再交给「直接」路由。
#[test]
fn speech_route_key_redirects_the_recording() {
    let (_data, core, platform, _, _server) = fixture(Some(""));

    core.set_route_input(RouteKey::Key { code: SELECTION_KEY }, InputSource::Speech).unwrap();
    core.trigger(RouteKey::Direct);
    core.trigger(RouteKey::Key { code: SELECTION_KEY });
    assert!(matches!(core.session_state(), SessionState::Recording { commands, .. }
        if commands == ["翻译文字 / Translate text"]));
    core.push_audio(vec![0; 16000]);
    core.stop();

    let surface = wait_for_surface(&core, |surface| !block_ids(surface, "test.echo").is_empty());
    assert_eq!(status_message(&surface, "test.echo"), "speech: I have a 苹果.");
    assert!(block_ids(&surface, "sayso.dictation").is_empty());
    assert!(platform.inserted.lock().unwrap().is_empty());
}

/// 播放失败应保留原文并提示；自动朗读和按钮共用同一通知，成功重试清除通知。
#[test]
fn speech_failure_preserves_content_and_retry_clears_the_notice() {
    let (_data, core, platform, _, _server) = fixture(Some("괜찮아"));
    *platform.speech_error.lock().unwrap() = Some("ko-KR voice is not installed".into());
    core.trigger(RouteKey::Key { code: SELECTION_KEY });
    let surface = wait_for_surface(&core, |surface| {
        !surface.working && block_ids(surface, "sayso.app").contains(&"speech".to_string())
    });
    assert_eq!(status_message(&surface, "test.echo"), "selection: 괜찮아");
    assert_eq!(*platform.spoken.lock().unwrap(), [("괜찮아".into(), Language::Korean)]);

    let play = || {
        core.perform_candidate_action(
            surface.id.clone(),
            "test.echo".into(),
            "echo".into(),
            BlockAction {
                id: None,
                label: None,
                kind: ActionKind::Speak { text: "괜찮아".into(), language: Language::Korean },
            },
        )
    };
    play();
    let failed = wait_for_surface(&core, |_| true);
    assert!(block_ids(&failed, "sayso.app").contains(&"speech".to_string()));
    *platform.speech_error.lock().unwrap() = None;
    play();
    let recovered = wait_for_surface(&core, |_| true);
    assert!(!block_ids(&recovered, "sayso.app").contains(&"speech".to_string()));
    assert_eq!(status_message(&recovered, "test.echo"), "selection: 괜찮아");

    let attempts = platform.spoken.lock().unwrap().len();
    core.trigger(RouteKey::Direct);
    play();
    assert_eq!(platform.spoken.lock().unwrap().len(), attempts, "stale buttons must not play during recording");
    core.cancel();
}

/// 生词本页在空闲时触发朗读失败，也必须走统一展示入口；悬停保留，移开后重新计时关闭。
#[test]
fn speech_failure_from_a_tab_starts_the_normal_dismiss_timer() {
    let (data, core, platform, _, _server) = fixture(Some(""));
    let stores = sayso_core::store::Stores::new(data.path().join("ExtensionData"));
    stores
        .apply(
            "sayso.wordbook",
            sayso_core::store::StoreWrite::Put {
                collection: "words".into(),
                key: "ko|괜찮아".into(),
                record: serde_json::json!({"term":"괜찮아","gloss":"没关系","language":"ko"})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        )
        .unwrap();
    *platform.speech_error.lock().unwrap() = Some("ko-KR voice is not installed".into());
    core.perform_tab_action(
        "sayso.wordbook".into(),
        "ko|괜찮아".into(),
        BlockAction {
            id: None,
            label: None,
            kind: ActionKind::Speak { text: "괜찮아".into(), language: Language::Korean },
        },
    );
    let surface = wait_for_surface(&core, |_| true);
    assert!(status_message(&surface, "sayso.app").contains("ko-KR"));
    core.set_hovering(true);
    std::thread::sleep(Duration::from_millis(3200));
    assert!(matches!(core.session_state(), SessionState::Presenting { .. }));
    let left = Instant::now();
    core.set_hovering(false);
    while !matches!(core.session_state(), SessionState::Idle) {
        assert!(left.elapsed() < Duration::from_secs(5), "speech failure surface did not auto-dismiss");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(left.elapsed() >= Duration::from_secs(3), "leaving the surface must reset its full hold time");
}

/// 录音中按选区路由键：放弃录音，改为读取选区交给划词翻译；翻译失败的原因显示在候选框中。
#[test]
fn selection_route_key_abandons_recording_and_reads_selection() {
    let (_data, core, platform, _, _server) = fixture(Some("serendipity"));

    core.trigger(RouteKey::Direct);
    core.trigger(RouteKey::Key { code: SELECTION_KEY });
    assert_eq!(platform.audio_stops.load(Ordering::SeqCst), 1);

    let surface = wait_for_surface(&core, |surface| !block_ids(surface, "sayso.app").is_empty());
    assert_eq!(surface.placeholder.as_deref(), Some("serendipity"));
    let message = status_message(&surface, "sayso.app");
    assert!(message.contains("划词翻译") && message.contains("未配置翻译模型"), "{message}");
    assert!(platform.inserted.lock().unwrap().is_empty());
}

/// 读取选区期间取消：中止平台的读取（不再等用户松开按键后模拟复制），会话回到空闲。
#[test]
fn cancelling_aborts_a_pending_selection_read() {
    let (_data, core, platform, _, _server) = fixture(None);

    let wait_until = |counter: &AtomicUsize, what: &str| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while counter.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    };

    core.trigger(RouteKey::Key { code: SELECTION_KEY });
    assert!(matches!(core.session_state(), SessionState::Processing { .. }));
    wait_until(&platform.selection_reads_started, "selection read did not start");
    core.cancel();

    wait_until(&platform.selection_reads_aborted, "selection read was not aborted");
    assert_eq!(core.session_state(), SessionState::Idle);
}

#[test]
fn concurrent_route_edits_preserve_both_commands_and_persist() {
    let (data, core, _, _, _server) = fixture(Some(""));
    let barrier = std::sync::Barrier::new(2);
    let speech = CommandKey { extension_id: "sayso.dictation".into(), command_id: "translate".into() };
    let selection = CommandKey { extension_id: "sayso.selection".into(), command_id: "translate".into() };
    let speech_route = RouteKey::Key { code: 89 };
    let selection_route = RouteKey::Key { code: 90 };
    std::thread::scope(|scope| {
        scope.spawn(|| {
            barrier.wait();
            core.bind_command(speech.clone(), Some(speech_route)).unwrap();
        });
        scope.spawn(|| {
            barrier.wait();
            core.bind_command(selection.clone(), Some(selection_route)).unwrap();
        });
    });

    core.set_route_input(speech_route, InputSource::Selection).unwrap();
    let settings = core.settings();
    assert_eq!(settings.binding(&speech), Some(speech_route));
    assert_eq!(settings.binding(&selection), Some(selection_route));
    assert_eq!(settings.route(speech_route).unwrap().input, InputSource::Selection);
    assert_eq!(settings.route(selection_route).unwrap().input, InputSource::Selection);
    assert_eq!(settings.asr_source, OPENAI_COMPATIBLE);
    let persisted = serde_json::from_slice::<sayso_core::settings::Settings>(
        &std::fs::read(data.path().join("settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(persisted, settings);
}

#[test]
fn secrets_persist_to_config_file_and_clear_on_empty() {
    let (data, core, _, _, _server) = fixture(Some(""));
    let slot = SecretSlot::Llm { provider: LlmProvider::Openai };
    let read = || {
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(data.path().join("config.json")).unwrap()).unwrap()
    };

    core.set_secret(slot.clone(), " sk-test ".into()).unwrap();
    assert_eq!(core.secret(slot.clone()), "sk-test");
    assert_eq!(read()["llm.openai"], "sk-test");

    core.set_secret(slot.clone(), "".into()).unwrap();
    assert_eq!(core.secret(slot), "");
    assert!(read().get("llm.openai").is_none());
}
