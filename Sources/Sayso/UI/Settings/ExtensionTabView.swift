import SaysoCore
import SwiftUI

/// 扩展页签：顶部搜索栏、core 按清单模板渲染好的记录列表、底部计数栏。打开页签不运行插件。
struct ExtensionTabView: View {
    let model: CoreModel
    let manifest: Manifest
    let tab: TabSpec

    @State private var query = ""
    @State private var pendingDeletion: TabItem?
    @State private var errorMessage: String?

    var body: some View {
        VStack(spacing: 0) {
            SearchField(text: $query, prompt: L("搜索\(tab.title)…", "Search \(tab.title)…"))
            Divider()
            content
        }
        .confirmationDialog(
            L("删除这条记录？", "Delete this item?"),
            isPresented: Binding(get: { pendingDeletion != nil }, set: { if !$0 { pendingDeletion = nil } }),
            presenting: pendingDeletion
        ) { item in
            Button(L("删除", "Delete"), role: .destructive) { delete(item) }
        }
        .alert(
            L("操作失败", "Something went wrong"),
            isPresented: Binding(get: { errorMessage != nil }, set: { if !$0 { errorMessage = nil } })
        ) {
            Button("OK") {}
        } message: {
            Text(errorMessage ?? "")
        }
    }

    /// 依赖数据版本，扩展数据变化后重新读取。
    private var items: Result<TabItems, Error> {
        _ = model.storeRevision(manifest.id)
        return Result { try model.core.tabItems(extensionId: manifest.id, query: query) }
    }

    @ViewBuilder
    private var content: some View {
        switch items {
        case .failure(let error):
            ContentUnavailableView(
                L("无法读取数据", "Unable to read data"),
                systemImage: "exclamationmark.triangle",
                description: Text(describe(error))
            )
            .frame(maxHeight: .infinity)
        case .success(let items) where items.total == 0:
            ContentUnavailableView(
                tab.title,
                systemImage: tab.icon ?? "puzzlepiece.extension",
                description: Text(manifest.description ?? "")
            )
            .frame(maxHeight: .infinity)
        case .success(let items):
            Group {
                if items.items.isEmpty {
                    ContentUnavailableView.search(text: query)
                } else {
                    ScrollView {
                        LazyVStack(spacing: 2) {
                            ForEach(items.items, id: \.key) { item in
                                ListRow { row(for: item) }
                            }
                        }
                        .padding(8)
                    }
                }
            }
            .frame(maxHeight: .infinity)
            ActionBar {
                Label(tab.title, systemImage: tab.icon ?? "puzzlepiece.extension")
                    .foregroundStyle(.secondary)
            } trailing: {
                Text(query.isEmpty
                     ? L("\(items.total) 项", "\(items.total) items")
                     : L("\(items.items.count) / \(items.total) 项", "\(items.items.count) of \(items.total) items"))
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func row(for item: TabItem) -> some View {
        BlockView(block: item.block, placement: .tab) { action in
            if case .delete = action.kind {
                pendingDeletion = item
            } else {
                model.core.performTabAction(extensionId: manifest.id, recordKey: item.key, action: action)
            }
        }
    }

    private func delete(_ item: TabItem) {
        do {
            try model.core.deleteRecord(extensionId: manifest.id, key: item.key)
        } catch {
            errorMessage = describe(error)
        }
    }
}
