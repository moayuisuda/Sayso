import AVFoundation
import Carbon.HIToolbox
import Observation
import os
import SaysoCore
import SwiftUI

/// core 的设置类型；与 SwiftUI 的 `Settings` 场景重名，统一用此名称。
typealias AppSettings = SaysoCore.Settings

/// core 状态在主线程上的镜像，供 SwiftUI 绑定。所有修改都交给 core，再由 core 推送回来。
@MainActor
@Observable
final class CoreModel {
    let core: Core
    private(set) var session: SessionState = .idle { didSet { onSessionChange(session) } }
    private(set) var settings: AppSettings
    private(set) var extensions: [ExtensionInfo] = []
    private(set) var problems: [CatalogProblem] = []
    /// 每个扩展的数据版本，数据变化时递增，页签据此刷新。
    private(set) var storeRevisions: [String: Int] = [:]
    /// 录音音量，0...1。音量只用于界面显示，不经过 core。
    var audioLevel: Float = 0
    @ObservationIgnored var onSessionChange: (SessionState) -> Void = { _ in }

    @ObservationIgnored private let logger = Logger(subsystem: "com.sayso.app", category: "core")

    init(platform: MacPlatform) throws {
        try installLogSink(sink: UnifiedLogSink())
        let observer = ObserverBridge()
        core = try Core(
            dataDir: AppPaths.root.path,
            defaults: PlatformDefaults(
                triggerKey: .rightOption,
                selectionRouteKey: UInt16(kVK_ANSI_T),
                speechRate: AVSpeechUtteranceDefaultSpeechRate
            ),
            platform: platform,
            observer: observer
        )
        settings = core.settings()
        Localization.native = settings.nativeLanguage
        observer.model = self
        reloadExtensions()
    }

    // MARK: 设置

    /// 修改设置并交给 core 规整、保存。
    func update(_ change: (inout AppSettings) -> Void) {
        var updated = settings
        change(&updated)
        applySettingsChange { try core.updateSettings(settings: updated) }
    }

    private func applySettingsChange(_ change: () throws -> Void) {
        do {
            try change()
        } catch {
            logger.error("saving settings failed: \(error.localizedDescription, privacy: .public)")
        }
        reloadSettings()
    }

    func binding<Value>(_ keyPath: WritableKeyPath<AppSettings, Value>) -> Binding<Value> {
        Binding(get: { self.settings[keyPath: keyPath] }, set: { value in self.update { $0[keyPath: keyPath] = value } })
    }

    /// 密钥存在钥匙串中，由 core 读写。读取时依赖 `settings`，保存后随设置变化通知刷新。
    func secret(_ slot: SecretSlot) -> Binding<String> {
        Binding(
            get: { _ = self.settings; return self.core.secret(slot: slot) },
            set: { self.core.setSecret(slot: slot, value: $0) }
        )
    }

    /// 识别来源配置项的取值。存在钥匙串还是设置中由 core 按配置项类型决定；读取依赖 `settings`，保存后随设置变化通知刷新。
    func asrValue(source: String, field: String) -> Binding<String> {
        Binding(
            get: { _ = self.settings; return self.core.asrValue(sourceId: source, fieldKey: field) },
            set: { value in
                do {
                    try self.core.setAsrValue(sourceId: source, fieldKey: field, value: value)
                } catch {
                    self.logger.error("saving settings failed: \(error.localizedDescription, privacy: .public)")
                }
            }
        )
    }

    /// 可选的识别来源：云端服务与通过 URL 接入的本地服务。
    var asrSources: [AsrSource] {
        core.asrSources()
    }

    var languagesAreValid: Bool { settings.targetLanguage != settings.nativeLanguage }

    // MARK: 扩展与命令

    func info(_ id: String) -> ExtensionInfo? {
        extensions.first { $0.manifest.id == id }
    }

