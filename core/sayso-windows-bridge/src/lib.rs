//! Windows 宿主的窄 C ABI。所有业务仍由 sayso-core 执行；JSON 内存只由分配方释放。
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::sync::Arc;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use sayso_core::asr::source::AsrFieldKind;
use sayso_core::plugin::RunStatus;
use sayso_core::protocol::{AppInfo, Delivery, Language};
use sayso_core::session::SessionState;
use sayso_core::settings::{Hotkey, PlatformDefaults, SecretSlot};
use sayso_core::{AppRelease, Core, CoreObserver, Platform, PlatformError};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

type Callback = extern "C" fn(*const c_char) -> *mut c_char;
type Release = extern "C" fn(*mut c_char);

struct Host {
    callback: Callback,
    release: Release,
    next_request: AtomicU64,
    pending: Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, String>>>>,
}

struct PendingCall<'a> {
    host: &'a Host,
    id: u64,
}
impl Drop for PendingCall<'_> {
    fn drop(&mut self) {
        let unfinished = self.host.pending.lock().unwrap().remove(&self.id).is_some();
        if unfinished {
            self.host.notify("cancelPlatform", json!(self.id));
        }
    }
}

impl Host {
    async fn call_async(&self, method: &str, args: Value) -> Result<Value, String> {
        let id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.pending.lock().unwrap().insert(id, sender);
        // 会话取消时 guard 随 future 丢弃，把取消传到 Windows 的剪贴板操作。
        let _guard = PendingCall { host: self, id };
        self.call("asyncPlatform", json!({"id":id,"method":method,"args":args}))?;
        receiver.await.map_err(|e| e.to_string())?
    }
    fn call(&self, method: &str, args: Value) -> Result<Value, String> {
        let request = CString::new(json!({"method": method, "args": args}).to_string()).unwrap();
        let result = (self.callback)(request.as_ptr());
        if result.is_null() {
            return Err("Windows callback returned null".into());
        }
        // 回调返回的内存在复制后立即交还 .NET，避免跨分配器释放。
        let response = unsafe { CStr::from_ptr(result) }.to_string_lossy().into_owned();
        (self.release)(result);
        let value: Value = serde_json::from_str(&response).map_err(|e| e.to_string())?;
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(error.into());
        }
        Ok(value["result"].clone())
    }
    fn notify(&self, method: &str, args: Value) {
        if let Err(error) = self.call(method, args) {
            eprintln!("{method}: {error}");
        }
    }
}

#[async_trait::async_trait]
impl Platform for Host {
    fn start_audio(&self) -> Result<(), PlatformError> {
        self.call("startAudio", Value::Null).map(|_| ()).map_err(|message| PlatformError::Failed { message })
    }
    fn stop_audio(&self) {
        self.notify("stopAudio", Value::Null);
    }
    async fn read_selection(&self) -> String {
        self.call_async("readSelection", Value::Null)
            .await
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }
    async fn insert_text(&self, text: String, destination: Option<AppInfo>) -> Result<Delivery, PlatformError> {
        let value = self
            .call_async("insertText", json!({"text":text,"destination":destination}))
            .await
            .map_err(|message| PlatformError::Failed { message })?;
        match value.as_str() {
            Some("inserted") => Ok(Delivery::Inserted),
            Some("copiedToClipboard") => Ok(Delivery::CopiedToClipboard),
            _ => Err(PlatformError::Failed { message: format!("Invalid insertText response: {value}") }),
        }
    }
    fn frontmost_app(&self) -> Option<AppInfo> {
        self.call("frontmostApp", Value::Null).ok().and_then(|v| serde_json::from_value(v).ok())
    }
    fn running_apps(&self) -> Vec<AppInfo> {
        self.call("runningApps", Value::Null).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }
    fn speak(&self, text: String, language: Language, rate: f32) -> Result<(), PlatformError> {
        self.call("speak", json!({"text":text,"language":language.speech_locale(),"rate":rate}))
            .map(|_| ())
            .map_err(|message| PlatformError::Failed { message })
    }
    fn stop_speaking(&self) {
        self.notify("stopSpeaking", Value::Null);
    }
    fn copy_text(&self, text: String) {
        self.notify("copyText", json!(text));
    }
    fn open_url(&self, url: String) {
        self.notify("openUrl", json!(url));
    }
}

