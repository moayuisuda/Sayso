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
            }

            SettingsSection(L("快捷键", "Shortcut")) {
                SettingsRow(
                    title: L("开始输入", "Start input"),
                    caption: L("所有命令的统一入口。按住说话，交给「直接」路由的命令（默认语音翻译）；按住期间按命令的路由键（如 T），改为交给该命令。路由键在「扩展」页设置。",
                               "The single entry for every command. Hold and speak to reach Direct commands (Speech Translate by default); press a command's route key (such as T) while holding to reach that command instead. Set route keys on the Extensions page.")
                ) {
                    HotkeyRecorder(hotkey: model.binding(\.triggerKey), monitor: hotkeys)
                }
                SettingsRow(
                    title: L("触发方式", "Mode"),
                    caption: L("录音或识别过程中按 Esc 取消。", "Press Esc to cancel while recording or recognizing.")
                ) {
                    Picker(L("触发方式", "Mode"), selection: model.binding(\.hotkeyStyle)) {
                        ForEach(HotkeyStyle.allCases) { Text($0.displayName).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .fixedSize()
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
        }
        .onReceive(permissionTimer) { _ in
            microphoneGranted = Permissions.microphoneGranted
            accessibilityGranted = Permissions.accessibilityGranted
        }
    }

    private func languagePicker(_ selection: Binding<Language>) -> some View {
        Picker("", selection: selection) {
            ForEach(supportedLanguages(), id: \.language) { Text($0.nativeName).tag($0.language) }
        }
        .labelsHidden()
        .fixedSize()
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
