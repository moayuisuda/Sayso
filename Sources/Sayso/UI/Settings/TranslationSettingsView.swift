import SwiftUI
import SaysoCore

struct TranslationSettingsView: View {
    let model: CoreModel

    @State private var testState: TestState = .idle

    private enum TestState: Equatable {
        case idle
        case running
        case succeeded(String)
        case failed(String)
    }

    var body: some View {
        SettingsPage {
            SettingsSection(L("模型", "Model")) {
                SettingsRow(title: L("服务", "Provider")) {
                    Picker(L("服务", "Provider"), selection: model.binding(\.llmProvider)) {
                        ForEach(providers, id: \.provider) { Text($0.displayName).tag($0.provider) }
                    }
                    .labelsHidden()
                    .fixedSize()
                }
                if providers.first(where: { $0.provider == model.settings.llmProvider })?.requiresApiKey == true {
                    SettingsRow(title: "API Key") {
                        SecureField("API Key", text: model.secret(.llm(provider: model.settings.llmProvider)))
                            .labelsHidden()
                            .settingsControlWidth()
                    }
                }
                SettingsRow(title: "Base URL") {
                    TextField("Base URL", text: model.binding(\.llmBaseUrl)).labelsHidden().settingsControlWidth()
                }
                SettingsRow(title: L("模型", "Model")) {
                    TextField(L("模型", "Model"), text: model.binding(\.llmModel)).labelsHidden().settingsControlWidth()
                }
                SettingsRow(
                    title: L("思考强度", "Reasoning"),
                    caption: L("只在一句话里夹杂了本地语言时调用，纯目标语言输入不会调用模型。翻译短句无需深度思考，选「关闭」最快；若服务不支持思考参数而报错，改回「模型默认」。",
                               "Only called when a sentence contains your native language. Short translations need no deep reasoning, so Off is fastest; if the service rejects the parameter, switch back to Model default.")
                ) {
                    Picker(L("思考强度", "Reasoning"), selection: model.binding(\.llmReasoning)) {
                        ForEach(ReasoningLevel.allCases, id: \.self) { Text($0.displayName).tag($0) }
                    }
                    .labelsHidden()
                    .fixedSize()
                }
            }

            SettingsSection(L("测试", "Test")) {
                SettingsRow(
                    title: L("连通性", "Connection"),
                    caption: L("用当前模型翻译一句示例，检查地址、密钥与模型名是否正确。",
                               "Translates a short sample with the current model to check the URL, key and model name.")
                ) {
                    HStack(spacing: 10) {
                        Button(L("测试翻译", "Run test"), action: runTest)
                            .buttonStyle(.borderedProminent)
                            .disabled(testState == .running || !isConfigured)
                        if testState == .running { ProgressView().controlSize(.small) }
                    }
                    switch testState {
                    case .succeeded(let output):
                        TestResult(text: output, systemImage: "checkmark.circle.fill", color: .green)
                    case .failed(let message):
                        TestResult(text: message, systemImage: "exclamationmark.triangle.fill", color: .red)
                    case .idle, .running:
                        EmptyView()
                    }
                }
            }
        }
    }

    private var providers: [LlmProviderInfo] { llmProviders() }

    /// 依赖 `settings`，设置或密钥变化后重新判断。
    private var isConfigured: Bool {
        _ = model.settings
        return model.core.llmConfigured()
    }

    private func runTest() {
        testState = .running
        Task {
            do {
                testState = .succeeded(try await model.core.testLlm())
            } catch {
                testState = .failed(describe(error))
            }
        }
    }
}

/// 测试结果：带状态色底的可选择文本。
private struct TestResult: View {
    let text: String
    let systemImage: String
    let color: Color

    var body: some View {
        Label {
            Text(text).textSelection(.enabled)
        } icon: {
            Image(systemName: systemImage).foregroundStyle(color)
        }
        .font(.system(size: 12))
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(color.opacity(0.1), in: RoundedRectangle(cornerRadius: Theme.Radius.control, style: .continuous))
    }
}

private extension ReasoningLevel {
    static let allCases: [ReasoningLevel] = [.modelDefault, .none, .low, .medium, .high]

    var displayName: String {
        switch self {
        case .modelDefault: return L("模型默认", "Model default")
        case .none: return L("关闭（最快）", "Off (fastest)")
        case .low: return L("低", "Low")
        case .medium: return L("中", "Medium")
        case .high: return L("高", "High")
        }
    }
}
