import Foundation

/// 应用数据目录：~/Library/Application Support/Sayso。目录内的布局由 core 管理。
enum AppPaths {
    static let root: URL = FileManager.default
        .urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("Sayso", isDirectory: true)
}
