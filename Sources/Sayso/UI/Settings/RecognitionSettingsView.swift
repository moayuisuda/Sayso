import SaysoCore
import SwiftUI

/// 识别服务设置：列出 core 注册的全部识别来源，按所选来源声明的配置项通用渲染。
struct RecognitionSettingsView: View {
    let model: CoreModel

    var body: some View {
        let sources = model.asrSources
        let selected = sources.first { $0.id == model.settings.asrSource }
        SettingsSection(L("语音模型", "Speech model")) {
            SettingsRow(
                title: L("识别服务", "Service"),
                caption: selected?.caption
            ) {
                Picker(L("识别服务", "Service"), selection: model.binding(\.asrSource)) {
                    ForEach(sources, id: \.id) { Text($0.title).tag($0.id) }
                    // 来源不可用时提示重新选择服务。
                    if selected == nil {
                        Text(L("不可用的服务", "Unavailable service")).tag(model.settings.asrSource)
                    }
                }
                .labelsHidden()
                .fixedSize()
            }
            if let selected {
                ForEach(selected.fields, id: \.key) { field in
                    SettingsRow(title: field.title, caption: field.caption) {
                        FieldControl(field: field, value: model.asrValue(source: selected.id, field: field.key))
                    }
                }
                if let link = selected.link, let url = URL(string: link.url) {
                    SettingsRow(title: "") { Link(link.title, destination: url) }
                }
            } else {
                SettingsRow(
                    title: L("所选服务不可用", "Selected service unavailable"),
                    caption: L("请选择可用的识别服务。",
                               "Choose an available recognition service.")
                ) { EmptyView() }
            }
        }
    }
}

/// 按配置项类型选择输入控件。
private struct FieldControl: View {
    let field: AsrField
    let value: Binding<String>

    var body: some View {
        switch field.kind {
        case .secret:
            SecureField(field.title, text: value).labelsHidden().settingsControlWidth()
        case .text:
            TextField(field.title, text: value, prompt: Text(field.defaultValue)).labelsHidden().settingsControlWidth()
        case .choice(let options):
            Picker(field.title, selection: value) {
                ForEach(options, id: \.value) { Text($0.title).tag($0.value) }
            }
            .labelsHidden()
            .fixedSize()
        }
    }
}
