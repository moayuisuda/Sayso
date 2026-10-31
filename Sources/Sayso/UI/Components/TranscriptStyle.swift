import SwiftUI

/// 识别原文、处理中占位与结果共用正文样式，高亮不改变字号或字重。
extension View {
    func transcriptStyle(secondary: Bool = false) -> some View {
        font(.system(size: secondary ? 13 : 15, weight: .regular))
            .lineSpacing(4)
            .foregroundStyle(secondary ? .secondary : .primary)
    }
}
