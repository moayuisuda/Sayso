import AppKit
import ApplicationServices
import os
import SaysoCore

/// 读取前台应用中当前选中的文本。
///
/// 优先通过辅助功能接口直接读取选区；读不到内容时（网页、Electron 等常见）临时模拟 ⌘C 复制选区，
/// 读取后恢复用户原来的剪贴板。
@MainActor
enum SelectionReader {
    /// 辅助功能调用是同步跨进程通信，目标应用卡顿时可能长时间阻塞，需限时。
    private static let accessibilityTimeout: Duration = .milliseconds(500)
    private static let copyTimeout: Duration = .milliseconds(600)
    private static let logger = Logger(subsystem: "com.sayso.app", category: "selection")

    static func read() async -> String {
        let app = NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "-"
        // 网页内容的选区常被报告为空串（浏览器未开启完整无障碍树），空串不能说明没有选中，需改用复制。
        if let text = await readViaAccessibility(), !text.isEmpty {
            logger.info("selection via accessibility app=\(app, privacy: .public) length=\(text.count)")
            return text
        }
        let text = await readViaCopy()
        logger.info("selection via copy app=\(app, privacy: .public) length=\(text.count)")
        return text
    }

    /// 返回 nil 表示该应用不支持读取选区或超时。
    private static func readViaAccessibility() async -> String? {
        let text = await withCheckedContinuation { (continuation: CheckedContinuation<String?, Never>) in
            let resumed = OSAllocatedUnfairLock(initialState: false)
            let resumeOnce: @Sendable (String?) -> Void = { value in
                if resumed.withLock({ let first = !$0; $0 = true; return first }) { continuation.resume(returning: value) }
            }
            Task.detached { resumeOnce(focusedSelectedText()) }
            Task.detached {
                try? await Task.sleep(for: accessibilityTimeout)
                resumeOnce(nil)
            }
        }
        logger.debug("accessibility selection \(text == nil ? "unavailable" : "length=\(text!.count)", privacy: .public)")
        return text
    }

    nonisolated private static func focusedSelectedText() -> String? {
        let systemWide = AXUIElementCreateSystemWide()
        var focused: CFTypeRef?
        guard AXUIElementCopyAttributeValue(systemWide, kAXFocusedUIElementAttribute as CFString, &focused) == .success,
              let focused
        else { return nil }
        var selected: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            unsafeDowncast(focused, to: AXUIElement.self),
            kAXSelectedTextAttribute as CFString,
            &selected
        ) == .success else { return nil }
        return selected as? String
    }

    private static func readViaCopy() async -> String {
        // 热键的修饰键还按着时发送 ⌘C，前台应用会收到 ⌘⌥⇧C 之类的组合（Chrome 中 ⌘⌥C 会打开检查元素），
        // 因此等用户松开后再复制：按住快捷键等结果时，一松手即读取。用户取消时 core 中止本次读取，不再复制。
        while !CGEventSource.flagsState(.combinedSessionState).intersection(Hotkey.standardModifierMask).isEmpty {
            do {
                try await Task.sleep(for: .milliseconds(20))
            } catch {
                logger.info("selection copy cancelled while waiting for key release")
                return ""
            }
        }

        let pasteboard = NSPasteboard.general
        let snapshot = ClipboardSnapshot.capture(from: pasteboard)
        let before = pasteboard.changeCount
        SyntheticKeys.pressCommand(SyntheticKeys.c)

        let copyDeadline = ContinuousClock.now + copyTimeout
        while pasteboard.changeCount == before, ContinuousClock.now < copyDeadline {
            try? await Task.sleep(for: .milliseconds(20))
        }
        // 没有选区时 ⌘C 不改变剪贴板，此时无需恢复。
        guard pasteboard.changeCount != before else {
            logger.info("selection copy: clipboard unchanged")
            return ""
        }
        let text = pasteboard.string(forType: .string) ?? ""
        snapshot.restore(to: pasteboard, ifChangeCountIs: pasteboard.changeCount)
        return text
    }
}
