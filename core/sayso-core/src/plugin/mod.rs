//! JS 插件：内置扩展与第三方扩展都是 `extension.json` + JS 模块，由内嵌的 QuickJS 运行，
//! 只能通过 `sayso.*` API 使用宿主能力。

mod builtin;
mod catalog;
mod instruction;
mod runtime;

use std::path::PathBuf;

use async_trait::async_trait;

pub use builtin::built_in_plugins;
pub use catalog::{load_installed, load_one, CatalogProblem, MANIFEST_FILE};
pub use instruction::Instruction;
pub use runtime::run;

use crate::error::CoreError;
use crate::protocol::{Delivery, Manifest};
use crate::store::StoreRecord;

/// 一个已加载的插件。
#[derive(Clone, Debug)]
pub struct Plugin {
    pub manifest: Manifest,
    pub source: PluginSource,
}

#[derive(Clone, Debug)]
pub enum PluginSource {
    /// 随 core 编译嵌入的内置插件。
    BuiltIn { main: &'static str, modules: &'static [(&'static str, &'static str)] },
    /// 安装在扩展目录中的插件。
    Installed { directory: PathBuf },
}

impl Plugin {
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn directory(&self) -> Option<&PathBuf> {
        match &self.source {
            PluginSource::BuiltIn { .. } => None,
            PluginSource::Installed { directory } => Some(directory),
        }
    }
}

/// 一次事件处理中插件可用的宿主能力。能力校验已由运行时完成，宿主只负责执行。
#[async_trait]
pub trait Host: Send + Sync {
    /// 执行一条指令。返回错误时记为警告，不中断插件。
    fn apply(&self, instruction: Instruction) -> Result<(), CoreError>;
    /// 用用户配置的翻译模型补全。
    async fn complete(&self, system: String, user: String) -> Result<String, CoreError>;
    /// 写入触发命令时的前台应用，返回投递方式。
    async fn insert(&self, text: String) -> Result<Delivery, CoreError>;
    /// 读取本插件的某个数据集合。
    fn records(&self, collection: &str) -> Result<Vec<StoreRecord>, CoreError>;
}

/// 单个插件处理一次事件的结果，显示在扩展管理页。
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RunResult {
    pub extension_id: String,
    pub status: RunStatus,
    /// 被忽略的指令等非致命问题。
    pub warnings: Vec<String>,
    /// `console.*` 输出的末尾部分。
    pub log: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RunStatus {
    Succeeded,
    Failed { message: String },
    TimedOut,
}
