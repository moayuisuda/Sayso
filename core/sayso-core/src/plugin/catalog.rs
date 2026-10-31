use std::collections::HashSet;
use std::path::Path;

use super::{Plugin, PluginSource};
use crate::error::CoreError;
use crate::protocol::Manifest;

pub const MANIFEST_FILE: &str = "extension.json";

/// 无法加载的扩展目录与原因，显示在扩展管理页。
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CatalogProblem {
    pub directory_name: String,
    pub reason: String,
}

/// 从扩展目录加载已安装的插件：每个子目录一个插件，目录内必须有 `extension.json`。
pub fn load_installed(root: &Path, reserved_ids: &HashSet<String>) -> (Vec<Plugin>, Vec<CatalogProblem>) {
    let mut directories: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && !path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.')))
        .collect();
    directories.sort();

    let mut plugins = Vec::new();
    let mut problems = Vec::new();
    let mut seen = reserved_ids.clone();
    for directory in directories {
        match load_one(&directory) {
            // id 是启停状态与数据目录的键，重复会导致串号。
            Ok(plugin) if !seen.insert(plugin.manifest.id.clone()) => problems.push(CatalogProblem {
                directory_name: directory_name(&directory),
                reason: format!("duplicate id {}", plugin.manifest.id),
            }),
            Ok(plugin) => plugins.push(plugin),
            Err(error) => {
                problems.push(CatalogProblem { directory_name: directory_name(&directory), reason: error.to_string() })
            }
        }
    }
    (plugins, problems)
}

/// 读取并校验一个扩展目录。
pub fn load_one(directory: &Path) -> Result<Plugin, CoreError> {
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(directory.join(MANIFEST_FILE))?)
        .map_err(|error| CoreError::InvalidManifest(error.to_string()))?;
    manifest.validate(false)?;
    Ok(Plugin { manifest, source: PluginSource::Installed { directory: directory.to_path_buf() } })
}

fn directory_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}
