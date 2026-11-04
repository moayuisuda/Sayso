import SaysoCore
import SwiftUI

/// 设置窗口：顶部为页签栏（位于隐藏的标题栏处），下方为当前页签的页面。
struct SettingsView: View {
    static let windowID = "settings"

    let app: AppModel

    private var model: CoreModel { app.model }

    var body: some View {
        @Bindable var navigation = app.navigation
        VStack(spacing: 0) {
            SettingsTabBar(items: items, selection: $navigation.tab)
            Divider()
            page(for: navigation.tab)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .ignoresSafeArea(.container, edges: .top)
        .frame(minWidth: 720, minHeight: 520)
        .tint(Theme.accent)
        // 界面语言跟随本地语言，切换后整窗刷新。
        .id(model.settings.nativeLanguage)
    }

    /// 固定页签在前；每个声明了页签的已启用扩展追加一个独立页签。
    private var items: [SettingsTabItem] {
        [
            SettingsTabItem(tab: .general, title: L("通用", "General"), icon: "gearshape"),
            SettingsTabItem(tab: .models, title: L("模型配置", "Models"), icon: "character.bubble"),
            SettingsTabItem(tab: .extensions, title: L("扩展", "Extensions"), icon: "puzzlepiece.extension"),
        ] + model.tabExtensions.compactMap { info in
            info.manifest.tab.map { SettingsTabItem(tab: .extension(info.manifest.id), title: $0.title, icon: $0.icon ?? "puzzlepiece.extension") }
        }
    }

    @ViewBuilder
    private func page(for tab: SettingsNavigation.Tab) -> some View {
        switch tab {
        case .general:
            GeneralSettingsView(model: model, hotkeys: app.hotkeys, pronouncer: app.platform.pronouncer)
        case .models:
            ModelSettingsView(model: model)
        case .extensions:
            ExtensionsSettingsView(model: model, packages: app.packages)
        case .extension(let id):
            if let manifest = model.tabExtensions.first(where: { $0.manifest.id == id })?.manifest, let tab = manifest.tab {
                ExtensionTabView(model: model, manifest: manifest, tab: tab)
                    .id(id)
            }
        }
    }
}

private struct SettingsTabItem: Identifiable {
    let tab: SettingsNavigation.Tab
    let title: String
    let icon: String

    var id: SettingsNavigation.Tab { tab }
}

/// 页签栏：第一行与窗口按钮同高，居中显示当前页签名；第二行为图标 + 名称的页签按钮。整个区域可拖动窗口。
private struct SettingsTabBar: View {
    let items: [SettingsTabItem]
    @Binding var selection: SettingsNavigation.Tab

    var body: some View {
        VStack(spacing: 2) {
            Text(items.first { $0.tab == selection }?.title ?? "Sayso")
                .font(.system(size: 13, weight: .semibold))
                .frame(height: 28)
            HStack(spacing: 4) {
                ForEach(items) { item in
                    TabButton(item: item, isSelected: item.tab == selection) { selection = item.tab }
                }
            }
            .padding(.bottom, 8)
        }
        .frame(maxWidth: .infinity)
        .background(WindowDragArea())
        .background(.bar)
    }
}

private struct TabButton: View {
    let item: SettingsTabItem
    let isSelected: Bool
    let select: () -> Void

    @State private var isHovering = false

    var body: some View {
        Button(action: select) {
            VStack(spacing: 3) {
                Image(systemName: item.icon)
                    .font(.system(size: 16, weight: .medium))
                    .frame(height: 20)
                Text(item.title)
                    .font(.system(size: 11, weight: .medium))
            }
            .foregroundStyle(isSelected ? Theme.accentForeground : Color.secondary)
            .padding(.horizontal, 8)
            .frame(minWidth: 72, minHeight: 48)
            .background(fill, in: RoundedRectangle(cornerRadius: Theme.Radius.row, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { isHovering = $0 }
    }

    private var fill: Color {
        if isSelected { return Theme.selectionFill }
        return isHovering ? Theme.hoverFill : .clear
    }
}
