use super::{Plugin, PluginSource};
use crate::protocol::Manifest;

/// 随 core 编译嵌入的内置插件：两个平台共用同一份 JS。
const BUILT_INS: [(&str, &str); 3] = [
    (include_str!("../../../plugins/dictation/extension.json"), include_str!("../../../plugins/dictation/main.js")),
    (include_str!("../../../plugins/selection/extension.json"), include_str!("../../../plugins/selection/main.js")),
    (include_str!("../../../plugins/wordbook/extension.json"), include_str!("../../../plugins/wordbook/main.js")),
];

pub fn built_in_plugins() -> Vec<Plugin> {
    BUILT_INS
        .iter()
        .map(|(manifest, main)| {
            let manifest: Manifest = serde_json::from_str(manifest).expect("built-in manifest is valid JSON");
            manifest.validate(true).expect("built-in manifest is valid");
            Plugin { manifest, source: PluginSource::BuiltIn { main } }
        })
        .collect()
}
