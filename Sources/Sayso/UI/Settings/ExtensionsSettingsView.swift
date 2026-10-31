import AppKit
import SwiftUI
import SaysoCore

/// 扩展管理：左侧为扩展列表，右侧为所选扩展的启停、命令快捷键、事件与能力、最近一次执行状态。
struct ExtensionsSettingsView: View {
    let model: CoreModel
    let packages: ExtensionPackageManager
    let hotkeys: HotkeyMonitor

    @State private var selectedID: String?
    /// 等待用户确认能力后启用的外部扩展。
    @State private var pendingApproval: ExtensionInfo?

    /// 未选择或所选扩展已被移除时，显示列表中的第一个。
    private var selected: ExtensionInfo? {
        model.extensions.first { $0.manifest.id == selectedID } ?? model.extensions.first
    }

    var body: some View {
        HStack(spacing: 0) {
            sidebar
                .frame(width: 260)
            Divider()
            if let selected {
                ExtensionDetail(info: selected, model: model, packages: packages, hotkeys: hotkeys) { enable in
                    if enable, !selected.builtIn {
                        pendingApproval = selected
                    } else {
                        model.setEnabled(enable, for: selected.manifest.id)
                    }
                }
                .id(selected.manifest.id)
            } else {
                ContentUnavailableView(L("没有扩展", "No extensions"), systemImage: "puzzlepiece.extension")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .alert(
            L("启用「\(pendingApproval?.manifest.name ?? "")」？", "Enable “\(pendingApproval?.manifest.name ?? "")”?"),
            isPresented: Binding(get: { pendingApproval != nil }, set: { if !$0 { pendingApproval = nil } }),
            presenting: pendingApproval
        ) { info in
            Button(L("启用", "Enable")) { model.setEnabled(true, for: info.manifest.id) }
            Button(L("取消", "Cancel"), role: .cancel) {}
        } message: { info in
            Text(info.permissionSummary)
        }
    }

    private var sidebar: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(model.extensions, id: \.manifest.id) { info in
                        Button { selectedID = info.manifest.id } label: {
                            ListRow(isSelected: info.manifest.id == selected?.manifest.id) {
                                ExtensionListItem(info: info)
                            }
                        }
                        .buttonStyle(.plain)
                    }

                    if !model.problems.isEmpty {
                        SidebarHeader(L("无法加载", "Failed to load"))
                        ForEach(model.problems, id: \.directoryName) { problem in
                            ListRow {
                                VStack(alignment: .leading, spacing: 2) {
                                    Label(problem.directoryName, systemImage: "exclamationmark.triangle.fill")
                                        .font(.system(size: 13, weight: .medium))
                                    Text(problem.reason)
                                        .font(.system(size: 11))
                                        .foregroundStyle(.red)
                                        .textSelection(.enabled)
                                }
                            }
                        }
                    }

                    SidebarHeader(L("外部扩展", "External extensions"))
                    Text(L("在扩展文件夹中为每个扩展建一个子目录，放入 extension.json 与 main.js（协议见 docs/extension-protocol.md）。外部扩展默认关闭，启用时需确认其声明的能力。",
                           "Create one sub-folder per extension in the Extensions folder with an extension.json and a main.js (see docs/extension-protocol.md). External extensions stay off until you approve their capabilities."))
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.horizontal, 10)
                }
                .padding(8)
            }
            ActionBar {
                Button {
                    packages.chooseAndInstall()
                } label: {
                    Label(L("安装扩展…", "Install…"), systemImage: "plus")
                }
                .buttonStyle(.chip)
                Button {
                    NSWorkspace.shared.open(URL(filePath: model.core.extensionsDirectory()))
                } label: {
                    Label(L("打开文件夹", "Open Folder"), systemImage: "folder")
                }
                .buttonStyle(.chip)
            } trailing: {
                Button {
                    model.core.reloadExtensions()
                } label: {
                    Label(L("重新加载", "Reload"), systemImage: "arrow.clockwise")
                }
                .buttonStyle(.chip)
            }
        }
    }
}

