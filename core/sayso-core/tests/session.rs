//! 输入会话的端到端行为：用测试平台与本地 HTTP 识别服务驱动 `Core`，断言快捷键触发、写入与候选框内容。
mod common;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use sayso_core::asr::source::OPENAI_COMPATIBLE;
use sayso_core::llm::LlmProvider;
use sayso_core::protocol::{
    ActionKind, AppInfo, BlockAction, BlockContent, Delivery, EventType, InputSource, Language,
};
use sayso_core::session::{SessionState, SurfaceView};
use sayso_core::settings::{Hotkey, PlatformDefaults, SecretSlot};
use sayso_core::{AppRelease, Core, CoreObserver, Platform, PlatformError, DEFAULT_ROW};

const SPEECH: Hotkey = Hotkey { key_code: 61, modifiers: 0 };
const SELECTION: Hotkey = Hotkey { key_code: 17, modifiers: 0x80000 };

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
    fn running_apps(&self) -> Vec<AppInfo> {
        vec![]
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
    extension_updates: AtomicUsize,
}

impl CoreObserver for RecordingObserver {
    fn session_changed(&self, state: SessionState) {
        self.states.lock().unwrap().push(state);
    }
    fn extensions_changed(&self) {
        self.extension_updates.fetch_add(1, Ordering::SeqCst);
    }
    fn store_changed(&self, _extension_id: String) {}
    fn settings_changed(&self) {}
    fn update_available(&self, _release: AppRelease) {}
}

/// 在真实 TCP 连接上检查批量识别契约；Drop 唤醒 accept，避免未使用的服务线程泄漏。
struct LocalAsr {
    address: SocketAddr,
    stopping: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    /// 每次识别请求的完整表单。
    forms: Arc<Mutex<Vec<String>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl LocalAsr {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stopping = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let forms = Arc::new(Mutex::new(Vec::new()));
        let worker = {
            let stopping = stopping.clone();
            let requests = requests.clone();
            let forms = forms.clone();
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
                    forms.lock().unwrap().push(multipart.into_owned());
                    requests.fetch_add(1, Ordering::SeqCst);
                    let response = r#"{"text":"I have a 苹果."}"#;
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
                }
            })
        };
        Self { address, stopping, requests, forms, worker: Some(worker) }
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

    // 第三方后处理扩展：按测试需要启用。
    install(
        "glossary",
        r#"{"schemaVersion":4,"id":"test.glossary","name":"Glossary","main":"main.js","events":["input"],
            "capabilities":["blocks","publish","insert"]}"#,
        &[(
            "main.js",
            r#"export async function input(event) {
                 sayso.blocks.upsert({ id: "gloss", type: "text", runs: [{ text: "释义" }] });
                 if (event.input.via === "speech") await sayso.insert("第三方处理结果");
                 sayso.publish({ type: "translated", output: "释义", pairs: [{ target: event.input.text, native: "释义" }], card: "gloss" });
               }"#,
        )],
    );

    let platform = Arc::new(FakePlatform { selection: selection.map(Into::into), ..Default::default() });
    let observer = Arc::new(RecordingObserver::default());
    let defaults = PlatformDefaults { speech_shortcut: SPEECH, selection_shortcut: SELECTION, speech_rate: 0.5 };
    let core =
        Core::new(data.path().to_string_lossy().into(), "0.0.0".into(), defaults, platform.clone(), observer.clone())
            .unwrap();
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
        .flat_map(|section| section.blocks.iter().map(|item| item.block.id.clone()))
        .collect()
}

fn status_message(surface: &SurfaceView, source: &str) -> String {
    let section = surface.sections.iter().find(|section| section.source_id == source).expect("section exists");
    let BlockContent::Status { message, .. } = &section.blocks[0].block.content else {
        panic!("expected status block")
    };
    message.clone()
}

