//! 把 core（及其依赖）的 `log` 日志转交平台日志系统：macOS 写入统一日志，Windows 由宿主决定。

use std::sync::Arc;

/// 日志级别，对应 `log::Level`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// 平台日志输出，可能在任意线程上调用。
#[uniffi::export(with_foreign)]
pub trait LogSink: Send + Sync {
    fn log(&self, level: LogLevel, target: String, message: String);
}

struct Forwarder(Arc<dyn LogSink>);

impl log::Log for Forwarder {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            log::Level::Error => LogLevel::Error,
            log::Level::Warn => LogLevel::Warn,
            log::Level::Info => LogLevel::Info,
            log::Level::Debug | log::Level::Trace => LogLevel::Debug,
        };
        self.0.log(level, record.target().to_owned(), record.args().to_string());
    }

    fn flush(&self) {}
}

/// 安装全局日志输出。进程内只能安装一次，宿主应在创建 `Core` 前调用。
/// 依赖库的调试日志量很大，只转交 Info 及以上。
#[uniffi::export]
pub fn install_log_sink(sink: Arc<dyn LogSink>) -> Result<(), crate::error::CoreError> {
    log::set_boxed_logger(Box::new(Forwarder(sink)))
        .map_err(|_| crate::error::CoreError::Io("log sink already installed".into()))?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(())
}
