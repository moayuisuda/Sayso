use super::{Plugin, PluginSource};
use crate::protocol::Manifest;

/// 随 core 编译嵌入的内置插件：两个平台共用同一份 JS。
const DICTATION_MODULES: &[(&str, &str)] = &[
    ("prompt.js", include_str!("../../../plugins/dictation/prompt.js")),
    ("languages.js", include_str!("../../../plugins/dictation/languages.js")),
    ("transcript.js", include_str!("../../../plugins/dictation/transcript.js")),
];

pub fn built_in_plugins() -> Vec<Plugin> {
    let plugins = [
        (
            include_str!("../../../plugins/dictation/extension.json"),
            include_str!("../../../plugins/dictation/main.js"),
            DICTATION_MODULES,
        ),
        (
            include_str!("../../../plugins/selection/extension.json"),
            include_str!("../../../plugins/selection/main.js"),
            &[][..],
        ),
        (
            include_str!("../../../plugins/wordbook/extension.json"),
            include_str!("../../../plugins/wordbook/main.js"),
            &[][..],
        ),
    ];
    plugins
        .iter()
        .map(|(manifest, main, modules)| {
            let manifest: Manifest = serde_json::from_str(manifest).expect("built-in manifest is valid JSON");
            manifest.validate(true).expect("built-in manifest is valid");
            Plugin { manifest, source: PluginSource::BuiltIn { main, modules } }
        })
        .collect()
}
