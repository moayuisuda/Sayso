//! 输入会话的端到端行为：用伪造的平台与子进程识别器驱动 `Core`，断言路由、写入与候选框内容。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use sayso_core::protocol::{AppInfo, BlockContent, Delivery, InputSource, Language};
use sayso_core::session::{SessionState, SurfaceView};
use sayso_core::settings::{CommandBinding, CommandKey, Hotkey, PlatformDefaults, Route, RouteKey};
use sayso_core::{Core, CoreObserver, Platform, PlatformError};

const SELECTION_KEY: u16 = 17;
const ECHO_KEY: u16 = 5;

#[derive(Default)]
struct FakePlatform {
    /// 选中的文本；None 表示读取一直等待（如用户还按着按键），直到被取消。
    selection: Option<String>,
    /// 开始与被中止的选区读取次数。
    selection_reads_started: AtomicUsize,
    selection_reads_aborted: Arc<AtomicUsize>,
    inserted: Mutex<Vec<(String, Option<AppInfo>)>>,
    audio_stops: AtomicUsize,
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
    async fn insert_text(&self, text: String, destination: Option<AppInfo>) -> Delivery {
        self.inserted.lock().unwrap().push((text, destination));
        Delivery::Inserted
    }
    fn frontmost_app(&self) -> Option<AppInfo> {
        Some(AppInfo { id: Some("com.example.notes".into()), name: Some("Notes".into()) })
    }
    fn speak(&self, _text: String, _language: Language, _rate: f32) {}
    fn stop_speaking(&self) {}
    fn copy_text(&self, _text: String) {}
    fn open_url(&self, _url: String) {}
    fn secret_read(&self, _account: String) -> Option<String> {
        None
    }
    fn secret_write(&self, _account: String, _value: Option<String>) {}
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

/// 数据目录中装好两个测试扩展：识别结果固定的识别器，以及把输入原样显示出来的 echo 命令。
fn fixture(selection: Option<&str>) -> (tempfile::TempDir, Core, Arc<FakePlatform>, Arc<RecordingObserver>) {
    let data = tempfile::tempdir().unwrap();
    let install = |name: &str, manifest: &str, files: &[(&str, &str)]| {
        let directory = data.path().join("Extensions").join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("extension.json"), manifest).unwrap();
        for (file, content) in files {
            std::fs::write(directory.join(file), content).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(directory.join(file), std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    };
    install(
        "asr",
        r#"{"schemaVersion":3,"id":"test.asr","name":"ASR","capabilities":["audio"],"recognizer":{"title":"Fixed","command":["./run.sh"]}}"#,
        &[(
            "run.sh",
            "#!/bin/sh\nwhile read -r line; do\n  case \"$line\" in *'\"type\":\"end\"'*) echo '{\"type\":\"final\",\"text\":\"I have a 苹果.\"}'; exit 0 ;; esac\ndone\n",
        )],
    );
    install(
        "echo",
        r#"{"schemaVersion":3,"id":"test.echo","name":"Echo","main":"main.js","events":["command"],
            "capabilities":["blocks"],"commands":[{"id":"show","title":"Show"}]}"#,
        &[(
            "main.js",
            r#"export function command(event) {
                 sayso.blocks.upsert({ id: "echo", type: "status", message: `${event.input.via}: ${event.input.text}` });
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
    let core = Core::new(
        data.path().to_string_lossy().into(),
        defaults,
        platform.clone(),
        observer.clone(),
    )
    .unwrap();
    let core = Arc::try_unwrap(core).ok().unwrap();

    let mut settings = core.settings();
    settings.asr_source = "test.asr".into();
    settings.enabled_extensions.extend(["test.asr".to_string(), "test.echo".to_string()]);
    settings.routes.push(Route { key: RouteKey::Key { code: ECHO_KEY }, input: InputSource::Speech });
    settings.command_bindings.push(CommandBinding {
        command: CommandKey { extension_id: "test.echo".into(), command_id: "show".into() },
        route: RouteKey::Key { code: ECHO_KEY },
    });
    core.update_settings(settings).unwrap();
    (data, core, platform, observer)
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

/// 直接路由：录音 → 扩展识别器 → 语音翻译。没有配置翻译模型时原样写入触发时的前台应用，并提示原因。
#[test]
fn speech_flow_inserts_transcript_into_the_triggering_app() {
    let (_data, core, platform, observer) = fixture(Some(""));

    core.trigger(RouteKey::Direct);
    assert_eq!(
        core.session_state(),
        SessionState::Recording {
            partial: String::new(), commands: vec!["翻译并输入 / Translate and type".into()]
        }
    );
    core.push_audio(vec![0; 320]);
    core.stop();

    let surface = wait_for_surface(&core, |surface| block_ids(surface, "sayso.dictation").len() == 2);
    assert_eq!(block_ids(&surface, "sayso.dictation"), ["text", "notice"]);
    let inserted = platform.inserted.lock().unwrap().clone();
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

/// 录音中按语音路由键：本次只交给该路由的命令，不再交给「直接」路由。
#[test]
fn speech_route_key_redirects_the_recording() {
    let (_data, core, platform, _) = fixture(Some(""));

    core.trigger(RouteKey::Direct);
    core.trigger(RouteKey::Key { code: ECHO_KEY });
    assert!(matches!(core.session_state(), SessionState::Recording { commands, .. } if commands == ["Show"]));
    core.stop();

    let surface = wait_for_surface(&core, |surface| !block_ids(surface, "test.echo").is_empty());
    assert_eq!(status_message(&surface, "test.echo"), "speech: I have a 苹果.");
    assert!(block_ids(&surface, "sayso.dictation").is_empty());
    assert!(platform.inserted.lock().unwrap().is_empty());
}

/// 录音中按选区路由键：放弃录音，改为读取选区交给划词翻译；翻译失败的原因显示在候选框中。
#[test]
fn selection_route_key_abandons_recording_and_reads_selection() {
    let (_data, core, platform, _) = fixture(Some("serendipity"));

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
    let (_data, core, platform, _) = fixture(None);

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
