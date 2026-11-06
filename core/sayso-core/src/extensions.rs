//! 已注册的扩展：内置插件与安装在扩展目录中的插件，以及它们的命令与最近一次执行结果。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;

use crate::l10n::tr;
use crate::plugin::{built_in_plugins, load_installed, CatalogProblem, Plugin, RunResult};
use crate::protocol::{Capability, CommandSpec, EventType, Manifest};
use crate::settings::{CommandKey, Hotkey, Settings, Shortcut};

/// 交给扩展管理页的扩展信息。
#[derive(Clone, Debug, uniffi::Record)]
pub struct ExtensionInfo {
    pub manifest: Manifest,
    pub built_in: bool,
    pub enabled: bool,
    pub last_result: Option<RunResult>,
    /// 启用或安装前向用户说明扩展会收到什么、提供什么、申请哪些能力。
    pub permission_summary: String,
    /// 订阅的 hook：什么时候运行、由谁广播。命令由快捷键触发，宿主按设置显示，不在其中。
    pub listens: Vec<HookLink>,
    /// 运行后会广播的 hook，以及随之运行的扩展。
    pub emits: Vec<HookLink>,
}

/// 扩展与一个 hook 之间确实连着的关系，用于在扩展管理页说明扩展之间如何串联。
#[derive(Clone, Debug, PartialEq, Serialize, uniffi::Record)]
pub struct HookLink {
    pub event: EventType,
    /// 这个 hook 在什么时候发生。
    pub description: String,
    /// 另一端的已启用扩展：订阅时为引出它的扩展，广播时为订阅它的扩展。
    pub extensions: Vec<String>,
    /// 向用户说明另一端的一句话；hook 由 Sayso 直接发出（`input`、`action`）时为空。
    pub relation: Option<String>,
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

    pub fn command_spec(&self, command: &CommandKey) -> Option<&CommandSpec> {
        self.get(&command.extension_id)?.manifest.commands.iter().find(|spec| spec.id == command.command_id)
    }

    /// 快捷键触发的命令及其名称；命令所属扩展未启用时为空。
    pub fn command_for<'a>(&self, settings: &'a Settings, key: Hotkey) -> Option<(&'a Shortcut, String)> {
        let shortcut = settings.shortcuts.get(&key)?;
        let (_, spec) = self.enabled_commands(settings).find(|(command, _)| *command == shortcut.command)?;
        Some((shortcut, spec.localized_title()))
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
                manifest: plugin.manifest.localized(),
                built_in: plugin.manifest.is_built_in(),
                enabled: settings.enabled_extensions.iter().any(|id| id == plugin.id()),
                last_result: self.last_results.get(plugin.id()).cloned(),
                permission_summary: permission_summary(&plugin.manifest),
                listens: self.listens(settings, &plugin.manifest),
                emits: self.emits(settings, &plugin.manifest),
            })
            .collect()
    }

    fn listens(&self, settings: &Settings, manifest: &Manifest) -> Vec<HookLink> {
        manifest
            .events
            .iter()
            .filter(|event| **event != EventType::Command)
            .filter_map(|event| match broadcaster_of(*event) {
                Some(capability) => {
                    let extensions = self.enabled_names(settings, manifest, |other| other.grants(capability));
                    peer_link(*event, tr("来自：", "From: "), extensions)
                }
                None => Some(HookLink {
                    event: *event,
                    description: hook_description(*event),
                    extensions: Vec::new(),
                    relation: None,
                }),
            })
            .collect()
    }

    fn emits(&self, settings: &Settings, manifest: &Manifest) -> Vec<HookLink> {
        [EventType::Translated, EventType::Inserted]
            .into_iter()
            .filter(|event| broadcaster_of(*event).is_some_and(|capability| manifest.grants(capability)))
            .filter_map(|event| {
                let extensions = self.enabled_names(settings, manifest, |other| other.events.contains(&event));
                peer_link(event, tr("接着运行：", "Runs next: "), extensions)
            })
            .collect()
    }

    /// 除 `manifest` 自身以外、满足条件的已启用扩展名。
    fn enabled_names(
        &self,
        settings: &Settings,
        manifest: &Manifest,
        matches: impl Fn(&Manifest) -> bool,
    ) -> Vec<String> {
        self.enabled(settings)
            .filter(|plugin| plugin.id() != manifest.id && matches(&plugin.manifest))
            .map(|plugin| plugin.manifest.name.clone())
            .collect()
    }
}

/// 与其他扩展之间的关系；另一端没有已启用的扩展时这条路走不通，不算关系。
fn peer_link(event: EventType, prefix: String, extensions: Vec<String>) -> Option<HookLink> {
    (!extensions.is_empty()).then(|| HookLink {
        event,
        description: hook_description(event),
        relation: Some(prefix + &extensions.join(&tr("、", ", "))),
        extensions,
    })
}

/// 让扩展的运行引出这个 hook 的能力：`translated` 由 `publish` 广播，`inserted` 在 `insert` 写入后由 Sayso 广播；
/// `input`、`action`、`focus` 不由扩展引出。
fn broadcaster_of(event: EventType) -> Option<Capability> {
    match event {
        EventType::Translated => Some(Capability::Publish),
        EventType::Inserted => Some(Capability::Insert),
        EventType::Command | EventType::Action | EventType::Input | EventType::Focus => None,
    }
}

fn hook_description(event: EventType) -> String {
    match event {
        EventType::Command => tr("按下它的命令的快捷键时", "When its command's shortcut is pressed"),
        EventType::Action => tr("点击它在候选框里的按钮时", "When its button in the candidate box is clicked"),
        EventType::Input => tr("快捷键触发后，Sayso 准备好输入时", "When Sayso has the input ready after a shortcut"),
        EventType::Translated => tr("翻译完毕时", "When a translation is done"),
        EventType::Inserted => tr("文字写入当前应用后", "After text is typed into the current app"),
        EventType::Focus => tr("切换到其他应用时", "When you switch to another app"),
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
        Capability::Context => tr(
            "设置提供给语音识别与翻译模型的背景",
            "Set the background given to the speech and translation models",
        ),
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
