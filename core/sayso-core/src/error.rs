use crate::l10n::tr;
use crate::protocol::Capability;

/// core 对外的错误。消息直接显示给用户，按本地语言输出。
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum CoreError {
    #[error("{0}")]
    InvalidManifest(String),
    #[error("Invalid instruction: {0}")]
    InvalidInstruction(String),
    #[error("Capability not declared: {}", .0.name())]
    CapabilityNotGranted(Capability),
    #[error("{}", tr("未配置翻译模型", "No translation model configured"))]
    ModelNotConfigured,
    #[error("insert is only allowed while handling the input event")]
    InsertOutsideInput,
    #[error("Invalid extension package: {0}")]
    InvalidPackage(String),
    #[error("{0}")]
    Llm(String),
    #[error("{0}")]
    Recognition(String),
    #[error("{0}")]
    Platform(#[from] crate::platform::PlatformError),
    #[error("{0}")]
    Update(String),
    #[error("Shell: {0}")]
    Shell(String),
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for CoreError {
    fn from(error: std::io::Error) -> Self {
        CoreError::Io(error.to_string())
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(error: serde_json::Error) -> Self {
        CoreError::Io(error.to_string())
    }
}
