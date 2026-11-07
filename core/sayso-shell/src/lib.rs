//! Core 与独立 shell 执行进程之间唯一的请求格式。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const EXECUTABLE: &str = if cfg!(windows) { "sayso-shell.exe" } else { "sayso-shell" };

#[derive(Deserialize, Serialize)]
pub struct Request {
    pub script: String,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub stdin: String,
    /// 控制结果独立写入文件，不混入脚本的 stdout/stderr，也不占用脚本的退出码。
    pub result: PathBuf,
}
