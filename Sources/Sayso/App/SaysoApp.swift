import AppKit
import SaysoCore
import SwiftUI

@main
struct SaysoApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        MenuBarExtra {
            MenuBarContent(app: delegate.app)
        } label: {
            MenuBarIcon(model: delegate.app.model)
        }

        Window("Sayso", id: SettingsView.windowID) {
            SettingsView(app: delegate.app)
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 800, height: 600)
        .windowResizability(.contentMinSize)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let app = AppModel()

    func applicationDidFinishLaunching(_ notification: Notification) {
        // 菜单栏应用，不显示 Dock 图标。
        NSApp.setActivationPolicy(.accessory)
        app.launch()
    }

    /// 双击扩展包时由系统交给本应用安装。
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls where url.pathExtension == packageFileExtension() {
            app.packages.install(from: url)
        }
    }
}

private struct MenuBarIcon: View {
    let model: CoreModel

    var body: some View {
        if case .recording = model.session {
            Image(systemName: "mic.fill")
        } else {
            Image(nsImage: AppIcon.menuBar)
        }
    }
}

private struct MenuBarContent: View {
    let app: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        let model = app.model
        let settings = model.settings
        ForEach(model.enabledCommands) { command in
            if let route = model.route(of: command.key) {
                Text("\(command.spec.title)：\(route.keycaps(trigger: settings.triggerKey).joined(separator: " "))")
            }
        }
        Text("\(name(settings.targetLanguage)) ← \(name(settings.nativeLanguage))")
        Divider()
        Button(L("生词本…", "Wordbook…")) { open(.extension(AppModel.wordbookID)) }
        Button(L("设置…", "Settings…")) { open(.general) }
            .keyboardShortcut(",")
        Divider()
        Button(L("退出 Sayso", "Quit Sayso")) { NSApp.terminate(nil) }
            .keyboardShortcut("q")
    }

    private func name(_ language: Language) -> String {
        supportedLanguages().first { $0.language == language }?.nativeName ?? ""
    }

    private func open(_ tab: SettingsNavigation.Tab) {
        app.navigation.tab = tab
        openWindow(id: SettingsView.windowID)
        NSApp.activate(ignoringOtherApps: true)
    }
}
