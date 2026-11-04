import SwiftUI
import SaysoCore

struct GeneralSettingsView: View {
    let model: CoreModel
    let hotkeys: HotkeyMonitor
    let pronouncer: Pronouncer

    @State private var microphoneGranted = Permissions.microphoneGranted
    @State private var accessibilityGranted = Permissions.accessibilityGranted
    @State private var previewError: String?
    private let permissionTimer = Timer.publish(every: 1, on: .main, in: .common).autoconnect()

    var body: some View {
        SettingsPage {
            SettingsSection(L("语言", "Languages")) {
                SettingsRow(title: L("目标语言", "Target language"), caption: L("正在学习的语言", "The language you are learning")) {
                    languagePicker(model.binding(\.targetLanguage))
                    if let restriction {
                        Text(restriction)
                            .font(.system(size: 12))
                            .foregroundStyle(.secondary)
                    }
                }
                SettingsRow(
                    title: L("本地语言", "Native language"),
                    caption: L("用目标语言说话即可正常输入；不会说的词直接用本地语言说，会自动翻译成目标语言。",
                               "Speak your target language as usual. Say any word you don't know in your native language and it is translated in place.")
                ) {
                    languagePicker(model.binding(\.nativeLanguage))
                    if !model.languagesAreValid {
                        Label(L("目标语言与本地语言不能相同", "Target and native languages must differ"), systemImage: "exclamationmark.triangle.fill")
                            .font(.system(size: 12))
                            .foregroundStyle(.red)
                    }
                }
                SettingsRow(
                    title: L("翻译背景", "Translation background"),
                    caption: L("提供给语音识别与翻译模型，让专业词汇更准确。写明你的身份或常说的领域即可。",
                               "Shared with the speech and translation models so domain terms come out right. Describe your role or the topics you usually talk about.")
                ) {
                    TextField(
                        L("翻译背景", "Translation background"),
                        text: model.binding(\.userContext),
                        prompt: Text(L("例如：我是软件工程师，常说编程与云服务术语", "e.g. I'm a software engineer and often use programming and cloud terms")),
                        axis: .vertical
                    )
                    .lineLimit(2...4)
                    .labelsHidden()
                    .settingsControlWidth()
                }
            }

            SettingsSection(L("快捷键", "Shortcuts")) {
                SettingsRow(
                    title: L("触发方式", "Mode"),
                    caption: L("语音快捷键的用法。录音或识别过程中按 Esc 取消。", "How speech shortcuts work. Press Esc to cancel while recording or recognizing.")
                ) {
                    Picker(L("触发方式", "Mode"), selection: model.binding(\.hotkeyStyle)) {
                        ForEach(HotkeyStyle.allCases) { Text($0.displayName).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .fixedSize()
                }
                ForEach(model.enabledCommands) { command in
                    ShortcutRow(command: command, model: model, hotkeys: hotkeys)
                }
            }

            SettingsSection(L("朗读", "Pronunciation")) {
                SettingsRow(title: L("自动朗读", "Read aloud")) {
                    Toggle(L("生成翻译块时自动朗读", "Read translated blocks aloud"), isOn: model.binding(\.autoSpeak))
                        .toggleStyle(.checkbox)
                }
                SettingsRow(title: L("语速", "Speed")) {
                    HStack(spacing: 12) {
                        Slider(value: model.binding(\.speechRate), in: 0.3...0.6)
                            .frame(maxWidth: 220)
                        Button(L("试听", "Preview")) {
                            do {
                                try pronouncer.speak("Practice makes perfect.", in: model.settings.targetLanguage, rate: model.settings.speechRate)
                                previewError = nil
                            } catch PlatformError.Failed(let message) {
                                previewError = message
                            } catch {
                                previewError = error.localizedDescription
                            }
                        }
                        .buttonStyle(.chip)
                    }
                    if let previewError {
                        Label(previewError, systemImage: "speaker.slash.fill")
                            .font(.system(size: 12))
                            .foregroundStyle(.red)
                    }
                }
            }

            SettingsSection(L("权限", "Permissions")) {
                PermissionRow(
                    title: L("麦克风", "Microphone"),
                    detail: L("用于录音", "Used for recording"),
                    granted: microphoneGranted
                ) {
                    Task { microphoneGranted = await Permissions.requestMicrophone() }
                }
                PermissionRow(
                    title: L("辅助功能", "Accessibility"),
                    detail: L("用于全局快捷键与输入文字", "Used for the global shortcut and typing text"),
                    granted: accessibilityGranted
                ) {
                    Permissions.promptAccessibility()
                }
            }

            SettingsSection(L("关于", "About")) {
                VersionRow(model: model)
            }
        }
        .onReceive(permissionTimer) { _ in
            microphoneGranted = Permissions.microphoneGranted
            accessibilityGranted = Permissions.accessibilityGranted
        }
    }

    /// 所选识别来源能识别的语言；来源不可用时不限制。
    private var recognizable: [Language] {
        model.selectedAsrSource?.languages ?? supportedLanguages().map(\.language)
    }

    /// 识别来源只支持部分语言时，提示可选范围。
    private var restriction: String? {
        guard let source = model.selectedAsrSource, source.languages.count < supportedLanguages().count else { return nil }
        let names = supportedLanguages().filter { source.languages.contains($0.language) }.map(\.nativeName).joined(separator: "、")
        return L("语音模型「\(source.title)」只支持：\(names)", "Speech model \"\(source.title)\" only supports: \(names)")
    }

    /// 识别来源不支持的语言不可选。
    private func languagePicker(_ selection: Binding<Language>) -> some View {
        Picker("", selection: selection) {
            ForEach(supportedLanguages(), id: \.language) { info in
                Text(info.nativeName).tag(info.language).disabled(!recognizable.contains(info.language))
            }
        }
        .labelsHidden()
        .fixedSize()
    }
}

/// 命令的快捷键（单修饰键或组合键，可清除）与命令收到的输入。一个快捷键只对应一个命令：设成其他命令的快捷键时，那个命令的快捷键被清除。
private struct ShortcutRow: View {
    let command: EnabledCommand
    let model: CoreModel
    let hotkeys: HotkeyMonitor

    var body: some View {
        let shortcut = model.shortcut(of: command.key)
        SettingsRow(title: command.spec.title, caption: command.extensionName) {
            HStack(spacing: 8) {
                HotkeyRecorder(
                    hotkey: Binding(get: { shortcut?.key }, set: { model.setShortcut($0, of: command.key) }),
                    monitor: hotkeys
                )
                if let shortcut {
                    Picker(L("输入", "Input"), selection: Binding(get: { shortcut.input }, set: { model.setInput($0, of: command.key) })) {
                        ForEach(InputSource.allCases) { Text($0.displayName).tag($0) }
                    }
                    .labelsHidden()
                    .fixedSize()
                    .help(L("命令收到的输入", "Input the command receives"))
                    Button {
                        model.setShortcut(nil, of: command.key)
                    } label: {
                        Image(systemName: "xmark")
                    }
                    .buttonStyle(.chip)
                    .help(L("清除", "Clear"))
                }
            }
        }
    }
}

private struct PermissionRow: View {
    let title: String
    let detail: String
    let granted: Bool
    let request: () -> Void

    var body: some View {
        SettingsRow(title: title, caption: detail) {
            if granted {
                Label(L("已授权", "Granted"), systemImage: "checkmark.circle.fill")
                    .foregroundStyle(.green)
            } else {
                Button(L("去授权", "Grant"), action: request)
                    .buttonStyle(.borderedProminent)
            }
        }
    }
}

/// 当前版本与手动检查更新；已发现新版本时显示下载入口。
private struct VersionRow: View {
    let model: CoreModel

    private enum CheckResult {
        case upToDate
        case failed(String)
    }

    @State private var checking = false
    @State private var result: CheckResult?

    var body: some View {
        SettingsRow(title: L("版本", "Version")) {
            HStack(spacing: 12) {
                Text(model.core.appVersion())
                    .monospacedDigit()
                Button(L("检查更新", "Check for updates"), action: check)
                    .buttonStyle(.chip)
                    .disabled(checking)
                if checking {
                    ProgressView().controlSize(.small)
                }
            }
            status
                .font(.system(size: 12))
        }
    }

    @ViewBuilder
    private var status: some View {
        if let update = model.update, let url = URL(string: update.url) {
            HStack(spacing: 8) {
                Label(L("新版本 \(update.version) 已发布", "Version \(update.version) is available"), systemImage: "arrow.down.circle.fill")
                    .foregroundStyle(Theme.accent)
                Link(L("前往下载", "Download"), destination: url)
            }
        } else if case .failed(let message) = result {
            Label(message, systemImage: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
        } else if case .upToDate = result {
            Label(L("已是最新版本", "You're up to date"), systemImage: "checkmark.circle.fill")
                .foregroundStyle(.secondary)
        }
    }

    private func check() {
        checking = true
        Task {
            do {
                result = try await model.checkForUpdate() == nil ? .upToDate : nil
            } catch {
                result = .failed(describe(error))
            }
            checking = false
        }
    }
}