/// 上报前台应用，等扩展处理完。
fn focus(core: &Core, observer: &RecordingObserver, app: Option<AppInfo>) {
    let before = observer.extension_updates.load(Ordering::SeqCst);
    core.app_focused(app);
    let deadline = Instant::now() + Duration::from_secs(10);
    while observer.extension_updates.load(Ordering::SeqCst) == before {
        assert!(Instant::now() < deadline, "focus event was not handled");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 说一句话，返回发给识别服务的表单。
fn dictate(core: &Core, server: &LocalAsr) -> String {
    core.trigger(SPEECH);
    core.push_audio(vec![0; 16000]);
    core.stop();
    wait_for_surface(core, |surface| !block_ids(surface, "sayso.dictation").is_empty());
    server.forms.lock().unwrap().last().expect("speech service was called").clone()
}

/// 背景按前台应用切换：有对应行时默认行之后接上该行，切到其他应用时只剩默认行，改表后立即生效。
#[test]
fn speech_recognition_uses_the_context_of_the_focused_app() {
    let (_data, core, _platform, observer, server) = fixture(Some(""));
    let xcode = AppInfo { id: Some("com.apple.dt.Xcode".into()), name: Some("Xcode".into()) };
    let notes = AppInfo { id: Some("com.apple.Notes".into()), name: Some("Notes".into()) };
    let save = |key: &str, app: Option<AppInfo>, text: &str| {
        core.save_app_row("sayso.context".into(), key.into(), app, text.into()).unwrap();
    };
    save(DEFAULT_ROW, None, "default background");
    save("xcode", Some(xcode.clone()), "swift background");

    focus(&core, &observer, Some(xcode));
    assert!(dictate(&core, &server).contains("default background\nswift background"));

    focus(&core, &observer, Some(notes));
    let form = dictate(&core, &server);
    assert!(form.contains("default background") && !form.contains("swift background"));

    let before = observer.extension_updates.load(Ordering::SeqCst);
    save(DEFAULT_ROW, None, "edited background");
    while observer.extension_updates.load(Ordering::SeqCst) == before {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(dictate(&core, &server).contains("edited background"));
}

/// 空表显示一条未保存的空行；点加号时它要留下，新行排在后面，删光后又回到一条空行。
#[test]
fn app_table_keeps_the_blank_row_when_adding_and_returns_to_it_when_emptied() {
    let (_data, core, _platform, _observer, _server) = fixture(Some(""));
    let extension = || "sayso.context".to_string();
    let keys = || core.app_table(extension()).unwrap().rows.into_iter().map(|row| row.key).collect::<Vec<_>>();
    let blank = keys();
    assert_eq!(blank.len(), 1);

    core.add_app_row(extension()).unwrap();
    let rows = keys();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], blank[0]);

    for key in rows {
        core.delete_record(extension(), key).unwrap();
    }
    assert_eq!(keys(), blank);
}

/// 语音快捷键：录音 → 本地 HTTP 服务 → 混说翻译。没有配置翻译模型时原样写入触发时的前台应用，并提示原因。
#[test]
fn speech_flow_inserts_transcript_into_the_triggering_app() {
    let (_data, core, platform, observer, server) = fixture(Some(""));

    core.trigger(SPEECH);
    assert_eq!(
        core.session_state(),
        SessionState::Recording {
            partial: String::new(), command: "语音输入".into(), shortcut: SPEECH
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

    // 松开后从「处理中」直接过渡：候选框第一次出现时已登记所有订阅者。
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
    core.trigger(SPEECH);
    core.push_audio(vec![0; 16000]);
    core.stop();

    let surface =
        wait_for_surface(&core, |surface| !surface.working && block_ids(surface, "sayso.dictation").len() == 3);
    assert_eq!(block_ids(&surface, "sayso.dictation"), ["text", "insertion", "notice"]);
    assert!(platform.inserted.lock().unwrap().is_empty());
    let blocks = &surface.sections.iter().find(|section| section.source_id == "sayso.dictation").unwrap().blocks;
    let BlockContent::Text { runs, .. } = &blocks[0].block.content else { panic!("expected retained transcript") };
    assert_eq!(runs.iter().map(|run| run.text.as_str()).collect::<String>(), "I have a 苹果.");
    let BlockContent::Status { message, .. } = &blocks[1].block.content else { panic!("expected insertion error") };
    assert!(message.contains("Clipboard snapshot failed"));
}

/// 关闭事件只作用于对应结果框，迟到事件不得打断下一轮录音或识别。
#[test]
fn panel_dismissal_ignores_stale_ids_and_never_cancels_recording_or_processing() {
    let (_data, core, platform, observer, _server) = fixture(Some("괜찮아"));
    core.trigger(SELECTION);
    let first = wait_for_surface(&core, |surface| !surface.working);
    let updates = observer.states.lock().unwrap().len();
    core.set_hovering(true);
    assert_eq!(observer.states.lock().unwrap().len(), updates, "hover must not publish unchanged content");
    core.dismiss_surface(first.id.clone());
    assert_eq!(core.session_state(), SessionState::Idle);

    core.trigger(SELECTION);
    let second = wait_for_surface(&core, |surface| !surface.working);
    assert_ne!(first.id, second.id);
    let updates = observer.states.lock().unwrap().len();
    core.dismiss_surface(first.id);
    assert_eq!(observer.states.lock().unwrap().len(), updates, "stale dismissals must not publish state");
    assert!(matches!(core.session_state(), SessionState::Presenting { surface } if surface.id == second.id));
    core.trigger(SPEECH);
    core.dismiss_surface(second.id.clone());
    assert!(matches!(core.session_state(), SessionState::Recording { .. }));
    assert_eq!(platform.audio_stops.load(Ordering::SeqCst), 0);
    core.cancel();

    // 未完成的选区读取保证停在 Processing，覆盖迟到的鼠标事件与 Enter。
    let (_data, processing, _, _, _server) = fixture(None);
    processing.trigger(SELECTION);
    processing.dismiss_surface(second.id);
    assert!(matches!(processing.session_state(), SessionState::Processing { .. }));
    processing.cancel();
}

/// 关闭或替换候选框应取消对应的 shell，而不是让脚本继续执行到事件超时。
#[test]
fn closing_or_replacing_a_surface_stops_its_shell() {
    common::install_shell_executable();
    for replace in [false, true] {
        let (data, core, _, _, _server) = fixture(Some("hello"));
        let directory = data.path().join("Extensions/shell");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("extension.json"),
            r#"{
            "schemaVersion":4,"id":"test.shell","name":"Shell","main":"main.js",
            "events":["input"],"capabilities":["shell"],"timeoutSeconds":10
        }"#,
        )
        .unwrap();
        std::fs::write(
            directory.join("main.js"),
            r#"
            export async function input() {
              await sayso.shell.run("echo started > started.txt; sleep 10; echo late > late.txt");
            }
        "#,
        )
        .unwrap();
        core.reload_extensions();
        core.set_enabled("test.shell".into(), true).unwrap();
        let key = Hotkey { key_code: 70, modifiers: 1 };
        core.set_shortcut(InputSource::Selection, Some(key)).unwrap();
        core.trigger(key);
        let surface = wait_for_surface(&core, |surface| surface.working && directory.join("started.txt").exists());
        if replace {
            core.set_enabled("test.shell".into(), false).unwrap();
            core.trigger(key);
        } else {
            core.dismiss_surface(surface.id);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let result = core
                .extensions()
                .into_iter()
                .find(|extension| extension.manifest.id == "test.shell")
                .unwrap()
                .last_result;
            if let Some(result) = result {
                assert!(
                    matches!(result.status, sayso_core::plugin::RunStatus::Failed { ref message } if message.contains("cancelled")),
                    "{result:?}"
                );
                break;
            }
            assert!(Instant::now() < deadline, "closing the surface did not stop its shell");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!directory.join("late.txt").exists());
    }
}

/// 再按语音键结束录音；同一条识别结果同时投递给两个已启用的订阅者。
#[test]
fn toggling_a_speech_shortcut_stops_the_recording() {
    let (_data, core, platform, _, _server) = fixture(Some(""));

    core.toggle(SPEECH);
    assert!(matches!(core.session_state(), SessionState::Recording { command, .. }
        if command == "语音输入"));
    core.set_enabled("test.glossary".into(), true).unwrap();
    core.push_audio(vec![0; 16000]);
    core.toggle(SPEECH);

    let surface = wait_for_surface(&core, |surface| !surface.working && !block_ids(surface, "test.glossary").is_empty());
    assert_eq!(status_message(&surface, "test.echo"), "speech: I have a 苹果.");
    assert!(!block_ids(&surface, "sayso.dictation").is_empty());
    let inserted = platform.inserted.lock().unwrap();
    assert_eq!(inserted.len(), 2);
    assert!(inserted.iter().any(|(text, _)| text == "第三方处理结果"));
}

/// 播放失败应保留原文并提示；自动朗读和按钮共用同一通知，成功重试清除通知。
#[test]
fn speech_failure_preserves_content_and_retry_clears_the_notice() {
    let (_data, core, platform, _, _server) = fixture(Some("괜찮아"));
    *platform.speech_error.lock().unwrap() = Some("ko-KR voice is not installed".into());
    core.trigger(SELECTION);
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
                disabled: false,
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
    core.trigger(SPEECH);
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
            disabled: false,
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

/// 录音中按选区快捷键：放弃录音，改为读取选区交给互译；翻译失败的原因显示在候选框中。
#[test]
fn selection_shortcut_abandons_recording_and_reads_selection() {
    let (_data, core, platform, _, _server) = fixture(Some("serendipity"));

    core.trigger(SPEECH);
    core.trigger(SELECTION);
    assert_eq!(platform.audio_stops.load(Ordering::SeqCst), 1);

    let surface = wait_for_surface(&core, |surface| !block_ids(surface, "sayso.app").is_empty());
    assert_eq!(surface.placeholder.as_deref(), Some("serendipity"));
    let message = status_message(&surface, "sayso.app");
    assert!(message.contains("互译") && message.contains("未配置翻译模型"), "{message}");
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

    core.trigger(SELECTION);
    assert!(matches!(core.session_state(), SessionState::Processing { .. }));
    wait_until(&platform.selection_reads_started, "selection read did not start");
    core.cancel();

    wait_until(&platform.selection_reads_aborted, "selection read was not aborted");
    assert_eq!(core.session_state(), SessionState::Idle);
}

#[test]
fn concurrent_input_shortcut_edits_persist() {
    let (data, core, _, _, _server) = fixture(Some(""));
    let barrier = std::sync::Barrier::new(2);
    let speech_key = Hotkey { key_code: 89, modifiers: 0 };
    let selection_key = Hotkey { key_code: 90, modifiers: 0 };
    std::thread::scope(|scope| {
        scope.spawn(|| {
            barrier.wait();
            core.set_shortcut(InputSource::Speech, Some(speech_key)).unwrap();
        });
        scope.spawn(|| {
            barrier.wait();
            core.set_shortcut(InputSource::Selection, Some(selection_key)).unwrap();
        });
    });

    let settings = core.settings();
    assert_eq!(settings.input_for(speech_key), Some(InputSource::Speech));
    assert_eq!(settings.input_for(selection_key), Some(InputSource::Selection));
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

/// 第三方扩展订阅 input 后与内置翻译一样：广播的词对由 Sayso 画成词条卡片，生词本照常附加按钮。
#[test]
fn selection_broadcast_reaches_built_in_and_external_extensions() {
    let (_data, core, _, _, _server) = fixture(Some("serendipity"));
    core.set_enabled("test.glossary".into(), true).unwrap();
    assert!(core.active_shortcuts().contains(&SELECTION));

    core.trigger(SELECTION);
    let surface = wait_for_surface(&core, |surface| !surface.working);
    // 内置互译也收到同一事件；测试环境没有配置翻译模型，因此它报告缺少配置。
    let error = status_message(&surface, "sayso.app");
    assert!(error.contains("互译") && error.contains("未配置翻译模型"), "{error}");
    let section = surface.sections.iter().find(|section| section.source_id == "test.glossary").unwrap();
    let card = section.blocks.iter().find(|item| item.block.id == "@term-0").expect("Sayso draws the term card");
    assert_eq!(card.attachments[0].source_id, "sayso.wordbook");
    assert_eq!(card.attachments[0].block_ref, "test.glossary/@term-0");
    assert_eq!(surface.footer.len(), 0, "a single pair offers no add-all button");
}

/// 扩展管理页的串联关系：广播 translated 的扩展与订阅它的扩展互相可见，没有扩展订阅的 inserted 不算关系。
#[test]
fn extensions_show_which_hooks_link_them() {
    let (_data, core, _, _, _server) = fixture(None);
    core.set_enabled("test.glossary".into(), true).unwrap();
    let info = |id: &str| core.extensions().into_iter().find(|info| info.manifest.id == id).unwrap();
    let names = |links: &[sayso_core::extensions::HookLink], event| {
        links.iter().find(|link| link.event == event).map(|link| link.extensions.clone()).unwrap_or_default()
    };

    let dictation = info("sayso.dictation");
    assert_eq!(dictation.emits.iter().map(|link| link.event).collect::<Vec<_>>(), [EventType::Translated]);
    assert_eq!(names(&dictation.emits, EventType::Translated), ["生词本 / Wordbook"]);
    let wordbook = info("sayso.wordbook");
    let sources = names(&wordbook.listens, EventType::Translated);
    assert!(["混说翻译 / Mixed-language Translation", "互译 / Two-way Translation", "Glossary"]
        .iter()
        .all(|name| sources.contains(&name.to_string())));
    assert!(wordbook.emits.is_empty());
}
