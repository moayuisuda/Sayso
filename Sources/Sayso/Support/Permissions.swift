import AVFoundation
import ApplicationServices

/// 应用所需的系统权限：麦克风（录音）与辅助功能（全局热键 + 模拟粘贴）。
enum Permissions {
    static var microphoneGranted: Bool {
        AVCaptureDevice.authorizationStatus(for: .audio) == .authorized
    }

    static func requestMicrophone() async -> Bool {
        await AVCaptureDevice.requestAccess(for: .audio)
    }

    static var accessibilityGranted: Bool {
        AXIsProcessTrusted()
    }

    /// 弹出系统授权引导。
    static func promptAccessibility() {
        let options = ["AXTrustedCheckOptionPrompt": true] as CFDictionary
        AXIsProcessTrustedWithOptions(options)
    }
}
