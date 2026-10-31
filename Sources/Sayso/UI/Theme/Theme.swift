import AppKit
import SwiftUI

/// 全局视觉规范。候选框与设置窗口只从这里取颜色、圆角与尺寸，整体风格参照 Raycast：
/// 半透明面板、细描边、紧凑的列表行、键帽式快捷键提示，主题色为 Indigo。
enum Theme {
    /// 主题色 Indigo #3F51B5，用于选中态、开关、主按钮等填充。
    static let accent = Color(rgb: 0x3F51B5)
    /// 文字与图标上的主题色：深色模式下提亮，保证在深色背景上的对比度。
    static let accentForeground = Color.adaptive(light: NSColor(rgb: 0x3F51B5), dark: NSColor(rgb: 0x9FA8DA))
    /// 翻译块、词条等高亮内容的底色。
    static let highlightFill = Color.adaptive(
        light: NSColor(rgb: 0x3F51B5, alpha: 0.13),
        dark: NSColor(rgb: 0x7986CB, alpha: 0.32)
    )
    /// 选中行、选中页签的底色。
    static let selectionFill = Color.adaptive(
        light: NSColor(rgb: 0x3F51B5, alpha: 0.12),
        dark: NSColor(rgb: 0x7986CB, alpha: 0.26)
    )
    /// 悬停行的底色。
    static let hoverFill = Color.primary.opacity(0.06)
    /// 设置卡片、操作栏等次级表面的底色。
    static let cardFill = Color.primary.opacity(0.035)
    /// 键帽底色。
    static let keycapFill = Color.primary.opacity(0.09)
    /// 卡片、输入框等元素的细描边。
    static let hairline = Color.primary.opacity(0.1)
    /// 悬浮面板外描边：在任意桌面背景上勾出面板边缘。
    static let panelStroke = Color.white.opacity(0.14)
    /// 录音时面板边缘流光的色彩：以主题色 Indigo 为主，掺入相邻的紫与蓝让光团流动时有色相变化，
    /// 浅色高光在深色模式下发亮。
    static let glow: [Color] = [
        accent,
        Color(rgb: 0x7C4DFF),
        Color(rgb: 0x536DFE),
        Color(rgb: 0xC5CAE9),
        Color(rgb: 0x448AFF),
        Color(rgb: 0x8C9EFF),
    ]

    enum Radius {
        static let panel: CGFloat = 12
        static let card: CGFloat = 10
        static let row: CGFloat = 8
        static let control: CGFloat = 6
        static let keycap: CGFloat = 4
    }
}

extension Color {
    init(rgb: UInt32, opacity: Double = 1) {
        self.init(nsColor: NSColor(rgb: rgb, alpha: opacity))
    }

    /// 随系统深浅色外观切换的颜色。
    static func adaptive(light: NSColor, dark: NSColor) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? dark : light
        })
    }
}

extension NSColor {
    convenience init(rgb: UInt32, alpha: CGFloat = 1) {
        self.init(
            srgbRed: CGFloat((rgb >> 16) & 0xFF) / 255,
            green: CGFloat((rgb >> 8) & 0xFF) / 255,
            blue: CGFloat(rgb & 0xFF) / 255,
            alpha: alpha
        )
    }
}
