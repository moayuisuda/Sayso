//! Sayso core：两个平台共用的会话、插件运行时、数据存储、模型与识别服务。
//! 平台宿主通过 UniFFI 调用 [`Core`]，并实现 [`Platform`] 与 [`CoreObserver`] 提供平台能力。

uniffi::setup_scaffolding!();

mod app;
pub mod asr;
pub mod error;
pub mod extensions;
pub mod l10n;
pub mod llm;
mod logging;
mod package;
pub mod platform;
pub mod plugin;
pub mod protocol;
pub mod session;
pub mod settings;
pub mod store;
mod time_util;

pub use app::{Core, PackageInfo, TabItem, TabItems};
pub use logging::{LogLevel, LogSink};
pub use platform::{CoreObserver, Platform, PlatformError};
