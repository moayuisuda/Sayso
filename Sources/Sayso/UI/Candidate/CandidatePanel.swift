import AppKit
import SaysoCore
import SwiftUI

/// 屏幕底部居中的悬浮候选框。不会成为 key window，因此不抢前台应用的焦点。
@MainActor
final class CandidatePanel {
    private let panel: NSPanel
    private let bottomOffset: CGFloat = 72
    private var resizeObserver: NSObjectProtocol?

    init(model: CoreModel) {
        panel = NonActivatingPanel(
            contentRect: NSRect(x: 0, y: 0, width: 320, height: 60),
            styleMask: [.nonactivatingPanel, .borderless, .fullSizeContentView],
            backing: .buffered,
            defer: true
        )
        panel.isFloatingPanel = true
        panel.level = .floating
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]

        // 面板尺寸跟随 SwiftUI 内容变化，变化后重新居中。
        let host = NSHostingController(rootView: CandidateView(model: model))
        host.sizingOptions = [.preferredContentSize]
        panel.contentViewController = host
        resizeObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.didResizeNotification,
            object: panel,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.reposition() }
        }
    }

    func update(for phase: SessionState) {
        if phase == .idle {
            panel.orderOut(nil)
            return
        }
        // 只有展示结果时块上的操作可点击，录音与处理中让鼠标穿透。
        if case .presenting = phase {
            panel.ignoresMouseEvents = false
        } else {
            panel.ignoresMouseEvents = true
        }
        if !panel.isVisible {
            reposition()
            panel.orderFrontRegardless()
        }
    }

    private func reposition() {
        let mouse = NSEvent.mouseLocation
        guard let screen = NSScreen.screens.first(where: { $0.frame.contains(mouse) }) ?? NSScreen.main else { return }
        let size = panel.frame.size
        let visible = screen.visibleFrame
        panel.setFrameOrigin(NSPoint(
            x: (visible.midX - size.width / 2).rounded(),
            y: (visible.minY + bottomOffset).rounded()
        ))
    }
}

private final class NonActivatingPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}
