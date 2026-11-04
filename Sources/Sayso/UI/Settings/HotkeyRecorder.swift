import AppKit
import Carbon.HIToolbox
import SaysoCore
import SwiftUI

/// 键帽输入框：点击后捕获按键，录制中显示主题色描边；Esc 或再次点击取消。
/// 按键如何解释由调用方的 `capture` 决定，返回 true 表示录制完成。
struct KeyCaptureField<Value: View>: View {
    let prompt: String
    let monitor: HotkeyMonitor
    let capture: (NSEvent) -> Bool
    @ViewBuilder let value: Value

    @State private var isRecording = false
    @State private var eventMonitor: Any?

    var body: some View {
        Button {
            isRecording ? finish() : begin()
        } label: {
            label
                .font(.system(size: 12))
                .padding(.horizontal, 5)
                .frame(minWidth: 130, minHeight: 28)
                .background(Theme.cardFill, in: RoundedRectangle(cornerRadius: Theme.Radius.control, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.Radius.control, style: .continuous)
                        .strokeBorder(isRecording ? Theme.accent : Theme.hairline, lineWidth: isRecording ? 1.5 : 1)
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onDisappear(perform: finish)
    }

    @ViewBuilder
    private var label: some View {
        if isRecording {
            HStack(spacing: 8) {
                Text(prompt).foregroundStyle(Theme.accentForeground)
                Keycap(label: "esc")
            }
        } else {
            value
        }
    }

    private func begin() {
        isRecording = true
        // 录制期间暂停全局监听，避免按到快捷键时触发命令。
        monitor.isPaused = true
        eventMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { event in
            if event.type == .keyDown, Int(event.keyCode) == kVK_Escape || capture(event) {
                finish()
            } else if event.type == .flagsChanged, capture(event) {
                finish()
            }
            return nil
        }
    }

    private func finish() {
        if let eventMonitor { NSEvent.removeMonitor(eventMonitor) }
        eventMonitor = nil
        isRecording = false
        monitor.isPaused = false
    }
}

/// 录制快捷键：单独按下并松开一个修饰键即为单修饰键快捷键，按下「修饰键 + 普通键」即为组合快捷键。
struct HotkeyRecorder: View {
    /// nil 表示没有快捷键。
    @Binding var hotkey: Hotkey?
    let monitor: HotkeyMonitor

    @State private var pendingModifier: UInt16?

    var body: some View {
        KeyCaptureField(prompt: L("请按下快捷键…", "Press a shortcut…"), monitor: monitor, capture: capture) {
            if let hotkey {
                KeycapRow(keys: hotkey.keycaps)
            } else {
                Text(L("设置快捷键", "Set shortcut")).foregroundStyle(.secondary)
            }
        }
    }

    private func capture(_ event: NSEvent) -> Bool {
        let keyCode = event.keyCode
        let flags = CGEventFlags(rawValue: UInt64(event.cgEvent?.flags.rawValue ?? 0))
        switch event.type {
        case .keyDown:
            let candidate = Hotkey(keyCode: keyCode, modifiers: flags.intersection(Hotkey.standardModifierMask).rawValue)
            // 不带修饰键的普通字符会干扰正常打字，只允许功能键单独作为快捷键。
            guard candidate.modifiers != 0 || candidate.isFunctionKey else { return false }
            hotkey = candidate
            return true
        case .flagsChanged:
            guard let flag = Hotkey.modifierFlagsByKeyCode[keyCode] else { return false }
            if flags.contains(flag) {
                pendingModifier = keyCode
                return false
            }
            guard pendingModifier == keyCode else { return false }
            hotkey = Hotkey(keyCode: keyCode, modifiers: 0)
            return true
        default:
            return false
        }
    }
}
