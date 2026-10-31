import SwiftUI

/// 模型配置共用一个滚动容器，各分区只负责自己的字段和交互。
struct ModelSettingsView: View {
    let model: CoreModel

    var body: some View {
        SettingsPage {
            RecognitionSettingsView(model: model)
            TranslationSettingsView(model: model)
        }
    }
}
