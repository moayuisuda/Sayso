//! 已注册的扩展：内置插件与安装在扩展目录中的插件，以及它们的命令与最近一次执行结果。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::l10n::tr;
use crate::plugin::{built_in_plugins, load_installed, CatalogProblem, Plugin, RunResult};
use crate::protocol::{Capability, CommandSpec, EventType, Manifest};
use crate::settings::{CommandKey, RouteKey, Settings};

/// 交给扩展管理页的扩展信息。
#[derive(Clone, Debug, uniffi::Record)]
pub struct ExtensionInfo {
    pub manifest: Manifest,
    pub built_in: bool,
    pub enabled: bool,
    pub last_result: Option<RunResult>,
    /// 启用或安装前向用户说明扩展会收到什么、提供什么、申请哪些能力。
    pub permission_summary: String,
}

/// 事件的投递范围。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Route {
    /// 只发给指定扩展（命令、操作回调）。`reports_failure` 为 true 时失败显示在候选框中。
    Target { extension_id: String, reports_failure: bool },
    /// 由某个扩展引起的广播（翻译词对、写入），发给除它以外的订阅者。
    Republished { by: String },
}

pub(crate) struct Extensions {
    built_ins: Vec<Plugin>,
    installed: Vec<Plugin>,
    problems: Vec<CatalogProblem>,
    last_results: HashMap<String, RunResult>,
}

impl Extensions {
    pub fn load(root: &Path) -> Self {
        let mut extensions = Extensions {
            built_ins: built_in_plugins(),
            installed: vec![],
            problems: vec![],
            last_results: HashMap::new(),
        };
        extensions.reload_installed(root);
        extensions
    }

    pub fn reload_installed(&mut self, root: &Path) {
        let reserved: HashSet<String> = self.built_ins.iter().map(|plugin| plugin.id().to_string()).collect();
        (self.installed, self.problems) = load_installed(root, &reserved);
    }

    pub fn all(&self) -> impl Iterator<Item = &Plugin> {
        self.built_ins.iter().chain(&self.installed)
    }

    pub fn get(&self, id: &str) -> Option<&Plugin> {
        self.all().find(|plugin| plugin.id() == id)
    }

    pub fn installed(&self, id: &str) -> Option<&Plugin> {
        self.installed.iter().find(|plugin| plugin.id() == id)
    }

    pub fn problems(&self) -> Vec<CatalogProblem> {
        self.problems.clone()
    }

    pub fn record_result(&mut self, result: RunResult) {
        self.last_results.insert(result.extension_id.clone(), result);
    }

    fn enabled<'a>(&'a self, settings: &'a Settings) -> impl Iterator<Item = &'a Plugin> {
        self.all().filter(|plugin| settings.enabled_extensions.iter().any(|id| id == plugin.id()))
    }

    /// 已启用扩展的命令，按注册顺序。
    pub fn enabled_commands<'a>(
        &'a self,
        settings: &'a Settings,
    ) -> impl Iterator<Item = (CommandKey, &'a CommandSpec)> {
        self.enabled(settings).flat_map(|plugin| {
            plugin
                .manifest
                .commands
                .iter()
                .map(|spec| (CommandKey { extension_id: plugin.id().into(), command_id: spec.id.clone() }, spec))
        })
    }

    /// 绑定在这些路由上的已启用命令。
    pub fn commands_routed_to(&self, settings: &Settings, routes: &[RouteKey]) -> Vec<CommandKey> {
        self.enabled_commands(settings)
            .filter(|(key, _)| settings.binding(key).is_some_and(|route| routes.contains(&route)))
            .map(|(key, _)| key)
            .collect()
    }

    /// 已启用、订阅了该事件且在投递范围内的扩展。
    pub fn recipients(&self, settings: &Settings, event_type: EventType, route: &Route) -> Vec<Plugin> {
        self.enabled(settings)
            .filter(|plugin| plugin.manifest.events.contains(&event_type))
            .filter(|plugin| match route {
                Route::Target { extension_id, .. } => plugin.id() == extension_id,
                Route::Republished { by } => plugin.id() != by,
            })
            .cloned()
            .collect()
    }

    pub fn infos(&self, settings: &Settings) -> Vec<ExtensionInfo> {
        self.all()
            .map(|plugin| ExtensionInfo {
                manifest: plugin.manifest.clone(),
                built_in: plugin.manifest.is_built_in(),
                enabled: settings.enabled_extensions.iter().any(|id| id == plugin.id()),
                last_result: self.last_results.get(plugin.id()).cloned(),
                permission_summary: permission_summary(&plugin.manifest),
            })
            .collect()
    }
}

/// 扩展能力对用户的说明。
#[uniffi::export]
pub fn capability_description(capability: Capability) -> String {
    match capability {
        Capability::Blocks => tr("在候选框中显示内容", "Show content in the candidate box"),
        Capability::Store => tr("保存自己的数据并拥有独立页签", "Keep its own data and own a tab"),
        Capability::Insert => tr(
            "在你点击按钮或触发它的命令时，把文字写入当前应用",
            "Type text into the current app when you click its button or trigger its command",
        ),
        Capability::Llm => tr("使用你配置的翻译模型", "Use the translation model you configured"),
        Capability::Publish => tr("向其他扩展广播事件", "Broadcast events to other extensions"),
        Capability::Fs => tr("读写你电脑上的文件", "Read and write files on your computer"),
    }
}

pub(crate) fn permission_summary(manifest: &Manifest) -> String {
    let mut lines = Vec::new();
    if !manifest.events.is_empty() {
        let events: Vec<_> = manifest.events.iter().map(|event| event.name()).collect();
        let events = events.join(", ");
        lines.push(tr(&format!("它将收到：{events}"), &format!("It will receive: {events}")));
    }
    let mut capabilities = manifest.capabilities.clone();
    capabilities.sort();
    capabilities.dedup();
    if capabilities.is_empty() {
        lines.push(tr("它不申请额外能力。", "It requests no extra capabilities."));
    } else {
        lines.push(tr("它申请：", "It requests:"));
        lines.extend(capabilities.into_iter().map(|capability| format!("• {}", capability_description(capability))));
    }
    lines.join("\n")
}
