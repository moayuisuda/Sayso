import SwiftUI

/// 设置页：内容限宽居中、可滚动，由若干 `SettingsSection` 纵向组成。
struct SettingsPage<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                content
            }
            .frame(maxWidth: 600)
            .padding(.horizontal, 32)
            .padding(.vertical, 28)
            .frame(maxWidth: .infinity)
        }
    }
}

/// 设置分组：可选标题 + 圆角卡片，卡片内的行纵向排列。
struct SettingsSection<Content: View>: View {
    let title: String?
    let content: Content

    init(_ title: String? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let title {
                Text(title)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .padding(.leading, 4)
            }
            VStack(alignment: .leading, spacing: 14) {
                content
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.cardFill, in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous)
                    .strokeBorder(Theme.hairline)
            )
        }
    }
}

/// 设置行：左列为右对齐的标题，右列为控件与说明。所有行共用标题列宽，控件左边缘对齐。
struct SettingsRow<Control: View>: View {
    static var labelWidth: CGFloat { 150 }

    let title: String
    var caption: String?
    @ViewBuilder let control: Control

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            Text(title)
                .multilineTextAlignment(.trailing)
                .frame(width: Self.labelWidth, alignment: .trailing)
            VStack(alignment: .leading, spacing: 6) {
                control
                if let caption {
                    Text(caption)
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

extension View {
    /// 设置页中输入框与选择器的统一宽度。
    func settingsControlWidth() -> some View {
        frame(maxWidth: 300, alignment: .leading)
    }
}
