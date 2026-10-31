//! 跨语言边界集成：验证分配/释放、设置持久化、真实扩展打包安装与协议错误。
use sayso_windows_bridge::{sayso_call, sayso_create, sayso_destroy, sayso_free, Bridge};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};

extern "C" fn callback(_: *const c_char) -> *mut c_char {
    CString::new(r#"{"result":null}"#).unwrap().into_raw()
}
extern "C" fn release(value: *mut c_char) {
    unsafe {
        drop(CString::from_raw(value));
    }
}

struct Client(*mut Bridge);
impl Client {
    fn new(path: &std::path::Path) -> Self {
        let path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle = std::ptr::null_mut();
        let result = unsafe { take(sayso_create(path.as_ptr(), callback, release, &mut handle)) };
        assert!(result.get("error").is_none(), "{result}");
        Self(handle)
    }
    fn call(&self, method: &str, args: Value) -> Value {
        let request = CString::new(json!({"method":method,"args":args}).to_string()).unwrap();
        let result = unsafe { take(sayso_call(self.0, request.as_ptr())) };
        assert!(result.get("error").is_none(), "{result}");
        result["result"].clone()
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            sayso_destroy(self.0);
        }
    }
}
unsafe fn take(pointer: *mut c_char) -> Value {
    let value = serde_json::from_slice(CStr::from_ptr(pointer).to_bytes()).unwrap();
    sayso_free(pointer);
    value
}

#[test]
fn settings_survive_native_handle_recreation_and_invalid_requests_remain_errors() {
    let directory = tempfile::tempdir().unwrap();
    {
        let client = Client::new(directory.path());
        let mut settings = client.call("settings", Value::Null);
        assert_eq!(settings["triggerKey"]["keyCode"], 0xA3);
        settings["targetLanguage"] = json!("ja");
        client.call("updateSettings", settings);
        for request in ["{", r#"{"method":"unknown"}"#, r#"{"method":"updateSettings","args":{}}"#] {
            let request = CString::new(request).unwrap();
            let result = unsafe { take(sayso_call(client.0, request.as_ptr())) };
            assert!(result["error"].is_string(), "{result}");
        }
        assert_eq!(client.call("settings", Value::Null)["targetLanguage"], "ja");
    }
    let client = Client::new(directory.path());
    assert_eq!(client.call("settings", Value::Null)["targetLanguage"], "ja");
}

#[test]
fn extension_package_roundtrip_preserves_code_and_updates_core_catalog() {
    let directory = tempfile::tempdir().unwrap();
    let extension = directory.path().join("Extensions/example");
    std::fs::create_dir_all(&extension).unwrap();
    let manifest = json!({"schemaVersion":4,"id":"test.package","name":"Package test","main":"main.js","events":["translated"],"capabilities":["blocks"]});
    std::fs::write(extension.join("extension.json"), manifest.to_string()).unwrap();
    std::fs::write(
        extension.join("main.js"),
        "export function translated() { sayso.blocks.upsert({ id: 'ok', type: 'status', message: 'ok' }); }",
    )
    .unwrap();
    let client = Client::new(directory.path());
    let package = directory.path().join("example.sayso");
    client.call("exportPackage", json!({"id":"test.package","path":package}));
    client.call("uninstall", json!("test.package"));
    let staged = client.call("openPackage", json!(package));
    assert_eq!(staged["manifest"]["id"], manifest["id"]);
    assert_eq!(staged["manifest"]["events"], manifest["events"]);
    assert_eq!(staged["replacing"], false);
    client.call("installPackage", staged["token"].clone());
    let catalog = client.call("extensions", Value::Null);
    let installed = catalog.as_array().unwrap().iter().find(|e| e["manifest"]["id"] == "test.package").unwrap();
    assert_eq!(installed["enabled"], true);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("Extensions/test.package/main.js")).unwrap(),
        "export function translated() { sayso.blocks.upsert({ id: 'ok', type: 'status', message: 'ok' }); }"
    );
}

#[test]
fn routing_commands_preserve_input_and_persist_across_the_abi() {
    let directory = tempfile::tempdir().unwrap();
    let command = json!({"extensionId":"sayso.selection","commandId":"translate"});
    let destination = json!({"kind":"key","code":89});
    {
        let client = Client::new(directory.path());
        let settings = client.call("bindCommand", json!({"command":command,"route":destination}));
        let routes = settings["routes"].as_array().unwrap();
        assert_eq!(routes.iter().find(|r| r["key"] == destination).unwrap()["input"], "selection");
        assert!(!routes.iter().any(|r| r["key"]["code"] == 0x54));
        assert_eq!(client.call("activeRouteKeys", Value::Null), json!([89]));
        client.call("setRouteInput", json!({"route":destination,"input":"speech"}));
    }
    let client = Client::new(directory.path());
    let settings = client.call("settings", Value::Null);
    assert_eq!(
        settings["routes"].as_array().unwrap().iter().find(|r| r["key"] == destination).unwrap()["input"],
        "speech"
    );
    let settings = client.call("bindCommand", json!({"command":command,"route":null}));
    assert!(!settings["routes"].as_array().unwrap().iter().any(|r| r["key"] == destination));
    assert_eq!(client.call("activeRouteKeys", Value::Null), json!([]));
}
