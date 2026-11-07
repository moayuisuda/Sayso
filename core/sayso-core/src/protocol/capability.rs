use serde::{Deserialize, Serialize};

/// 扩展可声明的能力，启用时由用户确认。声明顺序即展示顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    /// 向候选框添加块。
    Blocks,
    /// 拥有自己的数据集合与页签。
    Store,
    /// 把文字写入前台应用：用户点击块上的 `insert` 操作时，或处理用户触发的命令时（`sayso.insert`）。
    Insert,
    /// 使用用户配置的翻译模型（`sayso.llm`），扩展不需要自带 API Key。
    Llm,
    /// 向其他扩展广播事件。
    Publish,
    /// 读写扩展目录与用户指定路径中的文件（`sayso.fs`）。
    Fs,
    /// 设置提供给语音识别与翻译模型的背景（`sayso.context`）。
    Context,
    /// 执行跨平台 shell 脚本（`sayso.shell`），可以启动本机程序。
    Shell,
}

impl Capability {
    pub fn name(self) -> &'static str {
        match self {
            Self::Blocks => "blocks",
            Self::Store => "store",
            Self::Insert => "insert",
            Self::Llm => "llm",
            Self::Publish => "publish",
            Self::Fs => "fs",
            Self::Context => "context",
            Self::Shell => "shell",
        }
    }
}
