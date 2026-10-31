import AppKit
import SaysoCore

/// macOS 平台适配：core 通过它录音、读取选区、写入前台应用、朗读、访问剪贴板与钥匙串。
/// core 可能在任意线程调用这些方法，需要主线程的能力在这里切换。
final class MacPlatform: Platform, @unchecked Sendable {
    let pronouncer: Pronouncer
    private let injector: TextInjector
    private let recorder = AudioRecorder()
    /// 录音数据与音量的去向，由 `AppModel` 在 core 创建后接上。
    var onAudio: @Sendable (Data) -> Void = { _ in }
    var onLevel: @Sendable (Float) -> Void = { _ in }

    @MainActor
    init() {
        pronouncer = Pronouncer()
        injector = TextInjector()
    }

    func startAudio() throws {
        do {
            try recorder.start(onChunk: onAudio, onLevel: onLevel)
        } catch {
            throw PlatformError.Failed(message: error.localizedDescription)
        }
    }

    /// 录音尾部数据在返回前经 `onAudio` 送出。
    func stopAudio() {
        recorder.stop()
        onLevel(0)
    }

    func readSelection() async -> String {
        await SelectionReader.read()
    }

    func insertText(text: String, destination: AppInfo?) async -> Delivery {
        await injector.deliver(text, to: destination)
    }

    func frontmostApp() -> AppInfo? {
        AppInfo.frontmost()
    }

    func speak(text: String, language: Language, rate: Float) {
        onMain { self.pronouncer.speak(text, in: language, rate: rate) }
    }

    func stopSpeaking() {
        onMain { self.pronouncer.stop() }
    }

    func copyText(text: String) {
        onMain {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
        }
    }

    func openUrl(url: String) {
        onMain {
            guard let url = URL(string: url) else { return }
            NSWorkspace.shared.open(url)
        }
    }

    func secretRead(account: String) -> String? {
        let value = KeychainStore.read(account)
        return value.isEmpty ? nil : value
    }

    func secretWrite(account: String, value: String?) {
        KeychainStore.write(value ?? "", for: account)
    }
}

/// 在主线程执行：已在主线程时立即执行，保证与调用方的先后顺序（如开始录音前先停止朗读）；否则排入主队列。
private func onMain(_ body: @escaping @MainActor () -> Void) {
    if Thread.isMainThread {
        MainActor.assumeIsolated(body)
    } else {
        DispatchQueue.main.async { MainActor.assumeIsolated(body) }
    }
}
