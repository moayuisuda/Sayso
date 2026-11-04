import SwiftUI

/// 底部操作栏：左侧显示状态，右侧放操作或按键提示。候选框、扩展列表与扩展页签共用。
/// 单行时高 38；右侧内容换行时随之增高。
struct ActionBar<Leading: View, Trailing: View>: View {
    @ViewBuilder let leading: Leading
    @ViewBuilder let trailing: Trailing

    var body: some View {
        HStack(spacing: 12) {
            leading
            Spacer(minLength: 12)
            trailing
        }
        .font(.system(size: 12))
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .frame(minHeight: 38)
        .background(Theme.cardFill)
        .overlay(alignment: .top) {
            Rectangle().fill(Theme.hairline).frame(height: 1)
        }
    }
}
