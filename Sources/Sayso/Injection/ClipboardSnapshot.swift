import AppKit

/// 剪贴板文本类内容的快照。只读取文本类类型，避免触发其他应用的延迟数据提供者而阻塞。
struct ClipboardSnapshot {
    private static let textTypes: Set<NSPasteboard.PasteboardType> = [.string, .URL, .html, .rtf]

    let items: [[NSPasteboard.PasteboardType: Data]]

    static func capture(from pasteboard: NSPasteboard) -> ClipboardSnapshot {
        let items = (pasteboard.pasteboardItems ?? []).map { item in
            item.types.filter(textTypes.contains).reduce(into: [NSPasteboard.PasteboardType: Data]()) { result, type in
                result[type] = item.data(forType: type)
            }
        }
        return ClipboardSnapshot(items: items.filter { !$0.isEmpty })
    }

    /// 期间用户若又复制了别的内容（changeCount 变化），则不覆盖。
    func restore(to pasteboard: NSPasteboard, ifChangeCountIs changeCount: Int) {
        guard pasteboard.changeCount == changeCount else { return }
        pasteboard.clearContents()
        let restored = items.map { entries in
            let item = NSPasteboardItem()
            entries.forEach { item.setData($0.value, forType: $0.key) }
            return item
        }
        if !restored.isEmpty { pasteboard.writeObjects(restored) }
    }
}

/// 向前台应用发送模拟按键。
enum SyntheticKeys {
    static let c: CGKeyCode = 8
    static let v: CGKeyCode = 9

    static func pressCommand(_ key: CGKeyCode) {
        let source = CGEventSource(stateID: .combinedSessionState)
        for isDown in [true, false] {
            let event = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: isDown)
            event?.flags = .maskCommand
            event?.post(tap: .cghidEventTap)
        }
    }
}