private struct SidebarHeader: View {
    let title: String

    init(_ title: String) { self.title = title }

    var body: some View {
        Text(title)
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 10)
            .padding(.top, 14)
            .padding(.bottom, 4)
    }
}

private struct ExtensionListItem: View {
    let info: ExtensionInfo

    var body: some View {
        HStack(spacing: 10) {
            ExtensionIcon(info: info, size: 28)
                .saturation(info.enabled ? 1 : 0)
            VStack(alignment: .leading, spacing: 1) {
                Text(info.manifest.name).font(.system(size: 13, weight: .medium))
                Text(info.enabled ? info.kindName : L("已停用", "Disabled"))
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            }
        }
    }
}

/// 扩展图标：有页签时用页签图标；内置扩展为主题色，外部扩展为青色。
private struct ExtensionIcon: View {
    let info: ExtensionInfo
    let size: CGFloat

    var body: some View {
        IconTile(
            systemImage: info.manifest.tab?.icon ?? "puzzlepiece.extension.fill",
            color: info.builtIn ? Theme.accent : .teal,
            size: size
        )
    }
}

private struct ExtensionDetail: View {
    let info: ExtensionInfo
    let model: CoreModel
    let packages: ExtensionPackageManager
    let hotkeys: HotkeyMonitor
    let setEnabled: (Bool) -> Void

    @State private var confirmingDeletion = false

    private var manifest: Manifest { info.manifest }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                header

                if !manifest.commands.isEmpty {
                    SettingsSection(L("命令", "Commands")) {
                        ForEach(manifest.commands, id: \.id) { command in
                            CommandRow(
                                command: command,
                                key: CommandKey(extensionId: manifest.id, commandId: command.id),
                                model: model,
                                hotkeys: hotkeys
                            )
                        }
                    }
                    .disabled(!info.enabled)
                }

                if let recognizer = manifest.recognizer {
                    SettingsSection(L("语音识别器", "Speech recognizer")) {
                        Label {
                            Text(L("「\(recognizer.title)」：启用后可在「语音识别」中选用，录音时实时识别。",
                                   "“\(recognizer.title)”: once enabled, choose it under Recognition for real-time transcription."))
                        } icon: {
                            Image(systemName: "waveform").foregroundStyle(Theme.accentForeground)
                        }
                    }
                }

                if !manifest.events.isEmpty {
                    SettingsSection(L("事件", "Events")) {
                        FlowLayout(spacing: 6) {
                            ForEach(manifest.events, id: \.self) { Tag(text: eventName(event: $0)) }
                        }
                    }
                }

                SettingsSection(L("能力", "Capabilities")) {
                    if manifest.capabilities.isEmpty {
                        Text(L("不申请额外能力", "No extra capabilities")).foregroundStyle(.secondary)
                    }
                    ForEach(manifest.capabilities, id: \.self) { capability in
                        Label {
                            Text(capability.localizedDescription)
                        } icon: {
                            Image(systemName: capability.icon).foregroundStyle(Theme.accentForeground)
                        }
                    }
                }

                if let result = info.lastResult {
                    SettingsSection(L("最近一次运行", "Last run")) {
                        RunResultView(result: result)
                    }
                }
            }
            .padding(24)
        }
    }

    private var header: some View {
        HStack(alignment: .top, spacing: 14) {
            ExtensionIcon(info: info, size: 48)
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(manifest.name).font(.system(size: 17, weight: .semibold))
                    Tag(text: info.kindName)
                }
                if let description = manifest.description, !description.isEmpty {
                    Text(description)
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 12)
            if !info.builtIn {
                Button {
                    packages.export(manifest.id)
                } label: {
                    Label(L("导出…", "Export…"), systemImage: "square.and.arrow.up")
                }
                .buttonStyle(.chip)
                .help(L("导出为 .sayso 文件，可直接发给他人安装", "Export as a .sayso file that others can install"))
                Button(role: .destructive) {
                    confirmingDeletion = true
                } label: {
                    Label(L("删除…", "Delete…"), systemImage: "trash")
                }
                .buttonStyle(.chip)
            }
            Toggle(L("启用", "Enabled"), isOn: Binding(get: { info.enabled }, set: setEnabled))
                .toggleStyle(.switch)
                .labelsHidden()
        }
        .confirmationDialog(
            L("删除扩展「\(manifest.name)」？", "Delete “\(manifest.name)”?"),
            isPresented: $confirmingDeletion
        ) {
            Button(L("删除", "Delete"), role: .destructive) { packages.uninstall(manifest.id) }
        } message: {
            Text(L("扩展的文件会被删除，它保存的数据会保留。", "The extension's files are removed; its saved data is kept."))
        }
    }
}

