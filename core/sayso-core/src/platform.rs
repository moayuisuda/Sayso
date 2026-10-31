//! 平台适配层：各平台各自实现的能力（录音、选区、写入前台应用、朗读、安全存储）与状态推送。
//! macOS 由 Swift 实现，Windows 将由 C# 实现；core 只通过这里的接口使用它们。

use crate::protocol::{AppInfo, Delivery, Language};

/// 平台能力调用失败，消息直接显示给用户。
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PlatformError {
    #[error("{message}")]
    Failed { message: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for PlatformError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        PlatformError::Failed { message: error.reason }
    }
}

/// 平台能力。同步方法可能在任意线程上调用，需要主线程的实现自行切换。
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait Platform: Send + Sync {
    /// 开始录音：16kHz 单声道 16 位小端 PCM，经 `Core::push_audio` 按序送回。
    fn start_audio(&self) -> Result<(), PlatformError>;
    /// 停止录音。返回前须把录音尾部数据经 `Core::push_audio` 送完。
    fn stop_audio(&self);
    /// 读取前台应用中选中的文本；没有选区时为空字符串。
    async fn read_selection(&self) -> String;
    /// 把文字写入目标应用；没有可写入的位置时只放入剪贴板。
    async fn insert_text(&self, text: String, destination: Option<AppInfo>) -> Delivery;
    fn frontmost_app(&self) -> Option<AppInfo>;
    fn speak(&self, text: String, language: Language, rate: f32);
    fn stop_speaking(&self);
    fn copy_text(&self, text: String);
    fn open_url(&self, url: String);
    fn secret_read(&self, account: String) -> Option<String>;
    /// 写入密钥；`None` 删除。
    fn secret_write(&self, account: String, value: Option<String>);
}

/// core 状态变化的推送。回调在 core 的内部线程上同步发生并保持顺序：
/// 实现方只能把数据转交给 UI 线程，不能在回调中同步调用 `Core`。
#[uniffi::export(with_foreign)]
pub trait CoreObserver: Send + Sync {
    fn session_changed(&self, state: crate::session::SessionState);
    /// 扩展列表、启停状态或最近一次执行结果变化。
    fn extensions_changed(&self);
    /// 某个扩展的数据变化，页签需要刷新。
    fn store_changed(&self, extension_id: String);
    fn settings_changed(&self);
}
