import AppKit
import SwiftUI

/// 拖动即移动窗口的区域。设置窗口隐藏了标题栏，用它作为顶部页签栏的背景来保留拖动窗口的能力。
struct WindowDragArea: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView { DragView() }
    func updateNSView(_ nsView: NSView, context: Context) {}

    private final class DragView: NSView {
        override func mouseDown(with event: NSEvent) {
            window?.performDrag(with: event)
        }
    }
}