/// 命令及其路由（可清除）与路由的输入来源。同一路由上的命令同时触发，收到同一份输入。
private struct CommandRow: View {
    let command: CommandSpec
    let key: CommandKey
    let model: CoreModel
    let hotkeys: HotkeyMonitor

    private var route: RouteKey? { model.route(of: key) }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(command.title)
                Spacer(minLength: 12)
                if let route, let input = model.input(of: route) {
                    Picker(L("输入", "Input"), selection: Binding(get: { input }, set: { model.setInput($0, of: route) })) {
                        ForEach(InputSource.allCases) { Text($0.displayName).tag($0) }
                    }
                    .labelsHidden()
                    .fixedSize()
                    .help(L("该路由上的命令收到的输入", "Input that commands on this route receive"))
                }
                RouteRecorder(
                    route: Binding(get: { route }, set: { model.setRoute($0, of: key) }),
                    trigger: model.settings.triggerKey,
                    monitor: hotkeys
                )
                if route != nil {
                    Button {
                        model.setRoute(nil, of: key)
                    } label: {
                        Image(systemName: "xmark")
                    }
                    .buttonStyle(.chip)
                    .help(L("清除", "Clear"))
                }
            }
            let shared = sharedCommands
            if !shared.isEmpty {
                Label(L("与「\(names(shared))」同时触发", "Triggers together with “\(names(shared))”"), systemImage: "link")
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            }
        }
    }

    /// 已启用的其他命令中，与本命令路由相同的。
    private var sharedCommands: [EnabledCommand] {
        guard let route else { return [] }
        return model.enabledCommands.filter { $0.key != key && model.route(of: $0.key) == route }
    }

    /// 「扩展名 · 命令名」列表。
    private func names(_ commands: [EnabledCommand]) -> String {
        commands.map { "\($0.extensionName) · \($0.spec.title)" }.joined(separator: L("、", ", "))
    }
}

private struct RunResultView: View {
    let result: RunResult

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label {
                Text(title).textSelection(.enabled)
            } icon: {
                Image(systemName: icon).foregroundStyle(isClean ? Color.green : Color.red)
            }
            ForEach(Array(result.warnings.enumerated()), id: \.offset) { _, warning in
                Text(warning)
                    .font(.system(size: 11))
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
            }
            // 扩展 console.* 的输出（末尾部分），便于作者排查。
            if !result.log.isEmpty {
                Text(result.log)
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
                    .background(Theme.keycapFill, in: RoundedRectangle(cornerRadius: Theme.Radius.keycap, style: .continuous))
            }
        }
    }

    private var isClean: Bool { result.status == .succeeded && result.warnings.isEmpty }

    private var title: String {
        switch result.status {
        case .succeeded: return L("成功", "Succeeded")
        case .timedOut: return L("超时", "Timed out")
        case .failed(let message): return L("失败：\(message)", "Failed: \(message)")
        }
    }

    private var icon: String {
        switch result.status {
        case .succeeded: return isClean ? "checkmark.circle.fill" : "exclamationmark.circle.fill"
        case .timedOut: return "clock.badge.exclamationmark"
        case .failed: return "xmark.octagon.fill"
        }
    }
}

/// 小号标签，用于扩展类型与事件名。
private struct Tag: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(Theme.keycapFill, in: RoundedRectangle(cornerRadius: Theme.Radius.keycap, style: .continuous))
    }
}

