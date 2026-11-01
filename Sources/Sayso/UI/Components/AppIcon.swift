import AppKit
import SwiftUI

/// 应用自身的图标，均由 scripts/render-icons.swift 从 Resources/AppIcon.svg 生成并随应用打包。
enum AppIcon {
    /// 菜单栏图标：主体实心、字形镂空的单色模板，系统按菜单栏明暗着色。
    static let menuBar: NSImage = {
        let image = Bundle.main.image(forResource: "MenuBarIcon")!
        image.size = NSSize(width: 16, height: 16)
        image.isTemplate = true
        return image
    }()
}

/// 彩色应用图标，裁去 macOS 图标网格四周的留白，让主体占满 `size`。
struct AppIconView: View {
    var size: CGFloat = 18

    /// AppIcon.svg 画布 1024，主体 824。
    private static let bodyScale: CGFloat = 1024 / 824

    var body: some View {
        Image(nsImage: NSApp.applicationIconImage)
            .resizable()
            .interpolation(.high)
            .frame(width: size * Self.bodyScale, height: size * Self.bodyScale)
            .frame(width: size, height: size)
    }
}
