import AppKit
import SaysoCore

/// 通过剪贴板 + 模拟 ⌘V 把文本写入前台应用，完成后恢复用户原来的剪贴板内容。
@MainActor
final class TextInjector {
    /// 粘贴后等待目标应用读取剪贴板的时间（Electron 类应用需要数百毫秒）。
    private let restoreDelay: Duration = .milliseconds(400)

    func deliver(_ text: String, to destination: AppInfo?) async -> Delivery {
        let pasteboard = NSPasteboard.general
        // 没有外部目标应用时只放进剪贴板，保证说的话不会丢。
        guard destination != nil else {
            pasteboard.clearContents()
            pasteboard.setString(text, forType: .string)
            return .copiedToClipboard
        }

        let snapshot = ClipboardSnapshot.capture(from: pasteboard)
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
        let changeCount = pasteboard.changeCount

        SyntheticKeys.pressCommand(SyntheticKeys.v)
        try? await Task.sleep(for: restoreDelay)
        snapshot.restore(to: pasteboard, ifChangeCountIs: changeCount)
        return .inserted
    }
}
