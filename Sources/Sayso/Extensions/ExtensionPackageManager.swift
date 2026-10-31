import AppKit
import SaysoCore
import UniformTypeIdentifiers

/// 扩展包的导入、导出与删除：文件选择、安装确认与错误提示。打包、解包与安装由 core 完成。
@MainActor
final class ExtensionPackageManager {
    static let contentType = UTType(filenameExtension: packageFileExtension()) ?? .data

    private let model: CoreModel

    init(model: CoreModel) {
        self.model = model
    }

    /// 选择扩展包文件并安装。
    func chooseAndInstall() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [Self.contentType]
        panel.allowsMultipleSelection = false
        NSApp.activate(ignoringOtherApps: true)
        guard panel.runModal() == .OK, let url = panel.url else { return }
        install(from: url)
    }

    /// 读取扩展包，向用户说明其内容与能力，确认后原样安装并启用。
    func install(from url: URL) {
        Task {
            do {
                let package = try await model.core.openPackage(path: url.path)
                guard confirmInstall(package) else {
                    model.core.discardPackage(token: package.token)
                    return
                }
                try model.core.installPackage(token: package.token)
            } catch {
                show(error, title: L("无法安装扩展", "Unable to install extension"))
            }
        }
    }

    /// 把扩展目录原样导出为扩展包，可直接发给他人安装。
    func export(_ id: String) {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [Self.contentType]
        panel.nameFieldStringValue = "\(id).\(packageFileExtension())"
        NSApp.activate(ignoringOtherApps: true)
        guard panel.runModal() == .OK, let destination = panel.url else { return }
        Task {
            do {
                try await model.core.exportPackage(extensionId: id, destination: destination.path)
                NSWorkspace.shared.activateFileViewerSelecting([destination])
            } catch {
                show(error, title: L("无法导出扩展", "Unable to export extension"))
            }
        }
    }

    func uninstall(_ id: String) {
        do {
            try model.core.uninstall(extensionId: id)
        } catch {
            show(error, title: L("无法删除扩展", "Unable to delete extension"))
        }
    }

    // MARK: - Private

    private func confirmInstall(_ package: PackageInfo) -> Bool {
        let manifest = package.manifest
        let size = ByteCountFormatter.string(fromByteCount: Int64(package.byteSize), countStyle: .file)
        var lines = [manifest.description ?? "", L("大小：\(size)", "Size: \(size)")]
        if package.replacing {
            lines.append(L("将替换已安装的版本，扩展保存的数据会保留。", "This replaces the installed version; its saved data is kept."))
        }
        lines.append(package.permissionSummary)
        lines.append(L("扩展包含可执行代码，会在你的电脑上运行。只安装你信任的来源。",
                       "The extension contains executable code that runs on your computer. Only install it from sources you trust."))

        let alert = NSAlert()
        alert.messageText = L("安装扩展「\(manifest.name)」？", "Install “\(manifest.name)”?")
        alert.informativeText = lines.filter { !$0.isEmpty }.joined(separator: "\n\n")
        alert.addButton(withTitle: package.replacing ? L("替换", "Replace") : L("安装", "Install"))
        alert.addButton(withTitle: L("取消", "Cancel"))
        NSApp.activate(ignoringOtherApps: true)
        return alert.runModal() == .alertFirstButtonReturn
    }

    private func show(_ error: Error, title: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = describe(error)
        alert.alertStyle = .warning
        NSApp.activate(ignoringOtherApps: true)
        alert.runModal()
    }
}
