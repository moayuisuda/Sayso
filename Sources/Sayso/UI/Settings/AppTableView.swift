import SaysoCore
import SwiftUI

/// 「按应用配置」页签：顶部是固定的默认行，下方每行左边选应用、右边填文本，末尾加号加一行。读写都交给 core。
struct AppTableView: View {
    private static let defaultRow = "default"

    let model: CoreModel
    let manifest: Manifest
    let spec: AppTableSpec

    @State private var errorMessage: String?

    /// 依赖数据版本，扩展数据变化后重新读取。
    private var table: Result<AppTable, Error> {
        _ = model.storeRevision(manifest.id)
        return Result { try model.core.appTable(extensionId: manifest.id) }
    }

    var body: some View {
        SettingsPage {
            switch table {
            case .failure(let error):
                ContentUnavailableView(
                    L("无法读取数据", "Unable to read data"),
                    systemImage: "exclamationmark.triangle",
                    description: Text(describe(error))
                )
            case .success(let table):
                if let description = manifest.description {
                    Text(description)
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
                SettingsSection {
                    textRow(key: Self.defaultRow, app: nil, text: table.defaultText) { _ in
                        Text(L("默认", "Default"))
                            .help(L("当前应用不在下方列表中时使用", "Used when the current app isn't listed below"))
                    }
                    Divider()
                    ForEach(table.rows, id: \.key) { row in
                        textRow(key: row.key, app: row.app, text: row.text) { text in
                            appPicker(for: row, text: text, in: table)
                        }
                    }
                    Button {
                        perform { try model.core.addAppRow(extensionId: manifest.id) }
                    } label: {
                        Image(systemName: "plus")
                    }
                    .buttonStyle(.borderless)
                    .help(L("添加应用", "Add an app"))
                }
            }
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

    private func textRow<Leading: View>(
        key: String,
        app: AppInfo?,
        text: String,
        @ViewBuilder leading: @escaping (String) -> Leading
    ) -> some View {
        AppTableRow(saved: text, placeholder: spec.placeholder, maxLength: Int(spec.maxLength), leading: leading) {
            save(key: key, app: app, text: $0)
        }
        .id(key)
    }

    /// 已被其他行选中的应用不可选。
    private func appPicker(for row: AppRow, text: String, in table: AppTable) -> some View {
        let taken = Set(table.rows.filter { $0.key != row.key }.compactMap { $0.app?.id })
        return HStack(spacing: 4) {
            Picker("", selection: Binding(
                get: { row.app?.id },
                set: { id in save(key: row.key, app: table.apps.first { $0.id == id }, text: text) }
            )) {
                Text(L("选择应用", "Choose an app")).tag(String?.none)
                ForEach(table.apps, id: \.id) { app in
                    Text(app.name ?? app.id ?? "").tag(app.id).selectionDisabled(app.id.map(taken.contains) ?? false)
                }
            }
            .labelsHidden()
            Button {
                perform { try model.core.deleteRecord(extensionId: manifest.id, key: row.key) }
            } label: {
                Image(systemName: "minus.circle")
            }
            .buttonStyle(.borderless)
            .help(L("删除这一行", "Remove this row"))
        }
    }

    private func save(key: String, app: AppInfo?, text: String) {
        perform { try model.core.saveAppRow(extensionId: manifest.id, key: key, app: app, text: text) }
    }

    private func perform(_ change: () throws -> Void) {
        do {
            try change()
        } catch {
            errorMessage = describe(error)
        }
    }
}

/// 表的一行：左列由调用方提供，可拿到正在编辑的文本；右列是文本框，停止输入片刻后保存。
private struct AppTableRow<Leading: View>: View {
    private static var leadingWidth: CGFloat { 190 }

    let saved: String
    let placeholder: String?
    let maxLength: Int
    @ViewBuilder let leading: (String) -> Leading
    let save: (String) -> Void

    @State private var text: String

    init(
        saved: String,
        placeholder: String?,
        maxLength: Int,
        @ViewBuilder leading: @escaping (String) -> Leading,
        save: @escaping (String) -> Void
    ) {
        self.saved = saved
        self.placeholder = placeholder
        self.maxLength = maxLength
        self.leading = leading
        self.save = save
        _text = State(initialValue: saved)
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            leading(text)
                .frame(width: Self.leadingWidth, alignment: .leading)
            TextField("", text: $text, prompt: placeholder.map { Text($0) }, axis: .vertical)
                .lineLimit(2...4)
                .labelsHidden()
                .onChange(of: text) {
                    if text.count > maxLength { text = String(text.prefix(maxLength)) }
                }
                .task(id: text) {
                    try? await Task.sleep(for: .milliseconds(400))
                    if !Task.isCancelled { commit() }
                }
                .onDisappear(perform: commit)
        }
    }

    private func commit() {
        if text != saved { save(text) }
    }
}
