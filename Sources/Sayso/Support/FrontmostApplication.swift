import AppKit
import SaysoCore

extension AppInfo {
    /// 当前前台应用；前台是 Sayso 自身时为 nil。
    static func frontmost() -> AppInfo? {
        guard let app = NSWorkspace.shared.frontmostApplication,
              app.bundleIdentifier != Bundle.main.bundleIdentifier
        else { return nil }
        return AppInfo(id: app.bundleIdentifier, name: app.localizedName)
    }

    /// 正在运行、出现在 Dock 中的应用，不含 Sayso 自身。
    static func running() -> [AppInfo] {
        NSWorkspace.shared.runningApplications
            .filter { $0.activationPolicy == .regular && $0.bundleIdentifier != Bundle.main.bundleIdentifier }
            .map { AppInfo(id: $0.bundleIdentifier, name: $0.localizedName) }
    }
}
