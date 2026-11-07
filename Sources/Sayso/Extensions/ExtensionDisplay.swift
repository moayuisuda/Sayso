import SaysoCore

extension Capability {
    /// 能力说明只在 core 定义一份。
    var localizedDescription: String { capabilityDescription(capability: self) }

    var icon: String {
        switch self {
        case .blocks: return "rectangle.stack"
        case .store: return "externaldrive"
        case .insert: return "text.insert"
        case .llm: return "sparkles"
        case .publish: return "dot.radiowaves.left.and.right"
        case .fs: return "folder"
        case .context: return "text.bubble"
        case .shell: return "terminal"
        }
    }
}

extension InputSource: Identifiable {
    static let allCases: [InputSource] = [.speech, .selection]

    public var id: Self { self }

    var displayName: String {
        switch self {
        case .speech: return L("语音", "Speech")
        case .selection: return L("选中文本", "Selection")
        }
    }
}

extension ExtensionInfo {
    var kindName: String { builtIn ? L("内置", "Built-in") : L("外部", "External") }
}
