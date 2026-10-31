import AppKit
import Observation
import SaysoCore

/// 设置窗口当前所在的页签，菜单栏可直接跳到某个扩展的页签。
@MainActor
@Observable
final class SettingsNavigation {
    enum Tab: Hashable {
        case general, models, extensions
        /// 扩展自己的页签，关联值为扩展 id。
        case `extension`(String)
    }

    var tab: Tab = .general
}

/// 组装应用的各个部件：平台适配层、core 镜像、热键监听、候选框与扩展包管理。
@MainActor
final class AppModel {
    /// 内置生词本扩展的 id，菜单栏直接打开它的页签。
    static let wordbookID = "sayso.wordbook"

    let platform: MacPlatform
    let model: CoreModel
    let packages: ExtensionPackageManager
    let hotkeys = HotkeyMonitor()
    let navigation = SettingsNavigation()
    private let panel: CandidatePanel

    init() {
        let platform = MacPlatform()
        self.platform = platform
        do {
            model = try CoreModel(platform: platform)
        } catch {
            fatalError("Sayso core failed to start: \(describe(error))")
        }
        packages = ExtensionPackageManager(model: model)
        panel = CandidatePanel(model: model)

        let model = model
        let core = model.core
        platform.onAudio = { core.pushAudio(pcm: $0) }
        let audioLevel = model.audioLevel
        platform.onLevel = { audioLevel.set($0) }
        model.onSessionChange = { [panel] in panel.update(for: $0) }
        hotkeys.isSessionActive = { core.isSessionActive() }
        hotkeys.candidateSurface = { [panel] timestamp in panel.dismissibleSurface(forEventAt: timestamp) }
        hotkeys.onAction = { action in
            switch action {
            case .route(let route): core.trigger(route: route)
            case .stop: core.stop()
            case .toggle(let routes): core.toggle(routes: routes)
            case .cancel: core.cancel()
            case .dismiss(let id): core.dismissSurface(surfaceId: id)
            }
        }
        syncHotkeysWithSettings()
    }

    func launch() {
        Task {
            if !Permissions.microphoneGranted { _ = await Permissions.requestMicrophone() }
            if !Permissions.accessibilityGranted { Permissions.promptAccessibility() }
            // 辅助功能授权后才能创建事件监听，授权前定期重试。
            while !hotkeys.start() {
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }

    /// 入口键、路由或扩展启停变化时同步到监听器：只监听绑定了已启用命令的路由键。
    private func syncHotkeysWithSettings() {
        withObservationTracking {
            hotkeys.style = model.settings.hotkeyStyle
            hotkeys.trigger = model.settings.triggerKey
            _ = model.extensions
            hotkeys.routeKeys = Set(model.core.activeRouteKeys())
        } onChange: { [weak self] in
            Task { @MainActor in self?.syncHotkeysWithSettings() }
        }
    }
}
