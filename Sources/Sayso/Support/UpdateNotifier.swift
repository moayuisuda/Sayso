import AppKit
import os
import SaysoCore
import UserNotifications

/// 发现新版本时发送系统通知，点击通知打开发布页。首次发送时请求通知权限，用户拒绝则不再提醒。
final class UpdateNotifier: NSObject, UNUserNotificationCenterDelegate {
    private static let urlKey = "url"
    private let center = UNUserNotificationCenter.current()
    private let logger = Logger(subsystem: "com.sayso.app", category: "update")

    override init() {
        super.init()
        // 须在启动阶段设置，点击通知启动或激活应用时才能收到回调。
        center.delegate = self
    }

    func notify(_ release: AppRelease) {
        Task {
            do {
                guard try await center.requestAuthorization(options: [.alert, .sound]) else { return }
                let content = UNMutableNotificationContent()
                content.title = L("Sayso 有新版本", "Sayso update available")
                content.body = L("\(release.version) 已发布，点击前往下载。", "Version \(release.version) is out. Click to download.")
                content.userInfo = [Self.urlKey: release.url]
                try await center.add(UNNotificationRequest(identifier: "update-\(release.version)", content: content, trigger: nil))
            } catch {
                logger.error("update notification failed: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        if let url = (response.notification.request.content.userInfo[Self.urlKey] as? String).flatMap(URL.init(string:)) {
            NSWorkspace.shared.open(url)
        }
        completionHandler()
    }

    /// 设置窗口在前台时也显示横幅。
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .sound])
    }
}
