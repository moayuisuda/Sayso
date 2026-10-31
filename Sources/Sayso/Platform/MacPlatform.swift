import AppKit
import SaysoCore

/// macOS 平台适配：core 通过它录音、读取选区、写入前台应用、朗读与访问剪贴板。
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

    func insertText(text: String, destination: AppInfo?) async throws -> Delivery {
        await injector.deliver(text, to: destination)
    }

    func frontmostApp() -> AppInfo? {
        AppInfo.frontmost()
    }

    func speak(text: String, language: Language, rate: Float) throws {
        try onMainSync { try self.pronouncer.speak(text, in: language, rate: rate) }
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
}

/// 朗读开始或失败须在平台回调返回前确定，不能把抛错的操作异步丢到主队列。
private func onMainSync(_ body: @MainActor () throws -> Void) throws {
    if Thread.isMainThread {
        try MainActor.assumeIsolated(body)
    } else {
        try DispatchQueue.main.sync { try MainActor.assumeIsolated(body) }
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