fn session_json(state: SessionState) -> Value {
    match state {
        SessionState::Idle => json!({"kind":"idle"}),
        SessionState::Recording { partial, command, .. } => json!({"kind":"recording","text":partial,"command":command}),
        SessionState::Processing { message } => json!({"kind":"processing","text":message}),
        SessionState::Presenting { surface } => {
            json!({"kind":"presenting","id":surface.id,"placeholder":surface.placeholder,"working":surface.working,"footer":surface.footer,
            "sections":surface.sections.into_iter().map(|s| json!({"sourceId":s.source_id,"title":s.title,"blocks":s.blocks})).collect::<Vec<_>>() })
        }
    }
}

impl CoreObserver for Host {
    fn session_changed(&self, state: SessionState) {
        self.notify("sessionChanged", session_json(state));
    }
    fn extensions_changed(&self) {
        self.notify("extensionsChanged", Value::Null);
    }
    fn store_changed(&self, extension_id: String) {
        self.notify("storeChanged", json!(extension_id));
    }
    fn settings_changed(&self) {
        self.notify("settingsChanged", Value::Null);
    }
    fn update_available(&self, release: AppRelease) {
        self.notify("updateAvailable", release_json(Some(release)));
    }
}

fn release_json(release: Option<AppRelease>) -> Value {
    release.map_or(Value::Null, |release| json!({"version":release.version,"url":release.url}))
}

pub struct Bridge {
    core: Arc<Core>,
    runtime: tokio::runtime::Runtime,
    host: Arc<Host>,
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| e.to_string())
}
fn text(value: &Value, key: &str) -> Result<String, String> {
    value[key].as_str().map(str::to_owned).ok_or_else(|| format!("Missing string: {key}"))
}