    func setEnabled(_ enabled: Bool, for id: String) {
        do {
            try core.setEnabled(extensionId: id, enabled: enabled)
        } catch {
            logger.error("saving settings failed: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// 已启用且声明了页签的扩展，按注册顺序。
    var tabExtensions: [ExtensionInfo] {
        extensions.filter { $0.enabled && $0.manifest.tab != nil }
    }

    /// 已启用扩展的命令，按注册顺序。
    var enabledCommands: [EnabledCommand] {
        extensions.filter(\.enabled).flatMap { info in
            info.manifest.commands.map {
                EnabledCommand(key: CommandKey(extensionId: info.manifest.id, commandId: $0.id), spec: $0, extensionName: info.manifest.name)
            }
        }
    }

    func route(of command: CommandKey) -> RouteKey? {
        settings.commandBindings.first { $0.command == command }?.route
    }

    func input(of route: RouteKey) -> InputSource? {
        settings.routes.first { $0.key == route }?.input
    }

    /// 宿主只提交编辑意图，路由的继承、共享与清理由 core 统一处理。
    func setRoute(_ route: RouteKey?, of command: CommandKey) {
        applySettingsChange { try core.bindCommand(command: command, route: route) }
    }

    /// 路由的输入来源；同一路由上的全部命令收到同一份输入。
    func setInput(_ input: InputSource, of route: RouteKey) {
        applySettingsChange { try core.setRouteInput(route: route, input: input) }
    }

    func storeRevision(_ id: String) -> Int {
        storeRevisions[id, default: 0]
    }

    // MARK: core 推送

    fileprivate func apply(session: SessionState) {
        self.session = session
    }

    fileprivate func reloadSettings() {
        settings = core.settings()
        Localization.native = settings.nativeLanguage
    }

    fileprivate func reloadExtensions() {
        extensions = core.extensions()
        problems = core.extensionProblems()
    }

    fileprivate func storeChanged(_ id: String) {
        storeRevisions[id, default: 0] += 1
    }
}

/// 已启用扩展的一个命令。
struct EnabledCommand: Identifiable {
    let key: CommandKey
    let spec: CommandSpec
    let extensionName: String

    var id: CommandKey { key }
}

/// core 推送的状态按到达顺序转交主线程（主队列先进先出，保证会话状态不乱序）。
private final class ObserverBridge: CoreObserver, @unchecked Sendable {
    /// 只在主线程读写。
    weak var model: CoreModel?

    func sessionChanged(state: SessionState) {
        deliver { $0.apply(session: state) }
    }

    func extensionsChanged() {
        deliver { $0.reloadExtensions() }
    }

    func storeChanged(extensionId: String) {
        deliver { $0.storeChanged(extensionId) }
    }

    func settingsChanged() {
        deliver { $0.reloadSettings() }
    }

    private func deliver(_ update: @escaping @MainActor (CoreModel) -> Void) {
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                if let model = self.model { update(model) }
            }
        }
    }
}

/// core 的日志写入系统统一日志（subsystem com.sayso.app，category core）。
private final class UnifiedLogSink: LogSink {
    private let logger = Logger(subsystem: "com.sayso.app", category: "core")

    func log(level: LogLevel, target: String, message: String) {
        switch level {
        case .error: logger.error("[\(target, privacy: .public)] \(message, privacy: .public)")
        case .warn: logger.warning("[\(target, privacy: .public)] \(message, privacy: .public)")
        case .info: logger.info("[\(target, privacy: .public)] \(message, privacy: .public)")
        case .debug: logger.debug("[\(target, privacy: .public)] \(message, privacy: .public)")
        }
    }
}

extension CoreError {
    /// core 已按本地语言生成的错误信息。
    var message: String {
        switch self {
        case .InvalidManifest(let message), .InvalidInstruction(let message), .CapabilityNotGranted(let message),
             .ModelNotConfigured(let message), .InsertOutsideCommand(let message), .InvalidPackage(let message),
             .Llm(let message), .Recognition(let message), .Platform(let message), .Io(let message):
            return message
        }
    }
}

/// 显示给用户的错误信息：core 的错误使用其本地化信息。
func describe(_ error: Error) -> String {
    (error as? CoreError)?.message ?? error.localizedDescription
}
