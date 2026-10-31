import SwiftUI

/// 圆角方形图标底板：白色 SF Symbol + 渐变底色，用于扩展、应用等的标识。
struct IconTile: View {
    let systemImage: String
    var color: Color = Theme.accent
    var size: CGFloat = 24

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: size * 0.26, style: .continuous)
        shape
            .fill(color.gradient)
            .overlay(shape.strokeBorder(.white.opacity(0.18), lineWidth: 0.5))
            .overlay(
                Image(systemName: systemImage)
                    .font(.system(size: size * 0.5, weight: .semibold))
                    .foregroundStyle(.white)
            )
            .frame(width: size, height: size)
    }
}
