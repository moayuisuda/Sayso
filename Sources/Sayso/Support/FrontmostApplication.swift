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
}