impl Bridge {
    fn invoke(&self, method: &str, args: Value) -> Result<Value, String> {
        let core = &self.core;
        match method {
            "settings" => Ok(json!(core.settings())),
            "updateSettings" => { core.update_settings(decode(args)?).map_err(|e| e.to_string())?; Ok(json!(core.settings())) }
            "setShortcut" => {
                core.set_shortcut(decode(args["input"].clone())?, decode(args["key"].clone())?).map_err(|e| e.to_string())?;
                Ok(json!(core.settings()))
            }
            "session" => Ok(session_json(core.session_state())),
            "activeShortcuts" => Ok(json!(core.active_shortcuts())),
            "trigger" => { core.trigger(decode(args)?); Ok(Value::Null) }
            "toggle" => { core.toggle(decode(args)?); Ok(Value::Null) }
            "stop" => { core.stop(); Ok(Value::Null) }
            "cancel" => { core.cancel(); Ok(Value::Null) }
            "dismiss" => { core.dismiss(); Ok(Value::Null) }
            "dismissSurface" => { core.dismiss_surface(decode(args)?); Ok(Value::Null) }
            "hover" => { core.set_hovering(decode(args)?); Ok(Value::Null) }
            "languages" => Ok(json!(Language::ALL.map(|l| json!({"value":l,"title":l.native_name()})))),
            "providers" => Ok(json!(sayso_core::llm::LlmProvider::ALL.map(|p| json!({"value":p,"title":p.display_name(),"requiresKey":p.requires_api_key()})))),
            "asrSources" => Ok(json!(core.asr_sources().into_iter().map(|s| json!({"id":s.id,"title":s.title,"caption":s.caption,"languages":s.languages,
                "link":s.link.map(|l|json!({"title":l.title,"url":l.url})),
                "fields":s.fields.into_iter().map(|f| {
                    let (kind, options) = match f.kind {
                        AsrFieldKind::Secret => ("secret",vec![]), AsrFieldKind::Text => ("text",vec![]),
                        AsrFieldKind::Choice{options} => ("choice",options.into_iter().map(|o|json!({"value":o.value,"title":o.title})).collect()),
                    };
                    json!({"key":f.key,"title":f.title,"kind":kind,"required":f.required,"options":options,"caption":f.caption})
                }).collect::<Vec<_>>() })).collect::<Vec<_>>())),
            "asrValue" => Ok(json!(core.asr_value(text(&args,"source")?,text(&args,"field")?))),
            "setAsrValue" => { core.set_asr_value(text(&args,"source")?,text(&args,"field")?,text(&args,"value")?).map_err(|e|e.to_string())?; Ok(Value::Null) }
            "secret" => Ok(json!(core.secret(SecretSlot::Llm{provider:decode(args)?}))),
            "setSecret" => { core.set_secret(SecretSlot::Llm{provider:decode(args["provider"].clone())?},text(&args,"value")?).map_err(|e|e.to_string())?; Ok(Value::Null) }
            "testLlm" => self.runtime.block_on(core.test_llm()).map(|v|json!(v)).map_err(|e|e.to_string()),
            "appVersion" => Ok(json!(core.app_version())),
            "availableUpdate" => Ok(release_json(core.available_update())),
            "checkForUpdate" => self.runtime.block_on(core.check_for_update()).map(release_json).map_err(|e|e.to_string()),
            "startUpdateChecks" => { core.start_update_checks(); Ok(Value::Null) }
            "extensions" => Ok(json!(core.extensions().into_iter().map(|e| {
                let result=e.last_result.map(|r| {
                    let (status,message)=match r.status {
                        RunStatus::Succeeded => ("succeeded",None), RunStatus::Failed{message} => ("failed",Some(message)), RunStatus::TimedOut => ("timedOut",None),
                    };
                    json!({"status":status,"message":message,"warnings":r.warnings,"log":r.log})
                });
                json!({"manifest":e.manifest,"builtIn":e.built_in,"enabled":e.enabled,"permissions":e.permission_summary,"listens":e.listens,"emits":e.emits,"lastResult":result})
            }).collect::<Vec<_>>())),
            "extensionProblems" => Ok(json!(core.extension_problems().into_iter().map(|p|json!({"directory":p.directory_name,"reason":p.reason})).collect::<Vec<_>>())),
            "extensionsDirectory" => Ok(json!(core.extensions_directory())),
            "setEnabled" => { core.set_enabled(text(&args,"id")?,decode(args["enabled"].clone())?).map_err(|e|e.to_string())?; Ok(Value::Null) }
            "reloadExtensions" => { core.reload_extensions(); Ok(Value::Null) }
            "tabItems" => { let tab=core.tab_items(text(&args,"id")?,text(&args,"query")?).map_err(|e|e.to_string())?;
                Ok(json!({"total":tab.total,"items":tab.items.into_iter().map(|i|json!({"key":i.key,"block":i.block})).collect::<Vec<_>>()})) }
            "tabAction" => { core.perform_tab_action(text(&args,"id")?,text(&args,"key")?,decode(args["action"].clone())?); Ok(Value::Null) }
            "candidateAction" => { core.perform_candidate_action(text(&args,"surface")?,text(&args,"source")?,text(&args,"block")?,decode(args["action"].clone())?); Ok(Value::Null) }
            "appFocused" => { core.app_focused(decode(args)?); Ok(Value::Null) }
            "appTable" => core.app_table(decode(args)?).map(|table| json!(table)).map_err(|e|e.to_string()),
            "addAppRow" => { core.add_app_row(decode(args)?).map_err(|e|e.to_string())?; Ok(Value::Null) }
            "saveAppRow" => { core.save_app_row(text(&args,"id")?,text(&args,"key")?,decode(args["app"].clone())?,text(&args,"text")?).map_err(|e|e.to_string())?; Ok(Value::Null) }
            "deleteRecord" => { core.delete_record(text(&args,"id")?,text(&args,"key")?).map_err(|e|e.to_string())?; Ok(Value::Null) }
            "openPackage" => { let info=self.runtime.block_on(core.open_package(decode(args)?)).map_err(|e|e.to_string())?;
                Ok(json!({"token":info.token,"manifest":info.manifest,"bytes":info.byte_size,"replacing":info.replacing,"permissions":info.permission_summary})) }
            "installPackage" => { core.install_package(decode(args)?).map_err(|e|e.to_string())?; Ok(Value::Null) }
            "discardPackage" => { core.discard_package(decode(args)?); Ok(Value::Null) }
            "exportPackage" => { self.runtime.block_on(core.export_package(text(&args,"id")?,text(&args,"path")?)).map_err(|e|e.to_string())?; Ok(Value::Null) }
            "uninstall" => { core.uninstall(decode(args)?).map_err(|e|e.to_string())?; Ok(Value::Null) }
            _ => Err(format!("Unknown bridge method: {method}")),
        }
    }
}

