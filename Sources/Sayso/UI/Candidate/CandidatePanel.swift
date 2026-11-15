import AppKit
import SaysoCore
import SwiftUI

/// 可在当前屏幕定位的悬浮候选框。不会成为 key window，因此不抢前台应用的焦点。
@MainActor
final class CandidatePanel {
    private struct DismissalContext: Sendable {
        let surfaceID: String
        let shownAt: TimeInterval
        let foregroundProcessID: pid_t?
    }

    private let panel: NSPanel
    private let edgeMargin: CGFloat = 16
    private let model: CoreModel
    private let positions = candidatePositions()
    private var resizeObserver: NSObjectProtocol?
    private let core: Core
    private var dismissal: DismissalContext?
    private var localMouseMonitor: Any?
    private var globalMouseMonitor: Any?
    private var activationObserver: NSKeyValueObservation?

    init(model: CoreModel) {
        self.model = model
        core = model.core
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

        // 面板尺寸跟随 SwiftUI 内容变化，变化后按所选位置重新摆放。
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
        if case .presenting(let surface) = phase {
            watchDismissal(surfaceID: surface.id)
        } else {
            stopWatchingDismissal()
        }
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

    /// 键盘与鼠标共用系统启动时间，拒绝面板出现前已经产生的输入事件。
    func dismissibleSurface(forEventAt timestamp: TimeInterval) -> String? {
        guard let dismissal, timestamp >= dismissal.shownAt else { return nil }
        return dismissal.surfaceID
    }

    private func watchDismissal(surfaceID: String) {
        guard dismissal?.surfaceID != surfaceID else { return }
        stopWatchingDismissal()
        let context = DismissalContext(
            surfaceID: surfaceID,
            shownAt: ProcessInfo.processInfo.systemUptime,
            foregroundProcessID: NSWorkspace.shared.frontmostApplication?.processIdentifier
        )
        dismissal = context
        let clicks: NSEvent.EventTypeMask = [.leftMouseDown, .rightMouseDown, .otherMouseDown]
        localMouseMonitor = NSEvent.addLocalMonitorForEvents(matching: clicks) { [weak self] event in
            MainActor.assumeIsolated { self?.dismissIfOutside(event, context: context) }
            return event
        }
        globalMouseMonitor = NSEvent.addGlobalMonitorForEvents(matching: clicks) { [weak self] event in
            MainActor.assumeIsolated { self?.dismissIfOutside(event, context: context) }
        }
        // 激活通知不携带发生时间；直接观察实际前台应用变化，在回调产生时绑定面板与时间。
        activationObserver = NSWorkspace.shared.observe(\.frontmostApplication, options: [.initial, .new]) { [weak self] _, change in
            guard let application = change.newValue ?? nil else { return }
            let processID = application.processIdentifier
            let timestamp = ProcessInfo.processInfo.systemUptime
            Task { @MainActor [weak self] in
                guard let self,
                      self.dismissibleSurface(forEventAt: timestamp) == context.surfaceID,
                      processID != context.foregroundProcessID,
                      NSWorkspace.shared.frontmostApplication?.processIdentifier == processID else { return }
                self.core.dismissSurface(surfaceId: context.surfaceID)
            }
        }
    }

    private func dismissIfOutside(_ event: NSEvent, context: DismissalContext) {
        guard dismissibleSurface(forEventAt: event.timestamp) == context.surfaceID else { return }
        // 使用点击发生时的位置，避免延迟回调被鼠标随后移动的位置误导。
        let location = event.window.map { $0.convertPoint(toScreen: event.locationInWindow) } ?? event.locationInWindow
        if !panel.frame.contains(location) { core.dismissSurface(surfaceId: context.surfaceID) }
    }

    private func stopWatchingDismissal() {
        dismissal = nil
        if let localMouseMonitor { NSEvent.removeMonitor(localMouseMonitor) }
        if let globalMouseMonitor { NSEvent.removeMonitor(globalMouseMonitor) }
        activationObserver?.invalidate()
        localMouseMonitor = nil
        globalMouseMonitor = nil
        activationObserver = nil
    }

    func refreshPosition() {
        if panel.isVisible { reposition() }
    }

    /// 在剩余长度内按对齐取偏移：-1 靠起始边、0 居中、1 靠末端边，靠边时留出 edgeMargin。
    private func alignedOffset(_ available: CGFloat, _ alignment: Int8) -> CGFloat {
        let available = max(0, available)
        let inset = min(edgeMargin, available / 2)
        switch alignment {
        case ..<0: return inset
        case 0: return available / 2
        default: return available - inset
        }
    }

    private func reposition() {
        let mouse = NSEvent.mouseLocation
        guard let screen = NSScreen.screens.first(where: { $0.frame.contains(mouse) }) ?? NSScreen.main else { return }
        let size = panel.frame.size
        let visible = screen.visibleFrame
        guard let position = positions.first(where: { $0.value == model.settings.candidatePosition }) else { return }
        panel.setFrameOrigin(NSPoint(
            x: (visible.minX + alignedOffset(visible.width - size.width, position.horizontal)).rounded(),
            // AppKit 从屏幕底部向上计数，靠底即靠起始边。
            y: (visible.minY + alignedOffset(visible.height - size.height, -1)).rounded()
        ))
    }
}

private final class NonActivatingPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}