fn response(result: Result<Value, String>) -> *mut c_char {
    let value = match result {
        Ok(value) => json!({"result":value}),
        Err(error) => json!({"error":error}),
    };
    CString::new(value.to_string()).unwrap().into_raw()
}

/// 返回错误响应或句柄；调用方必须始终用 sayso_free 释放响应。
#[no_mangle]
pub unsafe extern "C" fn sayso_create(
    directory: *const c_char,
    version: *const c_char,
    callback: Callback,
    release: Release,
    output: *mut *mut Bridge,
) -> *mut c_char {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let host = Arc::new(Host { callback, release, next_request: AtomicU64::new(1), pending: Mutex::default() });
        let core = Core::new(
            CStr::from_ptr(directory).to_string_lossy().into_owned(),
            CStr::from_ptr(version).to_string_lossy().into_owned(),
            PlatformDefaults {
                speech_shortcut: Hotkey { key_code: 0xA3, modifiers: 0 },
                selection_shortcut: Hotkey { key_code: 0x54, modifiers: 1 },
                speech_rate: 0.5,
            },
            host.clone(),
            host.clone(),
        )
        .map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        *output = Box::into_raw(Box::new(Bridge { core, runtime, host }));
        Ok(Value::Null)
    }))
    .unwrap_or_else(|_| Err("Core initialization panicked".into()));
    response(result)
}

#[no_mangle]
pub unsafe extern "C" fn sayso_call(bridge: *const Bridge, request: *const c_char) -> *mut c_char {
    let bridge = &*bridge;
    response(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let value: Value = serde_json::from_slice(CStr::from_ptr(request).to_bytes()).map_err(|e| e.to_string())?;
            bridge.invoke(&text(&value, "method")?, value["args"].clone())
        }))
        .unwrap_or_else(|_| Err("Core request panicked".into())),
    )
}

#[no_mangle]
pub unsafe extern "C" fn sayso_audio(bridge: *const Bridge, data: *const u8, length: usize) {
    let bridge = &*bridge;
    bridge.core.push_audio(std::slice::from_raw_parts(data, length).to_vec());
}

#[no_mangle]
pub unsafe extern "C" fn sayso_complete(bridge: *const Bridge, id: u64, response: *const c_char) {
    let bridge = &*bridge;
    let value: Result<Value, String> =
        serde_json::from_slice(CStr::from_ptr(response).to_bytes()).map_err(|e| e.to_string());
    let result = value.and_then(|v| match v.get("error").and_then(Value::as_str) {
        Some(error) => Err(error.to_owned()),
        None => Ok(v["result"].clone()),
    });
    if let Some(sender) = bridge.host.pending.lock().unwrap().remove(&id) {
        let _ = sender.send(result);
    }
}

#[no_mangle]
pub unsafe extern "C" fn sayso_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[no_mangle]
pub unsafe extern "C" fn sayso_destroy(bridge: *mut Bridge) {
    if !bridge.is_null() {
        drop(Box::from_raw(bridge));
    }
}
